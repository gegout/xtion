use crate::{Error, Result};

pub(crate) const HOST_MAGIC: u16 = 0x4d47;
pub(crate) const REPLY_MAGIC: u16 = 0x4252;
pub(crate) const START: u16 = 0x7100;
pub(crate) const DATA: u16 = 0x7200;
pub(crate) const END: u16 = 0x7500;
pub(crate) const HEADER: usize = 12;

pub(crate) fn word(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}
pub(crate) fn words(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub(crate) fn request(opcode: u16, id: u16, payload: &[u8]) -> Result<Vec<u8>> {
    if !payload.len().is_multiple_of(2) || payload.len() > 502 {
        return Err(Error::Protocol("invalid command payload length"));
    }
    let mut out = words(&[HOST_MAGIC, (payload.len() / 2) as u16, opcode, id]);
    out.extend_from_slice(payload);
    Ok(out)
}

/// Skip a fragmented or coalesced late response only when both identifiers
/// match the previous transaction. Keep all bytes belonging to this command.
pub(crate) fn discard_previous(bytes: &mut Vec<u8>, opcode: u16, id: u16) -> Result<bool> {
    if bytes.len() < 8
        || word(bytes, 0) != REPLY_MAGIC
        || word(bytes, 4) != opcode
        || word(bytes, 6) != id
    {
        return Ok(false);
    }
    let n = usize::from(word(bytes, 2)) * 2;
    if !(2..=2048).contains(&n) {
        return Err(Error::Protocol("late reply word count"));
    }
    if bytes.len() < 8 + n {
        return Ok(true);
    }
    bytes.drain(..8 + n);
    Ok(bytes.is_empty())
}

/// None means that another USB IN read is needed.
pub(crate) fn reply(data: &[u8], opcode: u16, id: u16) -> Result<Option<&[u8]>> {
    if data.len() < 8 {
        return Ok(None);
    }
    if word(data, 0) != REPLY_MAGIC {
        return Err(Error::Protocol("reply magic"));
    }
    if word(data, 4) != opcode {
        return Err(Error::Protocol("reply opcode"));
    }
    if word(data, 6) != id {
        return Err(Error::Protocol("reply request ID"));
    }
    let len = usize::from(word(data, 2)) * 2;
    if !(2..=2048).contains(&len) {
        return Err(Error::Protocol("reply word count"));
    }
    if data.len() < 8 + len {
        return Ok(None);
    }
    if data.len() != 8 + len {
        return Err(Error::Protocol("trailing reply bytes"));
    }
    let status = word(data, 8);
    if status != 0 {
        return Err(Error::Nack(status));
    }
    Ok(Some(&data[10..]))
}

#[derive(Debug, Clone, Copy)]
pub struct FirmwareVersion {
    pub major: u8,
    pub minor: u8,
    pub build: u16,
    pub chip: u32,
    pub fpga: u16,
    pub system: u16,
}
impl FirmwareVersion {
    pub(crate) fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 12 {
            return Err(Error::Protocol("short firmware version"));
        }
        let major = data[1];
        let raw_build = word(data, 2);
        let build = if major >= 5 {
            let mut result = 0;
            for shift in [12, 8, 4, 0] {
                let digit = (raw_build >> shift) & 15;
                if digit > 9 {
                    return Err(Error::Protocol("invalid BCD firmware build"));
                }
                result = result * 10 + digit;
            }
            result
        } else {
            raw_build
        };
        Ok(Self {
            major,
            minor: data[0],
            build,
            chip: u32::from_le_bytes(data[4..8].try_into().unwrap()),
            fpga: word(data, 8),
            system: word(data, 10),
        })
    }
    pub(crate) fn check(self) -> Result<()> {
        if self.major != 5
            || !(1..=8).contains(&self.minor)
            || ![0x00202020, 0x00212020].contains(&self.chip)
        {
            return Err(Error::Unsupported(format!(
                "firmware {}.{}.{}, chip {:#010x}; implemented subset requires PS1080/PS1080A6 firmware 5.1–5.8",
                self.major, self.minor, self.build, self.chip
            )));
        }
        Ok(())
    }
    pub(crate) fn bulk_alt(self) -> u8 {
        if self.minor >= 5 { 0 } else { 1 }
    }
}

#[derive(Debug)]
pub(crate) struct Packet {
    pub kind: u16,
    pub sequence: u16,
    pub timestamp: u32,
    pub payload: Vec<u8>,
}

/// Streaming parser: USB transfers can split headers, split pixels, or contain many packets.
#[derive(Default)]
pub(crate) struct PacketDecoder {
    buffer: Vec<u8>,
    cursor: usize,
    pub discarded_bytes: u64,
}
impl PacketDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<()> {
        self.buffer.drain(..self.cursor);
        self.cursor = 0;
        if self.buffer.len() + bytes.len() > 2 * 1024 * 1024 {
            self.buffer.clear();
            return Err(Error::Protocol("stream parser buffer limit"));
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }
    pub fn next(&mut self) -> Result<Option<Packet>> {
        let magic = REPLY_MAGIC.to_le_bytes();
        let buffer = &self.buffer[self.cursor..];
        let mut skip = 0;
        while buffer.len() - skip >= 2 && buffer[skip..skip + 2] != magic {
            skip += 1;
        }
        if skip > 0 {
            self.cursor += skip;
            self.discarded_bytes += skip as u64;
        }
        let buffer = &buffer[skip..];
        if buffer.len() < HEADER {
            return Ok(None);
        }
        let len = usize::from(u16::from_be_bytes([buffer[6], buffer[7]]));
        if len < HEADER {
            self.cursor += 2;
            self.discarded_bytes += 2;
            return Err(Error::Protocol("packet length smaller than header"));
        }
        if buffer.len() < len {
            return Ok(None);
        }
        let p = Packet {
            kind: word(buffer, 2),
            sequence: word(buffer, 4),
            timestamp: u32::from_le_bytes(buffer[8..12].try_into().unwrap()),
            payload: buffer[HEADER..len].to_vec(),
        };
        self.cursor += len;
        Ok(Some(p))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[test]
    fn command_and_fragmented_reply() {
        assert_eq!(
            request(2, 7, &[18, 0]).unwrap(),
            [0x47, 0x4d, 1, 0, 2, 0, 7, 0, 18, 0]
        );
        let r = [0x52, 0x42, 2, 0, 2, 0, 7, 0, 0, 0, 30, 0];
        for n in 0..r.len() {
            assert!(reply(&r[..n], 2, 7).unwrap().is_none());
        }
        assert_eq!(reply(&r, 2, 7).unwrap().unwrap(), [30, 0]);
        assert!(reply(&r, 3, 7).is_err());
        assert!(reply(&r, 2, 8).is_err());
        for offset in [0, 2, 8] {
            let mut invalid = r;
            invalid[offset] = if offset == 2 { 0 } else { 0xff };
            assert!(reply(&invalid, 2, 7).is_err());
        }
        assert!(request(0, 0, &[1]).is_err());
    }
    #[test]
    fn firmware_byte_order_and_gate() {
        let v =
            FirmwareVersion::parse(&[8, 5, 0x99, 0x05, 0x20, 0x20, 0x20, 0, 5, 0, 1, 0]).unwrap();
        assert_eq!((v.major, v.minor, v.build, v.chip), (5, 8, 599, 0x00202020));
        v.check().unwrap();
        assert!(FirmwareVersion { major: 6, ..v }.check().is_err());
        assert!(FirmwareVersion::parse(&[0; 4]).is_err());
    }
    pub(crate) fn packet(kind: u16, seq: u16, ts: u32, payload: &[u8]) -> Vec<u8> {
        let mut p = words(&[REPLY_MAGIC, kind, seq]);
        p.extend_from_slice(&((HEADER + payload.len()) as u16).to_be_bytes());
        p.extend_from_slice(&ts.to_le_bytes());
        p.extend_from_slice(payload);
        p
    }
    #[test]
    fn every_usb_split_and_coalesced_packets() {
        let p = packet(DATA, 0xffff, 123, &[1, 2, 3, 4]);
        for split in 0..=p.len() {
            let mut d = PacketDecoder::default();
            d.push(&p[..split]).unwrap();
            if split < p.len() {
                assert!(d.next().unwrap().is_none());
            }
            d.push(&p[split..]).unwrap();
            let decoded = d.next().unwrap().unwrap();
            assert_eq!(
                (decoded.kind, decoded.sequence, decoded.timestamp),
                (DATA, 0xffff, 123)
            );
            assert_eq!(decoded.payload, [1, 2, 3, 4]);
            assert!(d.next().unwrap().is_none());
        }
        let mut d = PacketDecoder::default();
        d.push(&[0xff, 0, 0x52]).unwrap();
        assert!(d.next().unwrap().is_none());
        d.push(&p[1..]).unwrap();
        d.push(&p).unwrap();
        assert!(d.next().unwrap().is_some());
        assert!(d.next().unwrap().is_some());
        assert_eq!(d.discarded_bytes, 2);
    }
    #[test]
    fn malformed_packet_recovers() {
        let mut bad = packet(DATA, 1, 0, &[]);
        bad[6] = 0;
        bad[7] = 4;
        bad.extend(packet(START, 2, 0, &[]));
        let mut d = PacketDecoder::default();
        d.push(&bad).unwrap();
        assert!(d.next().is_err());
        assert_eq!(d.next().unwrap().unwrap().kind, START);
    }
    #[test]
    fn live_get_version_fixture() {
        // Actual EP0 reply observed on 1d27:0601 after the user's ACL grant.
        let bytes: Vec<u8> = include_str!("../tests/fixtures/version-5.8.22.hex")
            .split_whitespace()
            .map(|hex| u8::from_str_radix(hex, 16).unwrap())
            .collect();
        let v = FirmwareVersion::parse(reply(&bytes, 0, 0).unwrap().unwrap()).unwrap();
        assert_eq!(
            (v.major, v.minor, v.build, v.chip, v.fpga, v.system),
            (5, 8, 22, 0x00202020, 4, 15)
        );
        v.check().unwrap();
    }
}

#[cfg(test)]
mod late_tests {
    use super::*;
    #[test]
    fn late_reply_can_be_fragmented_or_precede_current_reply() {
        let late = words(&[REPLY_MAGIC, 1, 3, u16::MAX, 0]);
        let current = words(&[REPLY_MAGIC, 2, 2, 0, 0, 30]);
        let mut combined = late[..8].to_vec();
        assert!(discard_previous(&mut combined, 3, u16::MAX).unwrap());
        combined.extend(&late[8..]);
        combined.extend(&current);
        assert!(!discard_previous(&mut combined, 3, u16::MAX).unwrap());
        assert_eq!(reply(&combined, 2, 0).unwrap().unwrap(), [30, 0]);
        let mut wrong = late;
        assert!(!discard_previous(&mut wrong, 2, u16::MAX).unwrap());
        assert!(reply(&wrong, 2, 0).is_err());
    }
}

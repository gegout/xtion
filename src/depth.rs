use crate::{
    DepthFrame, DepthMode, DepthUnits, Error, Result,
    protocol::{DATA, END, Packet, START},
};
use std::time::Duration;

/// LUT generated only from device calibration, never guessed nominal geometry.
pub(crate) struct Calibration {
    table: Vec<u16>,
}
impl Calibration {
    pub fn from_device(fixed: &[u8], constant_shift: u16) -> Result<Self> {
        if fixed.len() < 108 || constant_shift == 0 {
            return Err(Error::Protocol("missing depth calibration"));
        }
        let float = |offset| {
            f64::from(f32::from_le_bytes(
                fixed[offset..offset + 4].try_into().unwrap(),
            ))
        };
        let baseline = float(92);
        let distance = float(100).trunc();
        let pixel_size = float(104);
        if [baseline, distance, pixel_size]
            .iter()
            .any(|x| !x.is_finite() || *x <= 0.0)
        {
            return Err(Error::Protocol(
                "nonpositive/nonfinite calibration geometry",
            ));
        }
        let mut table = vec![0; 2048];
        for shift in 1..2047u16 {
            let metric =
                ((f64::from(shift) - 4.0 * f64::from(constant_shift)) / 4.0 - 0.375) * pixel_size;
            let denominator = baseline - metric;
            let mm = 10.0 * distance * baseline / denominator;
            if denominator > 0.0 && mm.is_finite() && mm > 0.0 && mm < 10_000.0 {
                table[usize::from(shift)] = mm as u16;
            }
        }
        if table.iter().all(|&p| p == 0) {
            return Err(Error::Protocol("calibration produced no valid distances"));
        }
        Ok(Self { table })
    }
}

#[derive(Clone, Copy)]
pub(crate) enum WireFormat {
    Shift16,
    Packed11,
}
impl WireFormat {
    pub fn firmware_value(self) -> u16 {
        match self {
            Self::Shift16 => 0,
            Self::Packed11 => 3,
        }
    }
    fn bytes(self, pixels: usize) -> usize {
        match self {
            Self::Shift16 => pixels * 2,
            Self::Packed11 => pixels * 11 / 8,
        }
    }
}

pub(crate) struct FrameAssembler {
    mode: DepthMode,
    format: WireFormat,
    active: bool,
    previous: Option<u16>,
    bytes: Vec<u8>,
    prepend: usize,
    append: usize,
    pub incomplete: u64,
    pub sequence_gaps: u64,
}
impl FrameAssembler {
    pub fn new(mode: DepthMode, format: WireFormat) -> Self {
        Self {
            mode,
            format,
            active: false,
            previous: None,
            bytes: Vec::with_capacity(mode.pixel_count() * 2),
            prepend: 0,
            append: 0,
            incomplete: 0,
            sequence_gaps: 0,
        }
    }
    pub fn invalidate(&mut self) {
        if self.active {
            self.incomplete += 1;
        }
        self.active = false;
        self.bytes.clear();
    }
    pub fn push(
        &mut self,
        p: Packet,
        arrival: Duration,
        calibration: Option<&Calibration>,
    ) -> Result<Option<DepthFrame>> {
        if ![START, DATA, END].contains(&p.kind) {
            return Ok(None);
        }
        let discontinuity = self
            .previous
            .is_some_and(|prev| p.sequence != prev.wrapping_add(1));
        self.previous = Some(p.sequence);
        if discontinuity {
            self.sequence_gaps += 1;
        }
        if p.kind == START {
            self.invalidate();
            self.prepend = (p.timestamp >> 16) as usize;
            self.append = (p.timestamp & 0xffff) as usize;
            if self.prepend + self.append > self.mode.pixel_count() {
                return Err(Error::Frame("padding exceeds frame"));
            }
            self.active = true;
        } else if self.active && discontinuity {
            self.invalidate();
            return Err(Error::Frame("packet sequence gap"));
        }
        if !self.active {
            return Ok(None);
        }
        let pixel_count = self.mode.pixel_count() - self.prepend - self.append;
        if matches!(self.format, WireFormat::Packed11) && !pixel_count.is_multiple_of(8) {
            self.invalidate();
            return Err(Error::Frame(
                "packed frame pixel count must be a multiple of eight",
            ));
        }
        let expected = self.format.bytes(pixel_count);
        if self.bytes.len() + p.payload.len() > expected {
            self.invalidate();
            return Err(Error::Frame("payload exceeds configured frame"));
        }
        self.bytes.extend_from_slice(&p.payload);
        if p.kind != END {
            return Ok(None);
        }
        if self.bytes.len() != expected {
            self.invalidate();
            return Err(Error::Frame("incomplete encoded frame"));
        }
        let normalize = |shift: u16| {
            if shift >= 2047 {
                0
            } else if let Some(c) = calibration {
                c.table[usize::from(shift)]
            } else {
                shift
            }
        };
        let mut pixels = Vec::with_capacity(self.mode.pixel_count());
        pixels.resize(self.prepend, 0);
        match self.format {
            WireFormat::Shift16 => pixels.extend(
                self.bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| normalize(u16::from_le_bytes(*b))),
            ),
            WireFormat::Packed11 => {
                // Eight MSB-first 11-bit samples occupy eleven bytes. Accumulate
                // across protocol packet boundaries; unpack once a frame is complete.
                let mut accumulator = 0u32;
                let mut available = 0;
                for &byte in &self.bytes {
                    accumulator = (accumulator << 8) | u32::from(byte);
                    available += 8;
                    if available >= 11 {
                        available -= 11;
                        pixels.push(normalize(((accumulator >> available) & 2047) as u16));
                    }
                    accumulator &= (1 << available) - 1;
                }
            }
        }
        pixels.resize(self.mode.pixel_count(), 0);
        self.active = false;
        self.bytes.clear();
        DepthFrame::new(
            self.mode,
            if calibration.is_some() {
                DepthUnits::Millimetres
            } else {
                DepthUnits::RawShift
            },
            p.timestamp,
            arrival,
            pixels,
        )
        .map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mode() -> DepthMode {
        DepthMode {
            width: 2,
            height: 2,
            fps: 30,
        }
    }
    fn p(kind: u16, sequence: u16, timestamp: u32, payload: &[u8]) -> Packet {
        Packet {
            kind,
            sequence,
            timestamp,
            payload: payload.to_vec(),
        }
    }
    #[test]
    fn padding_invalid_values_and_end_timestamp() {
        let mut a = FrameAssembler::new(mode(), WireFormat::Shift16);
        a.push(p(START, 0xffff, 1 << 16, &[9]), Duration::ZERO, None)
            .unwrap();
        a.push(p(DATA, 0, 0, &[0, 0xff, 7]), Duration::ZERO, None)
            .unwrap();
        let f = a
            .push(p(END, 1, 345, &[12, 0]), Duration::from_secs(1), None)
            .unwrap()
            .unwrap();
        assert_eq!(f.pixels(), [0, 9, 0, 12]);
        assert_eq!(f.timestamp(), 345);
        assert_eq!(f.host_arrival(), Duration::from_secs(1));
    }
    #[test]
    fn lost_overflow_and_incomplete_frames_recover() {
        let mut a = FrameAssembler::new(mode(), WireFormat::Shift16);
        a.push(p(START, 10, 0, &[]), Duration::ZERO, None).unwrap();
        assert!(
            a.push(p(END, 12, 0, &[0; 8]), Duration::ZERO, None)
                .is_err()
        );
        a.push(p(START, 20, 0, &[]), Duration::ZERO, None).unwrap();
        assert!(
            a.push(p(END, 21, 0, &[1; 9]), Duration::ZERO, None)
                .is_err()
        );
        a.push(p(START, 22, 0, &[]), Duration::ZERO, None).unwrap();
        assert!(
            a.push(p(END, 23, 0, &[1; 7]), Duration::ZERO, None)
                .is_err()
        );
        a.push(p(START, 24, 0, &[0; 8]), Duration::ZERO, None)
            .unwrap();
        assert!(
            a.push(p(END, 25, 0, &[]), Duration::ZERO, None)
                .unwrap()
                .is_some()
        );
        assert_eq!(a.incomplete, 3);
    }
    #[test]
    fn calibration_geometry_and_invalids() {
        let mut f = [0; 108];
        f[92..96].copy_from_slice(&7.5f32.to_le_bytes());
        f[100..104].copy_from_slice(&120f32.to_le_bytes());
        f[104..108].copy_from_slice(&0.1f32.to_le_bytes());
        let c = Calibration::from_device(&f, 200).unwrap();
        // Synthetic geometry only: at shift 801 metric=-0.0125, z=1198.003 mm.
        assert_eq!(c.table[801], 1198);
        assert_eq!(c.table[0], 0);
        assert_eq!(c.table[2047], 0);
        assert!(Calibration::from_device(&[0; 108], 200).is_err());
        f[92..96].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(Calibration::from_device(&f, 200).is_err());
    }
    #[test]
    fn full_vga_frame_through_fragmented_usb_pipeline() {
        use crate::protocol::{PacketDecoder, tests::packet};
        let mode = DepthMode::VGA_30;
        let mut wire = Vec::new();
        let payload: Vec<u8> = (0..mode.pixel_count())
            .flat_map(|i| ((i % 2048) as u16).to_le_bytes())
            .collect();
        wire.extend(packet(START, 1, 0, &payload[..1000]));
        let mut sequence = 2;
        for part in payload[1000..].chunks(12000) {
            wire.extend(packet(DATA, sequence, 0, part));
            sequence += 1;
        }
        wire.extend(packet(END, sequence, 0x12345678, &[]));
        for chunk_size in [1, 511, 16384, 32768] {
            let mut decoder = PacketDecoder::default();
            let mut assembler = FrameAssembler::new(mode, WireFormat::Shift16);
            let mut frames = Vec::new();
            for chunk in wire.chunks(chunk_size) {
                decoder.push(chunk).unwrap();
                while let Some(packet) = decoder.next().unwrap() {
                    if let Some(frame) = assembler.push(packet, Duration::ZERO, None).unwrap() {
                        frames.push(frame);
                    }
                }
            }
            assert_eq!(frames.len(), 1);
            let frame = &frames[0];
            assert_eq!(
                (frame.width(), frame.height(), frame.timestamp()),
                (640, 480, 0x12345678)
            );
            assert_eq!(frame.pixels()[2046], 2046);
            assert_eq!(frame.pixels()[2047], 0);
            assert_eq!(frame.pixels()[2048], 0);
            assert_eq!(assembler.incomplete, 0);
        }
    }
    #[test]
    fn packed11_bit_order_invalids_and_split_packets() {
        let encoded = [0, 0, 4, 1, 6, 71, 255, 0, 63, 247, 255];
        let mode = DepthMode {
            width: 8,
            height: 1,
            fps: 30,
        };
        let mut assembler = FrameAssembler::new(mode, WireFormat::Packed11);
        assembler
            .push(p(START, 1, 0, &encoded[..1]), Duration::ZERO, None)
            .unwrap();
        assembler
            .push(p(DATA, 2, 0, &encoded[1..6]), Duration::ZERO, None)
            .unwrap();
        let frame = assembler
            .push(p(END, 3, 99, &encoded[6..]), Duration::ZERO, None)
            .unwrap()
            .unwrap();
        assert_eq!(frame.pixels(), [0, 1, 2, 100, 1023, 1024, 2046, 0]);
        assert_eq!(frame.timestamp(), 99);
        assembler
            .push(p(START, 4, 0, &[]), Duration::ZERO, None)
            .unwrap();
        assert!(
            assembler
                .push(p(END, 5, 100, &encoded[..10]), Duration::ZERO, None)
                .is_err()
        );
    }

    #[test]
    fn full_packed_vga_frame() {
        let mode = DepthMode::VGA_30;
        let encoded = [0, 0, 4, 1, 6, 71, 255, 0, 63, 247, 255];
        let mut assembler = FrameAssembler::new(mode, WireFormat::Packed11);
        assembler
            .push(p(START, 0, 0, &[]), Duration::ZERO, None)
            .unwrap();
        let payload = encoded.repeat(mode.pixel_count() / 8);
        let mut seq = 1;
        for part in payload.chunks(12000) {
            assembler
                .push(p(DATA, seq, 0, part), Duration::ZERO, None)
                .unwrap();
            seq += 1;
        }
        let frame = assembler
            .push(p(END, seq, 345, &[]), Duration::ZERO, None)
            .unwrap()
            .unwrap();
        assert_eq!(frame.pixels().len(), 307200);
        for group in frame.pixels().as_chunks::<8>().0 {
            assert_eq!(group, &[0, 1, 2, 100, 1023, 1024, 2046, 0]);
        }
    }
    #[test]
    fn captured_hardware_frame_decodes_to_public_metric_frame() {
        use crate::protocol::PacketDecoder;
        let calibration = Calibration::from_device(
            include_bytes!("../tests/fixtures/fixed-params-prefix.bin"),
            u16::from_le_bytes(*include_bytes!("../tests/fixtures/constant-shift.bin")),
        )
        .unwrap();
        for (captured, format, timestamp, stats_expected) in [
            (
                include_bytes!("../tests/fixtures/depth-vga-shift16.bin").as_slice(),
                WireFormat::Shift16,
                1755969030,
                (Some(472), Some(4463), 103109),
            ),
            (
                include_bytes!("../tests/fixtures/depth-vga-packed11.bin").as_slice(),
                WireFormat::Packed11,
                2645751169,
                (Some(420), Some(4241), 67637),
            ),
        ] {
            for chunk_size in [31, 32768, 262144, 1048576] {
                let mut decoder = PacketDecoder::default();
                let mut assembler = FrameAssembler::new(DepthMode::VGA_30, format);
                let mut frames = Vec::new();
                for part in captured.chunks(chunk_size) {
                    decoder.push(part).unwrap();
                    while let Some(packet) = decoder.next().unwrap() {
                        if let Some(frame) = assembler
                            .push(packet, Duration::ZERO, Some(&calibration))
                            .unwrap()
                        {
                            frames.push(frame);
                        }
                    }
                }
                assert_eq!(frames.len(), 1);
                let frame = &frames[0];
                assert_eq!(
                    (frame.width(), frame.height(), frame.timestamp()),
                    (640, 480, timestamp)
                );
                assert_eq!(frame.units(), DepthUnits::Millimetres);
                let stats = frame.stats();
                assert_eq!((stats.min, stats.max, stats.invalid_pixels), stats_expected);
                assert_eq!(assembler.incomplete, 0);
                assert_eq!(decoder.discarded_bytes, 0);
            }
        }
    }
}

use crate::{
    DepthFrame, DepthMode, Error, FirmwareVersion, Result,
    depth::{Calibration, FrameAssembler, WireFormat},
    protocol::{self, PacketDecoder},
    usb::{self, CommandClient, ControlIo, Usb},
};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    time::{Duration, Instant},
};

#[derive(Debug, Default, Clone, Copy)]
pub struct AcquisitionStats {
    pub bytes_received: u64,
    pub frames_received: u64,
    pub incomplete_frames: u64,
    pub startup_frames_discarded: u64,
    pub sequence_gaps: u64,
    pub usb_errors: u64,
    pub enable_ack_missing: bool,
    pub decode_errors: u64,
    pub discarded_bytes: u64,
    pub usb_timeouts: u64,
    pub dropped_usb_transfers: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Opened,
    Idle,
    Streaming,
    Starting,
    Stopping,
    RecoveryRequired,
    Recovering,
    Disconnected,
}

fn get_param<T: ControlIo>(c: &mut CommandClient<T>, id: u16) -> Result<u16> {
    let p = c.execute(2, &id.to_le_bytes())?;
    if p.len() != 2 {
        return Err(Error::Protocol("parameter response length"));
    }
    Ok(protocol::word(&p, 0))
}
fn set_param<T: ControlIo>(c: &mut CommandClient<T>, id: u16, value: u16) -> Result<()> {
    if !c.execute(3, &protocol::words(&[id, value]))?.is_empty() {
        return Err(Error::Protocol("SET_PARAM reply payload"));
    }
    Ok(())
}
fn confirm_activation(
    reply: Result<Vec<u8>>,
    mut receive: impl FnMut() -> Result<DepthFrame>,
) -> Result<DepthFrame> {
    match reply {
        Ok(payload) if !payload.is_empty() => {
            return Err(Error::Protocol("unexpected enable payload"));
        }
        Ok(_) | Err(Error::Timeout) => {}
        Err(error) => return Err(error),
    }
    let first = receive()?;
    let next = receive()?;
    if next.timestamp().wrapping_sub(first.timestamp()) as i32 <= 0 || next.mode() != first.mode() {
        return Err(Error::Frame("inconsistent startup frames"));
    }
    Ok(next)
}
#[derive(Default)]
struct SavedParams(Vec<(u16, u16)>, bool);
impl SavedParams {
    fn set<T: ControlIo>(&mut self, c: &mut CommandClient<T>, id: u16, value: u16) -> Result<()> {
        let previous = get_param(c, id)?;
        if previous == value {
            return Ok(());
        }
        // Record before sending: a failed reply can still mean the write reached hardware.
        self.0.push((id, previous));
        set_param(c, id, value)?;
        if get_param(c, id)? != value {
            return Err(Error::Protocol("parameter readback mismatch"));
        }
        Ok(())
    }
    fn restore<T: ControlIo>(&mut self, c: &mut CommandClient<T>) -> Result<()> {
        while let Some(&(id, value)) = self.0.last() {
            // Disable while the endpoint worker is still draining. If an
            // earlier write was ambiguous, read back before another write.
            if (id == 6 && !self.1) || get_param(c, id)? != value {
                self.1 = true;
                match set_param(c, id, value) {
                    Ok(()) => {}
                    Err(error) if matches!(error.root_cause(), Error::Timeout) => {
                        if get_param(c, id).ok() != Some(value) {
                            return Err(error);
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            if get_param(c, id)? != value {
                return Err(Error::Protocol("restoration readback mismatch"));
            }
            self.1 = false;
            self.0.pop();
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Health {
    pub firmware: Result<FirmwareVersion>,
    pub stream_mode: Option<Result<u16>>,
    pub bytes: Option<Result<usize>>,
    pub alternate: u8,
}

/// Owns one claimed interface; single-threaded pull acquisition provides natural backpressure.
pub struct Xtion {
    usb: Usb,
    firmware: FirmwareVersion,
    format: WireFormat,
    state: State,
    saved: SavedParams,
    calibration: Option<Calibration>,
    calibration_error: Option<String>,
    decoder: PacketDecoder,
    assembler: FrameAssembler,
    stats: AcquisitionStats,
    opened: Instant,
    last_ticks: Option<u32>,
    clock_hz: Option<f64>,
    last_recovery: Option<crate::RecoveryReport>,
    arrival: Duration,
    dump: Option<File>,
    first_frame: Option<DepthFrame>,
}
impl Xtion {
    pub fn open() -> Result<Self> {
        let devices = usb::discover()?;
        match devices.as_slice() {
            [] => Err(Error::DeviceNotFound),
            [d] => Self::open_at(d.bus, d.address),
            _ => Err(Error::MultipleDevices),
        }
    }
    pub fn open_at(bus: u8, address: u8) -> Result<Self> {
        let mut usb = Usb::open(bus, address)?;
        let firmware = FirmwareVersion::parse(&usb.client.execute(0, &[])?)?;
        firmware.check()?;
        Ok(Self {
            usb,
            firmware,
            format: WireFormat::Packed11,
            state: State::Opened,
            saved: SavedParams::default(),
            calibration: None,
            calibration_error: None,
            decoder: PacketDecoder::default(),
            assembler: FrameAssembler::new(DepthMode::VGA_30, WireFormat::Packed11),
            stats: AcquisitionStats::default(),
            opened: Instant::now(),
            last_ticks: None,
            clock_hz: None,
            last_recovery: None,
            arrival: Duration::ZERO,
            dump: None,
            first_frame: None,
        })
    }
    /// Independently sample control and bulk health on the selected target.
    /// A failed firmware query does not hide continued depth endpoint activity.
    pub fn diagnose_at(info: &usb::DeviceInfo) -> Result<Health> {
        let mut usb = Usb::open(info.bus, info.address)?;
        if !info.same_port_device(&usb.info) {
            return Err(Error::DeviceNotFound);
        }
        let firmware = usb
            .client
            .execute(0, &[])
            .and_then(|data| FirmwareVersion::parse(&data));
        let stream_mode = match &firmware {
            Ok(f) if f.check().is_ok() => Some(get_param(&mut usb.client, 6)),
            _ => None,
        };
        let bulk = usb.info.interfaces.iter().any(|i| {
            Some(i.configuration) == usb.info.active_configuration
                && i.number == 0
                && i.alternate == usb.current_alt
                && i.endpoints
                    .iter()
                    .any(|e| e.address == 0x81 && e.transfer_type == rusb::TransferType::Bulk)
        });
        let bytes = bulk.then(|| usb.endpoint_sample());
        Ok(Health {
            firmware,
            stream_mode,
            bytes,
            alternate: usb.current_alt,
        })
    }
    pub fn info(&self) -> &usb::DeviceInfo {
        &self.usb.info
    }
    /// Frame time on the host is recorded by the endpoint reader before queueing.
    pub fn clock_hz(&self) -> Option<f64> {
        self.clock_hz
    }
    pub fn last_frame_timestamp(&self) -> Option<u32> {
        self.last_ticks
    }
    pub fn last_recovery(&self) -> Option<&crate::RecoveryReport> {
        self.last_recovery.as_ref()
    }
    pub(crate) fn record_recovery(&mut self, report: crate::RecoveryReport) {
        self.last_recovery = Some(report);
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn firmware(&self) -> FirmwareVersion {
        self.firmware
    }
    pub(crate) fn reconcile_idle(&mut self) -> Result<()> {
        if self.usb.client.execute(5, &[])? != [1, 0] {
            return Err(Error::InvalidState("sensor outside PS mode"));
        }
        if get_param(&mut self.usb.client, 5)? != 0 || get_param(&mut self.usb.client, 7)? != 0 {
            return Err(Error::InvalidState("another firmware stream is active"));
        }
        match get_param(&mut self.usb.client, 6)? {
            0 => {}
            2 => {
                set_param(&mut self.usb.client, 6, 0)?;
                if get_param(&mut self.usb.client, 6)? != 0 {
                    return Err(Error::Protocol("depth still enabled"));
                }
            }
            _ => return Err(Error::InvalidState("unknown stream on depth interface")),
        }
        if self.usb.current_alt != self.firmware.bulk_alt() {
            return Err(Error::Unsupported(
                "recovery requires bulk alternate".into(),
            ));
        }
        self.usb.verify_quiet()?;
        self.state = State::Idle;
        Ok(())
    }
    /// Release the failed claim and recover the same port. Snapshots survive an
    /// unsuccessful reopen, so the same object can retry after reconnection.
    pub fn recover(&mut self, allow_device_reset: bool) -> Result<()> {
        let identity = self.info().clone();
        let stop_error = if self.state == State::Streaming {
            self.stop_depth().err()
        } else {
            None
        };
        self.state = State::Recovering;
        let result = (|| {
            self.usb.release()?;
            let mut next = Self::recover_at(&identity, allow_device_reset)?;
            self.saved.restore(&mut next.usb.client)?;
            next.usb.set_alt(self.usb.original_alt)?;
            next.usb.original_alt = self.usb.original_alt;
            if let (Some(report), Some(e)) = (&mut next.last_recovery, stop_error) {
                report.previous_error = Some(e.to_string());
            }
            *self = next;
            Ok(())
        })();
        if let Err(e) = &result {
            self.state = if matches!(e, Error::DeviceNotFound | Error::Usb(rusb::Error::NoDevice)) {
                State::Disconnected
            } else {
                State::RecoveryRequired
            };
        }
        result
    }
    pub fn host_elapsed(&self) -> Duration {
        self.opened.elapsed()
    }
    pub fn calibration_error(&self) -> Option<&str> {
        self.calibration_error.as_deref()
    }
    pub fn statistics(&self) -> AcquisitionStats {
        let (bytes_received, usb_timeouts, dropped_usb_transfers, usb_errors) =
            self.usb.reader_stats();
        AcquisitionStats {
            bytes_received,
            usb_errors,
            usb_timeouts,
            dropped_usb_transfers,
            incomplete_frames: self.assembler.incomplete,
            sequence_gaps: self.assembler.sequence_gaps,
            discarded_bytes: self.decoder.discarded_bytes,
            ..self.stats
        }
    }
    /// Request VGA/30 packed 11-bit shifts (16-bit diagnostic override available).
    /// Starts only after two complete frames with progressing sensor timestamps.
    pub fn start_depth(&mut self) -> Result<()> {
        let result = self.start_checked();
        if result
            .as_ref()
            .is_err_and(|e| e.category() == "disconnected")
        {
            self.state = State::Disconnected;
            self.usb.stop_reader();
        }
        result
    }
    fn start_checked(&mut self) -> Result<()> {
        if !matches!(self.state, State::Opened | State::Idle) {
            return Err(Error::InvalidState(
                "stop the current session before starting",
            ));
        }
        self.format = match std::env::var("XTION_WIRE_FORMAT").as_deref() {
            Ok("16") => WireFormat::Shift16,
            Ok("11") | Err(std::env::VarError::NotPresent) => WireFormat::Packed11,
            _ => return Err(Error::InvalidState("XTION_WIRE_FORMAT must be 11 or 16")),
        };
        let mode = self.usb.client.execute(5, &[])?;
        if mode != [1, 0] {
            return Err(Error::Unsupported(
                "device is not in PS operating mode; no mode change/reset will be sent".into(),
            ));
        }
        for id in [5, 6, 7] {
            if get_param(&mut self.usb.client, id)? != 0 {
                return Err(Error::InvalidState("device already has an active stream"));
            }
        }
        // Open dumps before changing device state. Never overwrite a prior capture.
        self.dump = match std::env::var_os("XTION_DUMP") {
            Some(path) => Some(OpenOptions::new().write(true).create_new(true).open(path)?),
            None => None,
        };
        self.last_ticks = None;
        self.state = State::Starting;
        let result = self.configure_depth();
        if let Err(cause) = result {
            return match self.stop_depth() {
                Ok(()) => Err(cause),
                Err(cleanup) => Err(Error::Cleanup {
                    cause: Box::new(cause),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        self.state = State::Streaming;
        Ok(())
    }
    fn configure_depth(&mut self) -> Result<()> {
        self.usb.client.execute(1, &[])?;
        self.usb.select_depth_alt(self.firmware.bulk_alt())?;
        // Upstream SetActualRead(TRUE) precedes firmware configuration and enable.
        self.usb.start_reader()?;
        self.calibration = None;
        self.calibration_error = None;
        match self.read_calibration() {
            Ok(c) => self.calibration = Some(c),
            Err(e) => {
                // Transport failures must not be hidden as a mere lack of calibration.
                if matches!(
                    e.root_cause(),
                    Error::Usb(_) | Error::Timeout | Error::Io(_)
                ) {
                    return Err(e);
                }
                self.calibration_error = Some(e.to_string());
            }
        }
        self.stats = AcquisitionStats::default();
        self.decoder = PacketDecoder::default();
        self.assembler = FrameAssembler::new(DepthMode::VGA_30, self.format);
        for (id, value) in [
            (18, self.format.firmware_value()),
            (19, 1),
            (20, 30),
            (2, 0),
            (23, 0),
            (24, 0),
            (55, 0),
        ] {
            self.saved.set(&mut self.usb.client, id, value)?;
        }
        // PS1080 XnSensorStreamHelper reads algorithm 0x80 after configuration.
        self.clock_hz = match self
            .usb
            .client
            .execute(22, &protocol::words(&[0x80, 0, 0, 0, 0]))
        {
            Ok(data) => data
                .try_into()
                .ok()
                .map(f32::from_le_bytes)
                .filter(|mhz| mhz.is_finite() && *mhz > 0.0)
                .map(|mhz| f64::from(mhz) * 1e6),
            Err(Error::Nack(_)) => None,
            Err(e) => return Err(e),
        };
        // Some 5.8.22 devices start transmitting but never answer stream enable.
        // The idle check already established the original value of stream 1.
        self.saved.0.push((6, 0));
        let reply = self.usb.client.execute_with_timeout(
            3,
            &protocol::words(&[6, 2]),
            Duration::from_millis(700),
        );
        self.stats.enable_ack_missing = matches!(reply, Err(Error::Timeout));
        self.first_frame = Some(confirm_activation(reply, || {
            self.receive_frame(Duration::from_secs(2))
        })?);
        self.stats.startup_frames_discarded = 1;
        Ok(())
    }
    fn read_calibration(&mut self) -> Result<Calibration> {
        let mut fixed = Vec::new();
        while fixed.len() < 108 {
            if !fixed.len().is_multiple_of(4) {
                return Err(Error::Protocol("unaligned fixed-parameter chunk"));
            }
            let p = self
                .usb
                .client
                .execute(4, &((fixed.len() / 4) as u16).to_le_bytes())?;
            if p.is_empty() || fixed.len() + p.len() > 1024 {
                return Err(Error::Protocol("invalid fixed-parameter chunk"));
            }
            fixed.extend(p);
        }
        let data = self
            .usb
            .client
            .execute(22, &protocol::words(&[0, 0, 1, 30, 0]))?;
        if data.len() != 2 {
            return Err(Error::Protocol("depth algorithm parameter length"));
        }
        Calibration::from_device(&fixed, protocol::word(&data, 0))
    }
    pub fn read_depth_frame(&mut self) -> Result<DepthFrame> {
        if self.state == State::Disconnected {
            self.recover(false)?;
            self.start_depth()?;
            return self.read_depth_frame_timeout(Duration::from_secs(5));
        }
        match self.read_depth_frame_timeout(Duration::from_secs(5)) {
            Ok(frame) => Ok(frame),
            Err(cause)
                if matches!(
                    cause.root_cause(),
                    Error::Timeout | Error::Usb(rusb::Error::NoDevice | rusb::Error::Pipe)
                ) =>
            {
                self.recover(false)
                    .and_then(|()| self.start_depth())
                    .and_then(|()| self.read_depth_frame_timeout(Duration::from_secs(5)))
                    .map_err(|recovery| Error::Recovery {
                        cause: Box::new(cause),
                        recovery: Box::new(recovery),
                    })
            }
            Err(error) => Err(error),
        }
    }
    /// Timeout covers the whole frame assembly, not just each USB read.
    pub fn read_depth_frame_timeout(&mut self, timeout: Duration) -> Result<DepthFrame> {
        if self.state != State::Streaming {
            return Err(Error::InvalidState("depth stream is not running"));
        }
        if timeout.is_zero() {
            return Err(Error::Timeout);
        }
        if let Some(frame) = self.first_frame.take() {
            return Ok(frame);
        }
        self.receive_frame(timeout)
    }
    fn receive_frame(&mut self, timeout: Duration) -> Result<DepthFrame> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(Error::InvalidState("frame timeout too large"))?;
        loop {
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            loop {
                let discarded_before = self.decoder.discarded_bytes;
                let packet = self.decoder.next();
                if self.decoder.discarded_bytes != discarded_before {
                    self.assembler.invalidate();
                }
                match packet {
                    Ok(Some(p)) => {
                        match self
                            .assembler
                            .push(p, self.arrival, self.calibration.as_ref())
                        {
                            Ok(Some(f)) => {
                                self.last_ticks = Some(f.timestamp());
                                self.stats.frames_received += 1;
                                return Ok(f);
                            }
                            Ok(None) => {}
                            Err(Error::Frame("packet sequence gap")) => {
                                self.stats.sequence_gaps += 1
                            }
                            Err(_) => self.stats.decode_errors += 1,
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        self.stats.decode_errors += 1;
                        self.assembler.invalidate();
                    }
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                continue;
            }
            match self
                .usb
                .read_depth(remaining.min(Duration::from_millis(200)))
            {
                Ok(chunk) => {
                    self.arrival = chunk.received.duration_since(self.opened);
                    if chunk.gap {
                        self.assembler.invalidate();
                        let discarded = self.decoder.discarded_bytes;
                        self.decoder = PacketDecoder::default();
                        self.decoder.discarded_bytes = discarded;
                    }
                    if let Some(dump) = &mut self.dump {
                        dump.write_all(&chunk.data)?;
                    }
                    self.decoder.push(&chunk.data)?;
                }
                Err(Error::Timeout) => {}
                Err(e) => {
                    if matches!(e, Error::Usb(rusb::Error::NoDevice)) {
                        self.state = State::Disconnected;
                    }
                    self.assembler.invalidate();
                    return Err(e);
                }
            }
        }
    }
    /// Restore settings, then the USB alternate. Retryable if restoration fails.
    pub fn stop_depth(&mut self) -> Result<()> {
        if matches!(self.state, State::Idle | State::Opened) {
            return Ok(());
        }
        if self.state == State::Disconnected || !self.usb.is_claimed() {
            return Err(Error::DeviceNotFound);
        }
        self.state = State::Stopping;
        let result = self.finish_stop();
        self.usb.stop_reader();
        if let Err(e) = &result {
            self.state = if e.category() == "disconnected" {
                State::Disconnected
            } else {
                State::RecoveryRequired
            };
        }
        result
    }
    fn finish_stop(&mut self) -> Result<()> {
        // Stream1 is the final saved setting, so restoration disables it first.
        self.saved
            .restore(&mut self.usb.client)
            .map_err(|cause| Error::Operation {
                operation: "restore sensor settings during shutdown",
                cause: Box::new(cause),
            })?;
        self.usb.stop_reader();
        if self.usb.current_alt == self.firmware.bulk_alt() {
            self.usb.verify_quiet()?;
        }
        self.usb.set_alt(self.usb.original_alt)?;
        if let Some(mut dump) = self.dump.take() {
            dump.flush()?;
        }
        self.assembler.invalidate();
        self.first_frame = None;
        self.state = State::Idle;
        Ok(())
    }
    /// Borrowing iterator; ends after the first error. Stop explicitly to inspect cleanup errors.
    ///
    /// An outstanding iterator holds the exclusive borrow until released.
    /// ```compile_fail
    /// fn concurrent(sensor: &mut xtion::Xtion) {
    ///     let mut frames = sensor.depth_frames();
    ///     sensor.stop_depth().unwrap();
    ///     let _ = frames.next();
    /// }
    /// ```
    pub fn depth_frames(&mut self) -> DepthFrames<'_> {
        DepthFrames {
            sensor: self,
            done: false,
        }
    }
}
impl Drop for Xtion {
    fn drop(&mut self) {
        if self.state == State::Streaming || self.state == State::Starting {
            let _ = self.stop_depth();
        }
    }
}

pub struct DepthFrames<'a> {
    sensor: &'a mut Xtion,
    done: bool,
}
impl Iterator for DepthFrames<'_> {
    type Item = Result<DepthFrame>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let frame = self.sensor.read_depth_frame();
        self.done = frame.is_err();
        Some(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    struct Mock {
        values: HashMap<u16, u16>,
        response: Vec<u8>,
        fail: bool,
    }
    impl ControlIo for Mock {
        fn write(&mut self, b: &[u8], _: Duration) -> Result<usize> {
            let opcode = protocol::word(b, 4);
            let id = protocol::word(b, 6);
            let param = protocol::word(b, 8);
            let mut data = vec![];
            let mut status = 0;
            if opcode == 2 {
                data = protocol::words(&[*self.values.get(&param).unwrap_or(&0)]);
            } else if self.fail {
                status = 5;
                self.fail = false;
            } else {
                self.values.insert(param, protocol::word(b, 10));
            }
            self.response =
                protocol::words(&[0x4252, 1 + (data.len() / 2) as u16, opcode, id, status]);
            self.response.extend(data);
            Ok(b.len())
        }
        fn read(&mut self, b: &mut [u8], _: Duration) -> Result<usize> {
            let n = self.response.len();
            b[..n].copy_from_slice(&self.response);
            self.response.clear();
            Ok(n)
        }
    }
    #[test]
    fn failed_start_keeps_snapshot_and_restores_original_settings() {
        let mut c = CommandClient::new(Mock {
            values: HashMap::from([(18, 3), (6, 0)]),
            response: vec![],
            fail: false,
        });
        let mut saved = SavedParams::default();
        saved.set(&mut c, 18, 0).unwrap();
        c.io.fail = true;
        assert!(saved.set(&mut c, 6, 2).is_err());
        assert_eq!(saved.0, [(18, 3), (6, 0)]);
        saved.restore(&mut c).unwrap();
        assert!(saved.0.is_empty());
        assert_eq!(c.io.values[&18], 3);
        assert_eq!(c.io.values[&6], 0);
    }
    #[test]
    fn failed_shutdown_is_retryable() {
        let mut c = CommandClient::new(Mock {
            values: HashMap::from([(6, 2)]),
            response: vec![],
            fail: true,
        });
        let mut saved = SavedParams(vec![(6, 0)], false);
        assert!(saved.restore(&mut c).is_err());
        assert_eq!(saved.0.len(), 1);
        saved.restore(&mut c).unwrap();
        assert_eq!(c.io.values[&6], 0);
    }
}

#[cfg(test)]
mod activation_tests {
    use super::*;
    fn frame(ticks: u32) -> DepthFrame {
        DepthFrame::new(
            DepthMode {
                width: 1,
                height: 1,
                fps: 30,
            },
            crate::DepthUnits::RawShift,
            ticks,
            Duration::ZERO,
            vec![500],
        )
        .unwrap()
    }
    #[test]
    fn missing_ack_requires_two_complete_progressing_frames() {
        let mut sequence = [u32::MAX - 10, 100].into_iter();
        let f = confirm_activation(Err(Error::Timeout), || Ok(frame(sequence.next().unwrap())))
            .unwrap();
        assert_eq!(f.timestamp(), 100);
        assert!(confirm_activation(Err(Error::Timeout), || Ok(frame(10))).is_err());
        assert!(confirm_activation(Err(Error::Timeout), || Err(Error::Timeout)).is_err());
    }
    #[test]
    fn transport_write_failures_and_nacks_do_not_accept_data_evidence() {
        for error in [
            Error::Nack(7),
            Error::Operation {
                operation: "send firmware command on EP0",
                cause: Box::new(Error::Timeout),
            },
            Error::Usb(rusb::Error::NoDevice),
        ] {
            assert!(
                confirm_activation(Err(error), || panic!("explicit failure consumed data"))
                    .is_err()
            );
        }
    }
}

#[cfg(test)]
mod hardware_faults {
    use super::*;
    #[test]
    #[ignore = "requires exclusive access to the physical PS1080"]
    fn live_missing_enable_and_disable_replies_reconcile_with_evidence() {
        let mut s = Xtion::open().unwrap();
        s.usb.client.omit_ack = Some(protocol::words(&[6, 2]));
        s.start_depth().unwrap();
        assert!(s.statistics().enable_ack_missing);
        assert!(s.statistics().frames_received >= 2);
        s.read_depth_frame().unwrap();
        s.usb.client.omit_ack = Some(protocol::words(&[6, 0]));
        s.stop_depth().unwrap();
        assert!(s.usb.client.omit_ack.is_none());
        assert_eq!(s.state(), State::Idle);
        s.start_depth().unwrap();
        s.read_depth_frame().unwrap();
        s.stop_depth().unwrap();
    }
    #[test]
    #[ignore = "requires exclusive access to the physical PS1080"]
    fn live_reopens_same_target_after_injected_reader_disconnect() {
        let mut s = Xtion::open().unwrap();
        s.start_depth().unwrap();
        let original = s.info().clone();
        s.usb.next_read_error = Some(Error::Usb(rusb::Error::NoDevice));
        for _ in 0..12 {
            let frame = s.read_depth_frame().unwrap();
            assert_eq!(frame.mode(), DepthMode::VGA_30);
            if s.last_recovery().is_some() {
                break;
            }
        }
        let report = s.last_recovery().unwrap();
        assert!(!report.device_reset_attempted);
        assert!(original.same_port_device(s.info()));
        s.stop_depth().unwrap();
    }
}

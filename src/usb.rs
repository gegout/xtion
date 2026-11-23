use crate::{Error, Result, protocol};
use rusb::{Context, Device, DeviceHandle, TransferType, UsbContext};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) const TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) fn candidate(vid: u16, pid: u16) -> bool {
    vid == 0x1d27 && [0x0500, 0x0600, 0x0601, 0x0609].contains(&pid)
}

#[derive(Debug, Clone)]
pub struct EndpointInfo {
    pub address: u8,
    pub transfer_type: TransferType,
    pub max_packet_size: u16,
    pub interval: u8,
}
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub configuration: u8,
    pub number: u8,
    pub alternate: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub kernel_driver_active: Option<bool>,
    pub endpoints: Vec<EndpointInfo>,
}
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus: u8,
    pub address: u8,
    pub ports: Vec<u8>,
    pub usb_version: rusb::Version,
    pub device_version: rusb::Version,
    pub speed: rusb::Speed,
    pub active_configuration: Option<u8>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial_number: Option<String>,
    /// Some(message) means opening or reading strings failed; descriptors remain usable.
    pub access_error: Option<String>,
    pub interfaces: Vec<InterfaceInfo>,
}
fn describe(device: &Device<Context>) -> Result<DeviceInfo> {
    let d = device.device_descriptor()?;
    let mut info = DeviceInfo {
        vendor_id: d.vendor_id(),
        product_id: d.product_id(),
        bus: device.bus_number(),
        address: device.address(),
        ports: device.port_numbers()?,
        usb_version: d.usb_version(),
        device_version: d.device_version(),
        speed: device.speed(),
        active_configuration: None,
        manufacturer: None,
        product: None,
        serial_number: None,
        access_error: None,
        interfaces: Vec::new(),
    };
    for index in 0..d.num_configurations() {
        let config = device.config_descriptor(index)?;
        for interface in config.interfaces() {
            for alt in interface.descriptors() {
                info.interfaces.push(InterfaceInfo {
                    configuration: config.number(),
                    number: alt.interface_number(),
                    alternate: alt.setting_number(),
                    class: alt.class_code(),
                    subclass: alt.sub_class_code(),
                    protocol: alt.protocol_code(),
                    kernel_driver_active: None,
                    endpoints: alt
                        .endpoint_descriptors()
                        .map(|e| EndpointInfo {
                            address: e.address(),
                            transfer_type: e.transfer_type(),
                            max_packet_size: e.max_packet_size(),
                            interval: e.interval(),
                        })
                        .collect(),
                });
            }
        }
    }
    match device.open() {
        Ok(h) => {
            info.active_configuration = h.active_configuration().ok();
            for interface in &mut info.interfaces {
                if Some(interface.configuration) == info.active_configuration {
                    interface.kernel_driver_active = h.kernel_driver_active(interface.number).ok();
                }
            }
            // String descriptors are standard read-only USB operations on candidate IDs only.
            let strings = (|| -> rusb::Result<()> {
                if d.manufacturer_string_index().is_some() {
                    info.manufacturer = Some(h.read_manufacturer_string_ascii(&d)?);
                }
                if d.product_string_index().is_some() {
                    info.product = Some(h.read_product_string_ascii(&d)?);
                }
                if d.serial_number_string_index().is_some() {
                    info.serial_number = Some(h.read_serial_number_string_ascii(&d)?);
                }
                Ok(())
            })();
            if let Err(e) = strings {
                info.access_error = Some(e.to_string());
            }
        }
        Err(e) => info.access_error = Some(e.to_string()),
    }
    Ok(info)
}
/// Enumerate upstream candidate IDs; does not send sensor commands or claim interfaces.
pub fn discover() -> Result<Vec<DeviceInfo>> {
    let context = Context::new()?;
    let mut found = Vec::new();
    for device in context.devices()?.iter() {
        let d = match device.device_descriptor() {
            Ok(d) => d,
            Err(rusb::Error::NoDevice) => continue,
            Err(e) => return Err(e.into()),
        };
        if candidate(d.vendor_id(), d.product_id()) {
            match describe(&device) {
                Ok(info) => found.push(info),
                Err(Error::Usb(rusb::Error::NoDevice)) => continue,
                Err(e) => return Err(e),
            }
        }
    }
    Ok(found)
}

/// Private seam used to exercise the actual command client without physical hardware.
pub(crate) trait ControlIo {
    fn write(&mut self, bytes: &[u8], timeout: Duration) -> Result<usize>;
    fn read(&mut self, bytes: &mut [u8], timeout: Duration) -> Result<usize>;
}
impl ControlIo for Arc<DeviceHandle<Context>> {
    fn write(&mut self, bytes: &[u8], timeout: Duration) -> Result<usize> {
        Ok(self.write_control(0x40, 0, 0, 0, bytes, timeout)?)
    }
    fn read(&mut self, bytes: &mut [u8], timeout: Duration) -> Result<usize> {
        Ok(self.read_control(0xc0, 0, 0, 0, bytes, timeout)?)
    }
}

pub(crate) struct CommandClient<T> {
    pub io: T,
    id: u16,
    previous: Option<(u16, u16)>,
    #[cfg(test)]
    pub omit_ack: Option<Vec<u8>>,
    trace: bool,
}
impl<T: ControlIo> CommandClient<T> {
    pub fn new(io: T) -> Self {
        Self {
            io,
            id: 0,
            previous: None,
            #[cfg(test)]
            omit_ack: None,
            trace: std::env::var_os("XTION_TRACE").is_some(),
        }
    }
    pub fn execute(&mut self, opcode: u16, payload: &[u8]) -> Result<Vec<u8>> {
        self.execute_with_timeout(opcode, payload, TIMEOUT)
    }
    pub fn execute_with_timeout(
        &mut self,
        opcode: u16,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        let id = self.id;
        self.id = self.id.wrapping_add(1);
        let request = protocol::request(opcode, id, payload)?;
        let previous = self.previous.replace((opcode, id));
        let deadline = Instant::now() + timeout;
        for attempt in 0..3 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }
            thread::sleep(Duration::from_millis(1));
            if self.trace {
                eprintln!("EP0 OUT {request:02x?}");
            }
            if self
                .io
                .write(&request, remaining.max(Duration::from_millis(1)))
                .map_err(|cause| Error::Operation {
                    operation: "send firmware command on EP0",
                    cause: Box::new(cause),
                })?
                != request.len()
            {
                return Err(Error::Protocol("short control write"));
            }
            // Allow stream mode switching to settle before polling EP0.
            let delay = if opcode == 3 && payload.starts_with(&6u16.to_le_bytes()) {
                300
            } else {
                10
            };
            thread::sleep(
                Duration::from_millis(delay)
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
            let mut all = Vec::new();
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(Error::Timeout);
                }
                let mut chunk = [0; 512];
                match self.io.read(
                    &mut chunk,
                    remaining.clamp(Duration::from_millis(1), Duration::from_millis(200)),
                ) {
                    Ok(n) => {
                        if self.trace {
                            eprintln!("EP0 IN {:02x?}", &chunk[..n]);
                        }
                        all.extend_from_slice(&chunk[..n]);
                        if all.len() > 2056 {
                            return Err(Error::Protocol("control response exceeds limit"));
                        }
                        if let Some((op, id)) = previous
                            && protocol::discard_previous(&mut all, op, id)?
                        {
                            continue;
                        }
                        match protocol::reply(&all, opcode, id) {
                            Ok(Some(data)) => {
                                #[cfg(test)]
                                if opcode == 3 && self.omit_ack.as_deref() == Some(payload) {
                                    self.omit_ack = None;
                                    return Err(Error::Timeout);
                                }
                                return Ok(data.to_vec());
                            }
                            Ok(None) => {}
                            Err(Error::Nack(13)) if attempt < 2 => {
                                thread::sleep(Duration::from_millis(100));
                                break;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    Err(Error::Timeout | Error::Usb(rusb::Error::Interrupted)) => {}
                    Err(e) => return Err(e),
                }
                thread::sleep(Duration::from_millis(1));
            }
        }
        Err(Error::Nack(13))
    }
}

pub(crate) struct StreamChunk {
    pub data: Vec<u8>,
    pub gap: bool,
    pub received: Instant,
}
#[derive(Default)]
struct ReaderCounters {
    bytes: AtomicU64,
    timeouts: AtomicU64,
    dropped: AtomicU64,
    errors: AtomicU64,
}
struct BulkReader {
    chunks: Receiver<StreamChunk>,
    stop: Arc<AtomicBool>,
    counters: Arc<ReaderCounters>,
    failure: Arc<Mutex<Option<Error>>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl BulkReader {
    fn spawn(
        size: usize,
        mut read: impl FnMut(&mut [u8]) -> Result<usize> + Send + 'static,
    ) -> Result<Self> {
        // Bounded USB staging only: do not block the device if a consumer stalls.
        let (tx, chunks) = mpsc::sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let counters = Arc::new(ReaderCounters::default());
        let failure = Arc::new(Mutex::new(None));
        let (worker_stop, worker_counters, worker_failure) =
            (stop.clone(), counters.clone(), failure.clone());
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("xtion-usb".into())
            .spawn(move || {
                let mut buffer = vec![0; size];
                let mut gap = false;
                if ready_tx.send(()).is_err() {
                    return;
                }
                while !worker_stop.load(Ordering::Acquire) {
                    match read(&mut buffer) {
                        Ok(0) => thread::sleep(Duration::from_millis(1)),
                        Ok(n) => {
                            worker_counters.bytes.fetch_add(n as u64, Ordering::Relaxed);
                            buffer.truncate(n);
                            match tx.try_send(StreamChunk {
                                data: buffer,
                                gap,
                                received: Instant::now(),
                            }) {
                                Ok(()) => {
                                    buffer = vec![0; size];
                                    gap = false;
                                }
                                Err(TrySendError::Full(chunk)) => {
                                    worker_counters.dropped.fetch_add(1, Ordering::Relaxed);
                                    buffer = chunk.data;
                                    buffer.resize(size, 0);
                                    gap = true;
                                }
                                Err(TrySendError::Disconnected(_)) => break,
                            }
                        }
                        Err(Error::Timeout) => {
                            worker_counters.timeouts.fetch_add(1, Ordering::Relaxed);
                            // rusb 0.9.4 returns partial timeout transfers as Ok(n).
                        }
                        Err(Error::Usb(rusb::Error::Interrupted)) => continue,
                        Err(error) => {
                            worker_counters.errors.fetch_add(1, Ordering::Relaxed);
                            if let Ok(mut failure) = worker_failure.lock() {
                                *failure = Some(error);
                            }
                            break;
                        }
                    }
                }
            })?;
        if ready_rx.recv_timeout(Duration::from_secs(1)).is_err() {
            stop.store(true, Ordering::Release);
            let _ = worker.join();
            return Err(Error::InvalidState("USB reader did not become ready"));
        }
        Ok(Self {
            chunks,
            stop,
            counters,
            failure,
            thread: Some(worker),
        })
    }
    fn read(&self, timeout: Duration) -> Result<StreamChunk> {
        match self.chunks.recv_timeout(timeout) {
            Ok(chunk) => Ok(chunk),
            Err(RecvTimeoutError::Timeout) => Err(Error::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(self
                .failure
                .lock()
                .ok()
                .and_then(|mut e| e.take())
                .unwrap_or(Error::Protocol("USB reader stopped"))),
        }
    }
    fn stats(&self) -> (u64, u64, u64, u64) {
        (
            self.counters.bytes.load(Ordering::Relaxed),
            self.counters.timeouts.load(Ordering::Relaxed),
            self.counters.dropped.load(Ordering::Relaxed),
            self.counters.errors.load(Ordering::Relaxed),
        )
    }
}
impl Drop for BulkReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) struct Usb {
    pub client: CommandClient<Arc<DeviceHandle<Context>>>,
    reader: Option<BulkReader>,
    last_reader_stats: (u64, u64, u64, u64),
    pub info: DeviceInfo,
    pub original_alt: u8,
    claimed: bool,
    #[cfg(test)]
    pub next_read_error: Option<Error>,
    pub current_alt: u8,
}
impl Usb {
    pub fn open(bus: u8, address: u8) -> Result<Self> {
        let context = Context::new()?;
        let device = context
            .devices()?
            .iter()
            .find(|d| d.bus_number() == bus && d.address() == address)
            .ok_or(Error::DeviceNotFound)?;
        let desc = device.device_descriptor()?;
        if !candidate(desc.vendor_id(), desc.product_id()) {
            return Err(Error::DeviceNotFound);
        }
        let info = describe(&device)?;
        let handle = device.open()?;
        let config = handle.active_configuration()?;
        if info
            .interfaces
            .iter()
            .filter(|i| i.configuration == config)
            .any(|i| {
                i.endpoints
                    .iter()
                    .any(|e| e.address == 4 || e.address == 0x85)
            })
        {
            return Err(Error::Unsupported("legacy bulk command transport".into()));
        }
        if !info
            .interfaces
            .iter()
            .any(|i| i.configuration == config && i.number == 0)
        {
            return Err(Error::Unsupported("no active interface 0".into()));
        }
        if handle.kernel_driver_active(0)? {
            return Err(Error::Unsupported(
                "interface 0 has a kernel driver; automatic detach is disabled".into(),
            ));
        }
        handle.claim_interface(0)?;
        let mut alt = [0];
        let ports = info
            .ports
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(".");
        let node = format!("/sys/bus/usb/devices/{bus}-{ports}");
        let kernel_alt = std::fs::read_to_string(format!("{node}/devnum"))
            .ok()
            .and_then(|v| v.trim().parse::<u8>().ok())
            .filter(|v| *v == address)
            .and_then(|_| {
                std::fs::read_to_string(format!("{node}:{config}.0/bAlternateSetting")).ok()
            })
            .and_then(|v| v.trim().parse::<u8>().ok());
        if let Some(value) = kernel_alt {
            alt[0] = value;
        } else {
            let n = handle
                .read_control(0x81, 0x0a, 0, 0, &mut alt, TIMEOUT)
                .map_err(|error| Error::Operation {
                    operation: "GET_INTERFACE on sensor interface 0 (before firmware query)",
                    cause: Box::new(error.into()),
                })?;
            if n != 1 {
                return Err(Error::Protocol("GET_INTERFACE length"));
            }
        }
        Ok(Self {
            client: CommandClient::new(Arc::new(handle)),
            reader: None,
            last_reader_stats: (0, 0, 0, 0),
            info,
            original_alt: alt[0],
            claimed: true,
            #[cfg(test)]
            next_read_error: None,
            current_alt: alt[0],
        })
    }
    pub fn select_depth_alt(&mut self, alt: u8) -> Result<()> {
        let config = self.client.io.active_configuration()?;
        if !self.info.interfaces.iter().any(|i| {
            i.configuration == config
                && i.number == 0
                && i.alternate == alt
                && i.endpoints
                    .iter()
                    .any(|e| e.address == 0x81 && e.transfer_type == TransferType::Bulk)
        }) {
            return Err(Error::Unsupported(format!(
                "interface 0 alternate {alt} has no bulk depth endpoint 0x81"
            )));
        }
        self.set_alt(alt)
    }
    pub fn set_alt(&mut self, alt: u8) -> Result<()> {
        if alt != self.current_alt {
            self.client.io.set_alternate_setting(0, alt)?;
            self.current_alt = alt;
        }
        Ok(())
    }
    pub fn endpoint_sample(&self) -> Result<usize> {
        let mut buf = [0; 16384];
        match self
            .client
            .io
            .read_bulk(0x81, &mut buf, Duration::from_millis(150))
        {
            Err(rusb::Error::Timeout) => Ok(0),
            result => Ok(result?),
        }
    }
    pub fn verify_quiet(&self) -> Result<()> {
        // Consume residual queued bytes, then require two consecutive quiet reads.
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut quiet = 0;
        let mut buf = [0u8; 16384];
        while Instant::now() < deadline {
            match self
                .client
                .io
                .read_bulk(0x81, &mut buf, Duration::from_millis(100))
            {
                Err(rusb::Error::Timeout) => {
                    quiet += 1;
                    if quiet == 2 {
                        return Ok(());
                    }
                }
                Ok(_) => quiet = 0,
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::InvalidState("depth traffic continued after stop"))
    }
    pub fn start_reader(&mut self) -> Result<()> {
        if self.reader.is_some() {
            return Err(Error::InvalidState("USB reader already running"));
        }
        let handle = self.client.io.clone();
        let size = match std::env::var("XTION_READ_KIB").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("1024") => 1024,
            Ok("256") => 256,
            Ok("32") => 32,
            _ => {
                return Err(Error::InvalidState(
                    "XTION_READ_KIB must be 32, 256 or 1024",
                ));
            }
        };
        self.reader = Some(BulkReader::spawn(size * 1024, move |buffer| {
            Ok(handle.read_bulk(0x81, buffer, Duration::from_millis(200))?)
        })?);
        Ok(())
    }
    pub fn stop_reader(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.stop.store(true, Ordering::Release);
            // Drop joins the thread; retain its final counters after join.
            let counters = reader.counters.clone();
            drop(reader);
            self.last_reader_stats = (
                counters.bytes.load(Ordering::Relaxed),
                counters.timeouts.load(Ordering::Relaxed),
                counters.dropped.load(Ordering::Relaxed),
                counters.errors.load(Ordering::Relaxed),
            );
        }
    }
    pub fn reader_stats(&self) -> (u64, u64, u64, u64) {
        self.reader
            .as_ref()
            .map_or(self.last_reader_stats, BulkReader::stats)
    }
    pub fn read_depth(&mut self, timeout: Duration) -> Result<StreamChunk> {
        #[cfg(test)]
        if let Some(e) = self.next_read_error.take() {
            return Err(e);
        }
        self.reader
            .as_ref()
            .ok_or(Error::InvalidState("USB reader is not running"))?
            .read(timeout)
    }
}
impl Usb {
    pub fn is_claimed(&self) -> bool {
        self.claimed
    }
    pub fn release(&mut self) -> Result<()> {
        self.stop_reader();
        if !self.claimed {
            return Ok(());
        }
        match self.client.io.release_interface(0) {
            Ok(()) | Err(rusb::Error::NoDevice | rusb::Error::NotFound) => {
                self.claimed = false;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }
}
impl Drop for Usb {
    fn drop(&mut self) {
        self.stop_reader();
        if self.claimed {
            let _ = self.set_alt(self.original_alt);
            let _ = self.release();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Mock {
        written: Vec<u8>,
        response: Vec<u8>,
        short: bool,
    }
    impl ControlIo for Mock {
        fn write(&mut self, b: &[u8], _: Duration) -> Result<usize> {
            self.written = b.to_vec();
            Ok(if self.short { 0 } else { b.len() })
        }
        fn read(&mut self, b: &mut [u8], _: Duration) -> Result<usize> {
            if self.response.is_empty() {
                return Err(Error::Usb(rusb::Error::NoDevice));
            }
            let n = self.response.len().min(3);
            b[..n].copy_from_slice(&self.response[..n]);
            self.response.drain(..n);
            Ok(n)
        }
    }
    #[test]
    fn actual_client_handles_fragmentation_and_short_writes() {
        let mut c = CommandClient::new(Mock {
            written: vec![],
            response: vec![0x52, 0x42, 2, 0, 2, 0, 0, 0, 0, 0, 30, 0],
            short: false,
        });
        assert_eq!(c.execute(2, &[20, 0]).unwrap(), [30, 0]);
        assert_eq!(c.io.written, protocol::request(2, 0, &[20, 0]).unwrap());
        c.io.short = true;
        assert!(matches!(c.execute(2, &[]), Err(Error::Protocol(_))));
    }
    #[test]
    fn identity_filter_does_not_select_webcams() {
        assert!(!candidate(0x045e, 0x0779));
        assert!(!candidate(0x04f2, 0xb729));
        assert!(!candidate(0x045e, 0x02ae));
        assert!(candidate(0x1d27, 0x0601));
    }
    #[test]
    fn bounded_reader_marks_dropped_data_and_propagates_disconnect() {
        let (source, input) = mpsc::channel::<Result<Vec<u8>>>();
        let reader = BulkReader::spawn(4096, move |buffer| {
            match input.recv_timeout(Duration::from_millis(10)) {
                Ok(Ok(bytes)) => {
                    buffer[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                Ok(Err(error)) => Err(error),
                Err(_) => Err(Error::Timeout),
            }
        })
        .unwrap();
        for _ in 0..9 {
            source.send(Ok(vec![1; 4])).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while reader.stats().2 == 0 {
            assert!(
                Instant::now() < deadline,
                "worker did not fill staging queue"
            );
            thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..8 {
            assert_eq!(reader.read(Duration::from_secs(1)).unwrap().data, [1; 4]);
        }
        source.send(Ok(vec![9; 4])).unwrap();
        let recovered = reader.read(Duration::from_secs(1)).unwrap();
        assert!(recovered.gap);
        assert_eq!(recovered.data, [9; 4]);
        source.send(Err(Error::Usb(rusb::Error::NoDevice))).unwrap();
        assert!(matches!(
            reader.read(Duration::from_secs(1)),
            Err(Error::Usb(rusb::Error::NoDevice))
        ));
        assert_eq!(reader.stats().0, 40);
        assert_eq!(reader.stats().2, 1);
        drop(reader); // joins without a blocked channel sender
    }
}

#[cfg(test)]
mod transaction_faults {
    use super::*;
    use std::collections::VecDeque;
    struct Script {
        reads: VecDeque<Result<Vec<u8>>>,
        writes: u8,
    }
    impl ControlIo for Script {
        fn write(&mut self, b: &[u8], _: Duration) -> Result<usize> {
            self.writes += 1;
            Ok(b.len())
        }
        fn read(&mut self, b: &mut [u8], _: Duration) -> Result<usize> {
            let data = self.reads.pop_front().unwrap_or(Err(Error::Timeout))?;
            b[..data.len()].copy_from_slice(&data);
            Ok(data.len())
        }
    }
    #[test]
    fn empty_interrupted_and_stale_responses_do_not_reissue_write() {
        let old = protocol::words(&[protocol::REPLY_MAGIC, 1, 1, 0, 0]);
        let next = protocol::words(&[protocol::REPLY_MAGIC, 2, 2, 1, 0, 30]);
        let reads = [
            Ok(old.clone()),
            Ok(vec![]),
            Err(Error::Usb(rusb::Error::Interrupted)),
            Ok(old[..8].to_vec()),
            Ok(old[8..].to_vec()),
            Ok(next),
        ];
        let mut c = CommandClient::new(Script {
            reads: reads.into(),
            writes: 0,
        });
        c.execute(1, &[]).unwrap();
        assert_eq!(c.execute(2, &[20, 0]).unwrap(), [30, 0]);
        assert_eq!(c.io.writes, 2);
    }
    #[test]
    fn deadline_does_not_resend_an_ambiguous_write() {
        let mut c = CommandClient::new(Script {
            reads: VecDeque::new(),
            writes: 0,
        });
        assert!(matches!(
            c.execute_with_timeout(3, &[18, 0, 3, 0], Duration::from_millis(20)),
            Err(Error::Timeout)
        ));
        assert_eq!(c.io.writes, 1);
    }
}

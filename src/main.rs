mod stress;
use std::{
    env,
    fs::OpenOptions,
    io::Write,
    process::ExitCode,
    time::{Duration, Instant},
};
use xtion::{DepthFrame, Error, Result, Xtion, discover};

fn report(frame: &DepthFrame) {
    let stats = frame.stats();
    println!(
        "{}x{} {:?} ticks={} min={:?} max={:?} mean={:?} invalid={}",
        frame.width(),
        frame.height(),
        frame.units(),
        frame.timestamp(),
        stats.min,
        stats.max,
        stats.mean,
        stats.invalid_pixels
    );
}
fn pgm(frame: &DepthFrame, path: &str) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    write!(
        file,
        "P5\n# Xtion {:?}; zero=invalid; device_ticks={}\n{} {}\n65535\n",
        frame.units(),
        frame.timestamp(),
        frame.width(),
        frame.height()
    )?;
    for p in frame.pixels() {
        file.write_all(&p.to_be_bytes())?;
    }
    file.flush()?;
    Ok(())
}
fn acquire(command: &str, arg: Option<&str>) -> Result<()> {
    let duration = if command == "stream" {
        let seconds = arg.unwrap_or("10").parse::<u64>().map_err(|_| {
            Error::InvalidState("stream duration must be an integer number of seconds")
        })?;
        if !(1..=3600).contains(&seconds) {
            return Err(Error::InvalidState(
                "stream duration must be 1..=3600 seconds",
            ));
        }
        Duration::from_secs(seconds)
    } else {
        Duration::from_secs(5)
    };
    let mut sensor = Xtion::open()?;
    println!("Firmware: {:?}", sensor.firmware());
    if let Err(error) = sensor.start_depth() {
        eprintln!("Startup USB counters: {:?}", sensor.statistics());
        return Err(error);
    }
    if let Some(error) = sensor.calibration_error() {
        eprintln!("Calibration unavailable: {error}; reporting raw shifts, not millimetres");
    }
    // Always attempt explicit cleanup, including after frame or file errors.
    let initial_bytes = sensor.statistics().bytes_received;
    let started = Instant::now();
    let mut frames = 0;
    let cutoff = sensor.host_elapsed();
    let mut fresh = 0u64;
    let mut first_arrival = None;
    let mut last_arrival = None;
    let result = (|| -> Result<()> {
        let mut previous: Option<DepthFrame> = None;
        loop {
            let remaining = duration.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                break;
            }
            let frame = match sensor.read_depth_frame_timeout(remaining.min(Duration::from_secs(5)))
            {
                Ok(frame) => frame,
                Err(Error::Timeout)
                    if command == "stream" && frames > 0 && started.elapsed() >= duration =>
                {
                    break;
                }
                Err(error) => return Err(error),
            };
            if frame.host_arrival() >= cutoff {
                first_arrival.get_or_insert(frame.host_arrival());
                last_arrival = Some(frame.host_arrival());
                fresh += 1;
            }
            report(&frame);
            if let Some(previous) = &previous {
                let dt_ticks = frame.timestamp().wrapping_sub(previous.timestamp());
                let device_fps = sensor
                    .clock_hz()
                    .filter(|_| dt_ticks != 0)
                    .map(|hz| hz / f64::from(dt_ticks));
                println!("device_clock_fps={device_fps:?}");
                let usb_interval = frame.host_arrival().saturating_sub(previous.host_arrival());
                println!(
                    "host_USB_interval_ms={:.3} ticks_since_previous={} queue_to_decode_ms={:.3}",
                    usb_interval.as_secs_f64() * 1000.,
                    frame.timestamp().wrapping_sub(previous.timestamp()),
                    sensor
                        .host_elapsed()
                        .saturating_sub(frame.host_arrival())
                        .as_secs_f64()
                        * 1000.
                );
                let threshold = if frame.units() == xtion::DepthUnits::Millimetres {
                    100
                } else {
                    5
                };
                println!(
                    "changed_pixels={} threshold={} {:?}",
                    previous.changed_pixels(&frame, threshold)?,
                    threshold,
                    frame.units()
                );
            }
            frames += 1;
            if command == "capture" {
                if let Some(path) = arg {
                    pgm(&frame, path)?;
                }
                break;
            }
            previous = Some(frame);
            if command == "diagnose" && frames >= 10 {
                break;
            }
        }
        Ok(())
    })();
    let elapsed = started.elapsed().as_secs_f64().max(f64::EPSILON);
    let arrival_fps = last_arrival.zip(first_arrival).and_then(|(last, first)| {
        let seconds = (last - first).as_secs_f64();
        (seconds > 0.).then(|| fresh.saturating_sub(1) as f64 / seconds)
    });
    println!(
        "fresh_complete_fps={arrival_fps:?} startup_buffered={}",
        frames - fresh
    );
    println!(
        "frames={} elapsed={:.3}s consumer_delivery_fps={:.2} successful_USB_bytes/s={:.0} {:?}",
        frames,
        elapsed,
        frames as f64 / elapsed,
        (sensor.statistics().bytes_received - initial_bytes) as f64 / elapsed,
        sensor.statistics()
    );
    let cleanup = sensor.stop_depth();
    match (result, cleanup) {
        (Err(cause), Err(cleanup)) => Err(Error::Cleanup {
            cause: Box::new(cause),
            cleanup: Box::new(cleanup),
        }),
        (Err(e), _) | (_, Err(e)) => Err(e),
        _ => Ok(()),
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        println!(
            "Usage: xtion probe | info | capture [frame.pgm] | stream [seconds=10] | diagnose | stress [seconds] [cycles] | recover [--device-reset]"
        );
        return Ok(());
    };
    if command == "diagnose" && args.get(1).map(String::as_str) == Some("--json") && args.len() == 2
    {
        let devices = discover()?;
        if devices.is_empty() {
            println!("{{\"state\":\"disconnected\"}}");
            return Err(Error::DeviceNotFound);
        }
        for d in devices {
            let h = match Xtion::diagnose_at(&d) {
                Ok(h) => h,
                Err(e) => {
                    println!(
                        "{{\"bus\":{},\"address\":{},\"ports\":{:?},\"state\":\"unavailable\",\"error_category\":\"{}\"}}",
                        d.bus,
                        d.address,
                        d.ports,
                        e.category()
                    );
                    return Err(e);
                }
            };
            let fw = h
                .firmware
                .as_ref()
                .ok()
                .map(|v| format!("\"{}.{}.{}\"", v.major, v.minor, v.build))
                .unwrap_or_else(|| "null".into());
            let bytes = h.bytes.as_ref().and_then(|r| r.as_ref().ok()).copied();
            let mode = h
                .stream_mode
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .copied();
            let state = match (mode, bytes) {
                (Some(0), Some(0)) => "idle",
                (Some(2), Some(1..)) => "active_depth",
                (_, Some(1..)) => "endpoint_active",
                _ => "unverified",
            };
            let error = h
                .firmware
                .as_ref()
                .err()
                .or_else(|| h.stream_mode.as_ref()?.as_ref().err())
                .or_else(|| h.bytes.as_ref()?.as_ref().err());
            let category = error
                .map(|e| format!("\"{}\"", e.category()))
                .unwrap_or_else(|| "null".into());
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let interfaces = d
                .interfaces
                .iter()
                .filter(|i| Some(i.configuration) == d.active_configuration)
                .map(|i| i.number)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            println!(
                "{{\"unix_ms\":{now},\"vid\":{},\"pid\":{},\"bus\":{},\"address\":{},\"ports\":{:?},\"interfaces\":{:?},\"depth_alternate\":{},\"firmware\":{fw},\"state\":\"{state}\",\"error_category\":{category},\"endpoint_bytes\":{},\"last_frame\":null,\"last_recovery\":null}}",
                d.vendor_id,
                d.product_id,
                d.bus,
                d.address,
                d.ports,
                interfaces,
                h.alternate,
                bytes
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "null".into())
            );
            h.firmware?;
            if let Some(mode) = h.stream_mode {
                mode?;
            }
            if let Some(bytes) = h.bytes {
                bytes?;
            }
        }
        return Ok(());
    }
    if command == "stress" {
        return stress::run(&args[1..]);
    }
    if command == "recover" {
        let reset = match &args[1..] {
            [] => false,
            [flag] if flag == "--device-reset" => true,
            _ => return Err(Error::InvalidState("usage: recover [--device-reset]")),
        };
        let devices = discover()?;
        let identity = match &devices[..] {
            [one] => one,
            [] => return Err(Error::DeviceNotFound),
            _ => return Err(Error::MultipleDevices),
        };
        let sensor = Xtion::recover_at(identity, reset)?;
        println!(
            "Verified idle on bus {} port {:?}, address {}",
            sensor.info().bus,
            sensor.info().ports,
            sensor.info().address
        );
        println!(
            "Recovery (open attempts, device reset): {:?}",
            sensor.last_recovery()
        );
        return Ok(());
    }
    let max_args = if ["capture", "stream"].contains(&command) {
        2
    } else {
        1
    };
    if args.len() > max_args {
        return Err(Error::InvalidState("too many command arguments"));
    }
    match command {
        "probe" | "info" | "diagnose" => {
            let devices = discover()?;
            for d in &devices {
                println!(
                    "USB {:03}/{:03} {:04x}:{:04x} {:?} {:?}",
                    d.bus, d.address, d.vendor_id, d.product_id, d.manufacturer, d.product
                );
                if command != "probe" {
                    println!("{d:#?}");
                }
            }
            if devices.is_empty() {
                return Err(Error::DeviceNotFound);
            }
            if command == "info" {
                let sensor = Xtion::open()?;
                println!("Firmware: {:?}", sensor.firmware());
                println!(
                    "Experimental requested mode: VGA 640x480 / 30 FPS, bulk packed 11-bit shifts (XTION_WIRE_FORMAT=16 selects unpacked shifts)"
                );
            }
            if command == "diagnose" {
                acquire("diagnose", None)?;
            }
        }
        "capture" | "stream" => acquire(command, args.get(1).map(String::as_str))?,
        "help" | "--help" | "-h" => println!(
            "Usage: xtion probe | info | capture [frame.pgm] | stream [seconds=10] | diagnose | stress [seconds] [cycles] | recover [--device-reset]\nXTION_TRACE=1 logs target control transfers. XTION_DUMP=new.usbraw saves depth bytes.\nCommands fail honestly if no target enumerates; see docs/device.md and docs/validation.md."
        ),
        _ => return Err(Error::InvalidState("unknown command; run xtion --help")),
    }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtion: {e}");
            match &e {
                Error::DeviceNotFound => eprintln!(
                    "No PS1080 candidate is visible to libusb. Check the physical USB connection; RGB webcams are not compatible targets."
                ),
                Error::Usb(rusb::Error::Access) => eprintln!(
                    "Device access denied. See README.md for a narrowly scoped udev rule; no automatic privilege escalation is attempted."
                ),
                Error::Timeout => eprintln!(
                    "Check XTION_TRACE=1 for the control transfer or frame read that timed out."
                ),
                _ => {}
            }
            ExitCode::FAILURE
        }
    }
}

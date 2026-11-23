use std::{
    fs,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use xtion::{Error, Result, Xtion};

fn rss_kib() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|s| s.starts_with("VmRSS:"))
                .and_then(|s| s.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

pub fn run(args: &[String]) -> Result<()> {
    let number = |index: usize, default: u64, max: u64| -> Result<u64> {
        let value = args
            .get(index)
            .map(|s| s.parse())
            .transpose()
            .map_err(|_| Error::InvalidState("stress expects seconds and cycles"))?
            .unwrap_or(default);
        if !(1..=max).contains(&value) {
            return Err(Error::InvalidState("stress argument outside bounds"));
        }
        Ok(value)
    };
    if args.len() > 2 {
        return Err(Error::InvalidState("usage: stress [seconds] [cycles]"));
    }
    let seconds = number(0, 2, 600)?;
    let cycles = number(1, 1, 100)?;
    let mut sensor = Xtion::open()?;
    for cycle in 1..=cycles {
        sensor.start_depth()?;
        // Stress exposes failures directly. Restarting would mix device clock epochs.
        let sample = (|| -> Result<()> {
            let cutoff = sensor.host_elapsed() + Duration::from_secs(1);
            let warmup = Instant::now();
            let first = loop {
                let frame = sensor.read_depth_frame_timeout(Duration::from_secs(5))?;
                if frame.host_arrival() >= cutoff {
                    break frame;
                }
                if warmup.elapsed() > Duration::from_secs(6) {
                    return Err(Error::Timeout);
                }
            };
            let baseline = sensor.statistics();
            let start = Instant::now();
            let begin = first.host_arrival();
            let mut last = begin;
            let mut ticks = first.timestamp();
            let mut strides = Vec::new();
            let mut peak = rss_kib();
            let initial_rss = peak;
            let mut latency = Duration::ZERO;
            while last - begin < Duration::from_secs(seconds) {
                let frame = sensor.read_depth_frame_timeout(Duration::from_secs(5))?;
                last = frame.host_arrival();
                let delta = frame.timestamp().wrapping_sub(ticks);
                if delta == 0 || delta > i32::MAX as u32 {
                    return Err(Error::Frame("nonprogressing clock"));
                }
                strides.push(delta);
                ticks = frame.timestamp();
                latency = latency.max(sensor.host_elapsed().saturating_sub(last));
                if strides.len().is_multiple_of(30) {
                    peak = peak.max(rss_kib());
                }
                if start.elapsed() > Duration::from_secs(seconds + 6) {
                    return Err(Error::Timeout);
                }
            }
            let stats = sensor.statistics();
            let frames = strides.len();
            strides.sort_unstable();
            let total_ticks: u64 = strides.iter().map(|&n| u64::from(n)).sum();
            let device_fps = sensor
                .clock_hz()
                .map(|hz| format!("{:.3}", frames as f64 * hz / total_ticks as f64))
                .unwrap_or_else(|| "null".into());
            // At very low completeness the median observed stride spans multiple
            // frames. Prefer the configured cadence and the device's own clock.
            let period = sensor
                .clock_hz()
                .map(|hz| (hz / f64::from(first.mode().fps)).round() as u64)
                .filter(|v| *v > 0)
                .unwrap_or(u64::from(strides[frames / 2]));
            let missed: u64 = strides
                .iter()
                .map(|&v| ((u64::from(v) + period / 2) / period).saturating_sub(1))
                .sum();
            let elapsed = (last - begin).as_secs_f64();
            let fps = frames as f64 / elapsed;
            let delivery_fps = frames as f64 / start.elapsed().as_secs_f64();
            let incomplete = stats.incomplete_frames - baseline.incomplete_frames;
            let meets_target = fps >= 25.0
                && missed * 100 < (frames as u64 + missed)
                && incomplete * 100 < (frames as u64 + incomplete)
                && stats.usb_errors == baseline.usb_errors
                && stats.decode_errors == baseline.decode_errors;
            let bps = (stats.bytes_received - baseline.bytes_received) as f64 / elapsed;
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            println!(
                "{{\"cycle\":{cycle},\"unix_ms\":{ms},\"frames\":{frames},\"arrival_seconds\":{elapsed:.4},\"complete_fps\":{fps:.3},\"device_clock_fps\":{device_fps},\"meets_vga_target\":{meets_target},\"delivery_fps\":{delivery_fps:.3},\"usb_bytes_s\":{bps:.0},\"missing_tick_intervals_estimate\":{missed},\"incomplete\":{},\"sequence_gaps\":{},\"decoder_errors\":{},\"discarded_bytes\":{},\"queue_drops\":{},\"usb_errors\":{},\"usb_timeouts\":{},\"startup_ack_missing\":{},\"rss_initial_kib\":{initial_rss},\"rss_peak_kib\":{peak},\"rss_final_kib\":{},\"max_host_latency_ms\":{}}}",
                stats.incomplete_frames - baseline.incomplete_frames,
                stats.sequence_gaps - baseline.sequence_gaps,
                stats.decode_errors - baseline.decode_errors,
                stats.discarded_bytes - baseline.discarded_bytes,
                stats.dropped_usb_transfers - baseline.dropped_usb_transfers,
                stats.usb_errors - baseline.usb_errors,
                stats.usb_timeouts - baseline.usb_timeouts,
                stats.enable_ack_missing,
                rss_kib(),
                latency.as_millis()
            );
            if seconds >= 120 && !meets_target {
                return Err(Error::Frame(
                    "steady acquisition below the VGA reliability target",
                ));
            }
            Ok(())
        })();
        let stop = sensor.stop_depth();
        println!(
            "{{\"cycle\":{cycle},\"verified_stop\":{},\"state\":\"{:?}\"}}",
            stop.is_ok(),
            sensor.state()
        );
        match (sample, stop) {
            (Err(cause), Err(cleanup)) => {
                return Err(Error::Cleanup {
                    cause: Box::new(cause),
                    cleanup: Box::new(cleanup),
                });
            }
            (Err(e), _) | (_, Err(e)) => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

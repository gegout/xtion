use crate::*;
use std::{
    io::{self, Read, Write},
    thread,
    time::{Duration, Instant},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub trait FrameSource {
    fn next_frame(&mut self) -> Result<Option<DepthFrame>>;
    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}
pub struct LiveSource(xtion::Xtion);
impl LiveSource {
    pub fn open() -> Result<Self> {
        let mut sensor = xtion::Xtion::open()?;
        sensor.start_depth()?;
        Ok(Self(sensor))
    }
}
impl FrameSource for LiveSource {
    fn next_frame(&mut self) -> Result<Option<DepthFrame>> {
        let f = self.0.read_depth_frame_timeout(Duration::from_secs(2))?;
        let s = self.0.statistics();
        Ok(Some(DepthFrame {
            info: FrameInfo {
                width: f.width(),
                height: f.height(),
                metric: f.units() == xtion::DepthUnits::Millimetres,
                ticks: f.timestamp(),
                arrival_ns: f.host_arrival().as_nanos() as u64,
                intrinsics: CameraIntrinsics::nominal(f.width(), f.height()),
                health: CaptureHealth {
                    state: format!("{:?}", self.0.state()),
                    bytes: s.bytes_received,
                    frames: s.frames_received,
                    incomplete: s.incomplete_frames,
                    startup_discards: s.startup_frames_discarded,
                    gaps: s.sequence_gaps,
                    queue_drops: s.dropped_usb_transfers,
                    errors: s.usb_errors,
                    timeouts: s.usb_timeouts,
                    decode_errors: s.decode_errors,
                    discarded_bytes: s.discarded_bytes,
                    missing_enable_ack: s.enable_ack_missing,
                },
            },
            pixels: f.pixels().to_vec(),
        }))
    }
    fn stop(&mut self) -> Result<()> {
        self.0.stop_depth()?;
        Ok(())
    }
}
const MAGIC: &[u8; 8] = b"XTVISION";
fn pixel_count(i: &FrameInfo) -> Result<usize> {
    i.width
        .checked_mul(i.height)
        .filter(|n| (1..=4_194_304).contains(n))
        .ok_or_else(|| "invalid recording dimensions".into())
}
pub struct Recorder<W: Write>(W);
impl<W: Write> Recorder<W> {
    pub fn new(mut w: W) -> Result<Self> {
        w.write_all(MAGIC)?;
        w.write_all(&[1])?;
        Ok(Self(w))
    }
    pub fn frame(&mut self, f: &DepthFrame) -> Result<()> {
        if pixel_count(&f.info)? != f.pixels.len() {
            return Err("invalid pixel count".into());
        }
        let meta = serde_json::to_vec(&f.info)?;
        self.0.write_all(&(meta.len() as u32).to_le_bytes())?;
        self.0.write_all(&meta)?;
        let mut bytes = Vec::with_capacity(f.pixels.len() * 2);
        for p in &f.pixels {
            bytes.extend_from_slice(&p.to_le_bytes())
        }
        self.0.write_all(&bytes)?;
        Ok(())
    }
    pub fn finish(mut self) -> Result<W> {
        self.0.flush()?;
        Ok(self.0)
    }
}
pub struct ReplaySource<R: Read> {
    input: R,
    real_time: bool,
    prev: Option<u64>,
    due: Instant,
}
impl<R: Read> ReplaySource<R> {
    pub fn new(mut input: R, real_time: bool) -> Result<Self> {
        let mut header = [0; 9];
        input.read_exact(&mut header)?;
        if &header[..8] != MAGIC || header[8] != 1 {
            return Err("unsupported XTV recording".into());
        }
        Ok(Self {
            input,
            real_time,
            prev: None,
            due: Instant::now(),
        })
    }
}
impl<R: Read> FrameSource for ReplaySource<R> {
    fn next_frame(&mut self) -> Result<Option<DepthFrame>> {
        let mut len = [0; 4];
        if self.input.read(&mut len[..1])? == 0 {
            return Ok(None);
        }
        self.input.read_exact(&mut len[1..])?;
        let n = u32::from_le_bytes(len) as usize;
        if n > 65536 {
            return Err("oversized metadata".into());
        }
        let mut meta = vec![0; n];
        self.input.read_exact(&mut meta)?;
        let info: FrameInfo = serde_json::from_slice(&meta)?;
        let mut bytes = vec![0; 2 * pixel_count(&info)?];
        self.input.read_exact(&mut bytes)?;
        if let Some(last) = self.prev {
            let delta = info
                .arrival_ns
                .checked_sub(last)
                .ok_or(io::Error::other("arrival time reversed"))?;
            if self.real_time {
                self.due += Duration::from_nanos(delta);
                thread::sleep(self.due.saturating_duration_since(Instant::now()))
            }
        } else {
            self.due = Instant::now()
        }
        self.prev = Some(info.arrival_ns);
        Ok(Some(DepthFrame {
            info,
            pixels: bytes
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_record_header_is_an_error() {
        let mut data = Recorder::new(Vec::new()).unwrap().finish().unwrap();
        data.push(5);
        assert!(
            ReplaySource::new(&data[..], false)
                .unwrap()
                .next_frame()
                .is_err()
        );
    }
}

/// Static background followed by a rectangular surface approaching, pausing and retreating.
#[derive(Default)]
pub struct SyntheticSource {
    index: u32,
}
impl FrameSource for SyntheticSource {
    fn next_frame(&mut self) -> Result<Option<DepthFrame>> {
        let t = self.index;
        if t >= 100 {
            return Ok(None);
        }
        self.index += 1;
        let (w, h) = (96, 64);
        let mut pixels = vec![3000; w * h];
        if (10..70).contains(&t) && t != 24 {
            let x = 12 + (t.min(30) - 10) as usize;
            let z = if t < 30 {
                1600 - (t - 10) * 10
            } else if t < 45 {
                1400
            } else {
                1400 + (t - 45) * 10
            };
            for y in 20..44 {
                pixels[y * w + x..y * w + x + 24].fill(z as u16)
            }
        }
        Ok(Some(DepthFrame {
            info: FrameInfo {
                width: w,
                height: h,
                metric: true,
                ticks: t * 100,
                arrival_ns: t as u64 * 40_000_000,
                intrinsics: CameraIntrinsics::nominal(w, h),
                health: CaptureHealth {
                    frames: t as u64 + 1,
                    state: "Streaming".into(),
                    ..Default::default()
                },
            },
            pixels,
        }))
    }
}

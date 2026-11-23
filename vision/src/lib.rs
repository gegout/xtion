//! Depth-only observations. Zero is missing data.
pub mod source;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum IntrinsicsSource {
    Calibrated,
    Nominal,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraIntrinsics {
    pub source: IntrinsicsSource,
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
}
impl CameraIntrinsics {
    pub fn nominal(w: usize, h: usize) -> Self {
        Self {
            source: IntrinsicsSource::Nominal,
            fx: w as f64 / (2. * 29_f64.to_radians().tan()),
            fy: h as f64 / (2. * 22.5_f64.to_radians().tan()),
            cx: (w as f64 - 1.) / 2.,
            cy: (h as f64 - 1.) / 2.,
        }
    }
}
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureHealth {
    pub state: String,
    pub bytes: u64,
    pub frames: u64,
    pub incomplete: u64,
    pub startup_discards: u64,
    pub gaps: u64,
    pub queue_drops: u64,
    pub errors: u64,
    pub timeouts: u64,
    pub decode_errors: u64,
    pub discarded_bytes: u64,
    pub missing_enable_ack: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameInfo {
    pub width: usize,
    pub height: usize,
    pub metric: bool,
    pub ticks: u32,
    pub arrival_ns: u64,
    pub intrinsics: CameraIntrinsics,
    pub health: CaptureHealth,
}
#[derive(Clone, Debug, PartialEq)]
pub struct DepthFrame {
    pub info: FrameInfo,
    pub pixels: Vec<u16>,
}
pub mod ai;
pub mod describe;
pub mod geometry;
pub mod image;
pub mod perception;

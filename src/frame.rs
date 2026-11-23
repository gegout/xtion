use crate::{Error, Result};
use std::time::Duration;

/// Physical units must be checked before interpreting pixel differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthUnits {
    RawShift,
    Millimetres,
}

/// The only mode currently requested from firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthMode {
    pub width: u16,
    pub height: u16,
    pub fps: u16,
}
impl DepthMode {
    pub const VGA_30: Self = Self {
        width: 640,
        height: 480,
        fps: 30,
    };
    pub(crate) fn pixel_count(self) -> usize {
        usize::from(self.width) * usize::from(self.height)
    }
}

/// Owned contiguous pixels. Zero always means invalid; no implicit unit conversion.
#[derive(Debug, Clone)]
pub struct DepthFrame {
    mode: DepthMode,
    units: DepthUnits,
    timestamp: u32,
    arrival: Duration,
    pixels: Vec<u16>,
}
#[derive(Debug, Clone, Copy)]
pub struct DepthStats {
    pub min: Option<u16>,
    pub max: Option<u16>,
    pub mean: Option<f64>,
    pub invalid_pixels: usize,
}
impl DepthFrame {
    /// Construct a frame from decoded pixels, also useful for offline consumers.
    /// Raw shifts must be normalized: 1..=2046 are valid, zero is invalid.
    pub fn new(
        mode: DepthMode,
        units: DepthUnits,
        timestamp: u32,
        arrival: Duration,
        pixels: Vec<u16>,
    ) -> Result<Self> {
        if mode.width == 0
            || mode.height == 0
            || mode.fps == 0
            || pixels.len() != mode.pixel_count()
        {
            return Err(Error::Frame("dimensions/FPS do not match pixel storage"));
        }
        if units == DepthUnits::RawShift && pixels.iter().any(|&p| p >= 2047) {
            return Err(Error::Frame(
                "raw shifts must normalize invalid values to zero",
            ));
        }
        Ok(Self {
            mode,
            units,
            timestamp,
            arrival,
            pixels,
        })
    }
    pub fn width(&self) -> usize {
        usize::from(self.mode.width)
    }
    pub fn height(&self) -> usize {
        usize::from(self.mode.height)
    }
    pub fn mode(&self) -> DepthMode {
        self.mode
    }
    pub fn units(&self) -> DepthUnits {
        self.units
    }
    /// Raw wrapping EOF device ticks; clock frequency is not assumed.
    pub fn timestamp(&self) -> u32 {
        self.timestamp
    }
    /// Monotonic elapsed host time since opening this sensor.
    pub fn host_arrival(&self) -> Duration {
        self.arrival
    }
    pub fn pixels(&self) -> &[u16] {
        &self.pixels
    }
    pub fn pixel(&self, x: usize, y: usize) -> Option<u16> {
        if x >= self.width() || y >= self.height() {
            None
        } else {
            Some(self.pixels[y * self.width() + x])
        }
    }
    pub fn stats(&self) -> DepthStats {
        let mut min = None;
        let mut max = None;
        let mut sum = 0u64;
        let mut valid = 0;
        for &p in &self.pixels {
            if p == 0 {
                continue;
            }
            min = Some(min.map_or(p, |v: u16| v.min(p)));
            max = Some(max.map_or(p, |v: u16| v.max(p)));
            sum += u64::from(p);
            valid += 1;
        }
        DepthStats {
            min,
            max,
            mean: (valid > 0).then(|| sum as f64 / valid as f64),
            invalid_pixels: self.pixels.len() - valid,
        }
    }
    /// Count differences above a threshold in this frame's units. Invalid pairs are skipped.
    pub fn changed_pixels(&self, other: &Self, threshold: u16) -> Result<usize> {
        if self.mode != other.mode || self.units != other.units {
            return Err(Error::Frame("cannot compare different modes or units"));
        }
        Ok(self
            .pixels
            .iter()
            .zip(&other.pixels)
            .filter(|&(a, b)| *a != 0 && *b != 0 && a.abs_diff(*b) > threshold)
            .count())
    }
}

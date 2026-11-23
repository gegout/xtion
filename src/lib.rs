//! Experimental native PS1080 USB depth access. See docs/protocol.md for evidence and limits.
mod depth;
mod device;
mod error;
mod frame;
mod protocol;
mod recovery;
mod usb;

pub use device::{AcquisitionStats, DepthFrames, Health, State, Xtion};
pub use error::{Error, Result};
pub use frame::{DepthFrame, DepthMode, DepthStats, DepthUnits};
pub use protocol::FirmwareVersion;
pub use recovery::RecoveryReport;
pub use usb::{DeviceInfo, EndpointInfo, InterfaceInfo, discover};

use std::{error, fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    DeviceNotFound,
    MultipleDevices,
    Usb(rusb::Error),
    Io(io::Error),
    Unsupported(String),
    Protocol(&'static str),
    Nack(u16),
    InvalidState(&'static str),
    Frame(&'static str),
    Timeout,
    Operation {
        operation: &'static str,
        cause: Box<Error>,
    },
    Recovery {
        cause: Box<Error>,
        recovery: Box<Error>,
    },
    Cleanup {
        cause: Box<Error>,
        cleanup: Box<Error>,
    },
}

impl Error {
    pub fn root_cause(&self) -> &Self {
        match self {
            Self::Operation { cause, .. } | Self::Cleanup { cause, .. } => cause.root_cause(),
            Self::Recovery { recovery, .. } => recovery.root_cause(),
            _ => self,
        }
    }
    pub fn category(&self) -> &'static str {
        match self.root_cause() {
            Self::DeviceNotFound | Self::Usb(rusb::Error::NoDevice) => "disconnected",
            Self::Usb(rusb::Error::Access) => "access_denied",
            Self::Usb(rusb::Error::Busy) => "interface_busy",
            Self::Timeout => "timeout",
            Self::Nack(_) => "firmware_nack",
            Self::Usb(_) => "usb",
            Self::Frame(_) => "frame",
            Self::Protocol(_) => "protocol",
            Self::InvalidState(_) => "state",
            Self::Unsupported(_) => "unsupported",
            Self::MultipleDevices => "ambiguous_device",
            _ => "io",
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery { cause, recovery } => write!(f, "{cause}. Recovery failed: {recovery}"),
            Self::Operation { operation, cause } => write!(f, "{operation}: {cause}"),
            Self::Cleanup { cause, cleanup } => {
                write!(f, "{cause}; cleanup also failed: {cleanup}")
            }
            Self::DeviceNotFound => write!(
                f,
                "no candidate PS1080 USB device found (1d27:0500/0600/0601/0609)"
            ),
            Self::MultipleDevices => write!(
                f,
                "multiple sensors found; select one with Xtion::open_at(bus, address)"
            ),
            Self::Usb(e) => write!(f, "USB: {e}"),
            Self::Io(e) => write!(f, "I/O: {e}"),
            Self::Unsupported(s) => write!(f, "unsupported sensor: {s}"),
            Self::Protocol(s) => write!(f, "invalid protocol response: {s}"),
            Self::Nack(n) => write!(f, "firmware rejected command (NACK {n})"),
            Self::InvalidState(s) => write!(f, "invalid acquisition state: {s}"),
            Self::Frame(s) => write!(f, "invalid depth frame: {s}"),
            Self::Timeout => write!(
                f,
                "timed out waiting for a complete depth frame or USB reply"
            ),
        }
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Recovery { cause, .. }
            | Self::Cleanup { cause, .. }
            | Self::Operation { cause, .. } => Some(cause.as_ref()),
            Self::Usb(e) => Some(e),
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl From<rusb::Error> for Error {
    fn from(e: rusb::Error) -> Self {
        if e == rusb::Error::Timeout {
            Self::Timeout
        } else {
            Self::Usb(e)
        }
    }
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

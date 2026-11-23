//! Recovery is scoped to the original physical port and bounded to one USB reset.
use crate::{DeviceInfo, Error, Result, Xtion, discover, usb::Usb};
use std::{
    thread,
    time::{Duration, Instant},
};

impl DeviceInfo {
    pub fn same_port_device(&self, other: &Self) -> bool {
        !self.ports.is_empty()
            && self.bus == other.bus
            && self.ports == other.ports
            && self.vendor_id == other.vendor_id
            && self.product_id == other.product_id
            && self.device_version == other.device_version
            && self
                .serial_number
                .as_ref()
                .is_none_or(|s| other.serial_number.as_ref() == Some(s))
    }
}

fn locate(identity: &DeviceInfo) -> Result<DeviceInfo> {
    discover()?
        .into_iter()
        .find(|d| identity.same_port_device(d))
        .ok_or(Error::DeviceNotFound)
}

#[derive(Debug, Clone, Default)]
pub struct RecoveryReport {
    pub open_attempts: u32,
    pub device_reset_attempted: bool,
    pub previous_error: Option<String>,
}

fn retry_port<T>(
    allow_reset: bool,
    mut open: impl FnMut() -> Result<T>,
    mut reset_device: impl FnMut() -> Result<()>,
    mut backoff: impl FnMut() -> bool,
) -> Result<(T, RecoveryReport)> {
    let mut report = RecoveryReport::default();
    let mut consecutive = 0;
    loop {
        report.open_attempts += 1;
        let error = match open() {
            Ok(value) => return Ok((value, report)),
            Err(error) => error,
        };
        match error.root_cause() {
            Error::DeviceNotFound | Error::Usb(rusb::Error::NoDevice) => consecutive = 0,
            Error::Timeout
            | Error::Usb(rusb::Error::Io | rusb::Error::Pipe)
            | Error::InvalidState("depth traffic continued after stop") => {
                consecutive += 1;
                if consecutive >= 2 {
                    if !allow_reset || report.device_reset_attempted {
                        return Err(error);
                    }
                    if let Err(recovery) = reset_device() {
                        return Err(Error::Recovery {
                            cause: Box::new(error),
                            recovery: Box::new(recovery),
                        });
                    }
                    report.device_reset_attempted = true;
                    consecutive = 0;
                }
            }
            _ => return Err(error),
        }
        report.previous_error = Some(error.to_string());
        if !backoff() {
            return Err(error);
        }
    }
}

impl Xtion {
    /// Return a verified idle handle at the original physical port. Reset is
    /// opt-in, limited to one attempt and applied only to a claimed target.
    pub fn recover_at(identity: &DeviceInfo, allow_device_reset: bool) -> Result<Self> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let (mut sensor, report) = retry_port(
            allow_device_reset,
            || {
                let d = locate(identity)?;
                let mut sensor = Self::open_at(d.bus, d.address)?;
                if !identity.same_port_device(sensor.info()) {
                    return Err(Error::DeviceNotFound);
                }
                sensor.reconcile_idle()?;
                Ok(sensor)
            },
            || {
                if Instant::now() >= deadline {
                    return Err(Error::Timeout);
                }
                let d = locate(identity)?;
                let usb = Usb::open(d.bus, d.address)?;
                if !identity.same_port_device(&usb.info) {
                    return Err(Error::DeviceNotFound);
                }
                match usb.client.io.reset() {
                    Ok(()) | Err(rusb::Error::NotFound | rusb::Error::NoDevice) => Ok(()),
                    Err(e) => Err(e.into()),
                }
            },
            || {
                let remaining = deadline.saturating_duration_since(Instant::now());
                thread::sleep(remaining.min(Duration::from_millis(300)));
                Instant::now() < deadline
            },
        )?;
        sensor.record_recovery(report);
        Ok(sensor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disappearance_waits_passively_and_timeout_reopens_before_reset() {
        let mut outcomes = [
            Err(Error::DeviceNotFound),
            Err(Error::Timeout),
            Err(Error::Timeout),
            Err(Error::Usb(rusb::Error::NoDevice)),
            Ok(57),
        ]
        .into_iter();
        let mut resets = 0;
        let (address, report) = retry_port(
            true,
            || outcomes.next().unwrap(),
            || {
                resets += 1;
                Ok(())
            },
            || true,
        )
        .unwrap();
        assert_eq!((address, report.open_attempts, resets), (57, 5, 1));
        assert!(report.device_reset_attempted);
    }
    #[test]
    fn absence_busy_and_refused_reset_do_not_reset_other_devices() {
        let no_reset = || panic!("unauthorized reset");
        let mut polls = 0;
        assert!(matches!(
            retry_port::<()>(
                true,
                || Err(Error::DeviceNotFound),
                no_reset,
                || {
                    polls += 1;
                    polls < 3
                }
            ),
            Err(Error::DeviceNotFound)
        ));
        assert_eq!(polls, 3);
        assert!(
            retry_port::<()>(
                true,
                || Err(Error::Usb(rusb::Error::Busy)),
                no_reset,
                || panic!()
            )
            .is_err()
        );
        assert!(retry_port::<()>(false, || Err(Error::Timeout), no_reset, || true).is_err());
    }
    #[test]
    fn repeated_control_failure_permits_at_most_one_target_reset() {
        let mut resets = 0;
        assert!(
            retry_port::<()>(
                true,
                || Err(Error::Timeout),
                || {
                    resets += 1;
                    Ok(())
                },
                || true
            )
            .is_err()
        );
        assert_eq!(resets, 1);
    }
    fn identity() -> DeviceInfo {
        DeviceInfo {
            vendor_id: 0x1d27,
            product_id: 0x601,
            bus: 1,
            address: 56,
            ports: vec![2],
            usb_version: rusb::Version(2, 0, 0),
            device_version: rusb::Version(0, 0, 1),
            speed: rusb::Speed::High,
            active_configuration: Some(1),
            manufacturer: None,
            product: None,
            serial_number: Some("one".into()),
            access_error: None,
            interfaces: vec![],
        }
    }
    #[test]
    fn reconnect_matches_new_address_but_never_another_port_or_serial() {
        let original = identity();
        let mut next = original.clone();
        next.address = 89;
        assert!(original.same_port_device(&next));
        next.ports = vec![3];
        assert!(!original.same_port_device(&next));
        next.ports = vec![2];
        next.serial_number = None;
        assert!(!original.same_port_device(&next));
    }
}

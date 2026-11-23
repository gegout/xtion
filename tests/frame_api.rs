use std::time::Duration;
use xtion::{DepthFrame, DepthMode, DepthUnits, Error};

fn frame(pixels: Vec<u16>) -> DepthFrame {
    DepthFrame::new(
        DepthMode {
            width: 2,
            height: 2,
            fps: 30,
        },
        DepthUnits::Millimetres,
        42,
        Duration::ZERO,
        pixels,
    )
    .unwrap()
}
#[test]
fn external_consumer_compares_frames_without_usb() {
    let a = frame(vec![0, 1000, 1100, 2000]);
    let b = frame(vec![500, 1100, 1301, 1900]);
    assert_eq!(a.changed_pixels(&b, 100).unwrap(), 1);
    assert_eq!(a.pixel(1, 1), Some(2000));
    assert_eq!(a.pixel(2, 0), None);
    assert_eq!(a.pixel(usize::MAX, usize::MAX), None);
    assert_eq!(a.timestamp(), 42);
    assert_eq!(a.pixels().len(), 4);
    assert_eq!(a.stats().invalid_pixels, 1);
    assert_eq!(a.stats().min, Some(1000));
    assert_eq!(a.stats().max, Some(2000));
    assert_eq!(a.stats().mean, Some(4100.0 / 3.0));
    assert_eq!(frame(vec![0; 4]).stats().mean, None);
}
#[test]
fn dimensions_encoding_and_unit_mismatches_are_rejected() {
    assert!(
        DepthFrame::new(
            DepthMode::VGA_30,
            DepthUnits::RawShift,
            0,
            Duration::ZERO,
            vec![]
        )
        .is_err()
    );
    assert!(
        DepthFrame::new(
            DepthMode {
                width: 1,
                height: 1,
                fps: 30
            },
            DepthUnits::RawShift,
            0,
            Duration::ZERO,
            vec![2047]
        )
        .is_err()
    );
    let a = frame(vec![1000; 4]);
    let b = DepthFrame::new(
        a.mode(),
        DepthUnits::RawShift,
        0,
        Duration::ZERO,
        vec![1000; 4],
    )
    .unwrap();
    assert!(a.changed_pixels(&b, 100).is_err());
}
#[test]
fn usb_failure_categories_remain_distinct() {
    assert!(matches!(Error::from(rusb::Error::Timeout), Error::Timeout));
    assert!(matches!(
        Error::from(rusb::Error::Access),
        Error::Usb(rusb::Error::Access)
    ));
    assert!(matches!(
        Error::from(rusb::Error::NoDevice),
        Error::Usb(rusb::Error::NoDevice)
    ));
    assert!(Error::DeviceNotFound.to_string().contains("no candidate"));
}

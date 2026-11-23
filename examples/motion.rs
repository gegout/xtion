use xtion::{DepthUnits, Error, Xtion};

fn main() -> xtion::Result<()> {
    let threshold: u16 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "100".into())
        .parse()
        .map_err(|_| Error::InvalidState("expected a millimetre threshold (u16)"))?;
    let mut sensor = Xtion::open()?;
    sensor.start_depth()?;
    let result = (|| -> xtion::Result<()> {
        let first = sensor.read_depth_frame()?;
        if first.units() != DepthUnits::Millimetres {
            return Err(Error::Unsupported(
                "motion threshold is in millimetres; device calibration is required".into(),
            ));
        }
        for second in sensor.depth_frames().take(90) {
            let second = second?;
            println!(
                "Changed since initial frame: {} (threshold {threshold} mm)",
                first.changed_pixels(&second, threshold)?
            );
        }
        Ok(())
    })();
    let cleanup = sensor.stop_depth();
    result?;
    cleanup
}

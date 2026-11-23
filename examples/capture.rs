use xtion::Xtion;

fn main() -> xtion::Result<()> {
    let mut sensor = Xtion::open()?;
    sensor.start_depth()?;
    let result = sensor.read_depth_frame();
    let cleanup = sensor.stop_depth();
    let frame = result?;
    cleanup?;
    println!(
        "{}x{} {:?}: {:?}",
        frame.width(),
        frame.height(),
        frame.units(),
        frame.stats()
    );
    Ok(())
}

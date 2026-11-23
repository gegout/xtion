use std::time::Instant;
use xtion::Xtion;

fn main() -> xtion::Result<()> {
    let mut sensor = Xtion::open()?;
    sensor.start_depth()?;
    let start = Instant::now();
    let mut count = 0;
    let result = (|| -> xtion::Result<()> {
        for frame in sensor.depth_frames().take(300) {
            let frame = frame?;
            count += 1;
            println!(
                "frame={count} FPS={:.2} {:?} {:?}",
                count as f64 / start.elapsed().as_secs_f64(),
                frame.units(),
                frame.stats()
            );
        }
        Ok(())
    })();
    let cleanup = sensor.stop_depth();
    println!("{:?}", sensor.statistics());
    result?;
    cleanup
}

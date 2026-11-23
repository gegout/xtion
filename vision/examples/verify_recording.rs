use std::{
    fs::File,
    io::{BufReader, BufWriter},
};
use xtion_vision::source::*;
fn main() -> Result<()> {
    let path = std::env::args().nth(1).ok_or("supply a new output path")?;
    let mut sensor = LiveSource::open()?;
    let mut writer = Recorder::new(BufWriter::new(File::create_new(&path)?))?;
    let mut expected = Vec::new();
    for _ in 0..15 {
        let f = sensor.next_frame()?.ok_or("live source ended")?;
        writer.frame(&f)?;
        expected.push(f)
    }
    sensor.stop()?;
    writer.finish()?;
    let mut replay = ReplaySource::new(BufReader::new(File::open(path)?), false)?;
    for f in expected {
        assert_eq!(Some(f), replay.next_frame()?);
    }
    assert_eq!(None, replay.next_frame()?);
    println!("15 live frames matched replay including pixels, clocks, intrinsics and health");
    Ok(())
}

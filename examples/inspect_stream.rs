//! Read-only diagnostic for an already streaming 1d27:0601 sensor. Sends no vendor commands.
use rusb::{Context, UsbContext};
use std::{
    fs::OpenOptions,
    io::Write,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = Context::new()?;
    let device = context
        .devices()?
        .iter()
        .find(|d| {
            d.device_descriptor()
                .is_ok_and(|d| d.vendor_id() == 0x1d27 && d.product_id() == 0x0601)
        })
        .ok_or("sensor absent")?;
    let handle = device.open()?;
    if handle.kernel_driver_active(0)? {
        return Err("sensor interface already bound".into());
    }
    handle.claim_interface(0)?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/xtion-live-endpoint.usbraw".into()),
    )?;
    let mut data = vec![0; 1024 * 1024];
    let start = Instant::now();
    let mut total = 0;
    while start.elapsed() < Duration::from_secs(2) {
        match handle.read_bulk(0x81, &mut data, Duration::from_millis(200)) {
            Ok(n) => {
                if total == 0 {
                    println!("First bytes: {:02x?}", &data[..n.min(64)]);
                }
                output.write_all(&data[..n])?;
                total += n;
            }
            Err(rusb::Error::Timeout) => {}
            Err(error) => return Err(error.into()),
        }
    }
    output.flush()?;
    handle.release_interface(0)?;
    println!(
        "Endpoint bytes read: {total} in {:.3}s",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

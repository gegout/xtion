//! Diagnostic: query an already running sensor while draining its depth endpoint.
use rusb::{Context, UsbContext};
use std::time::{Duration, Instant};
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
    handle.claim_interface(0)?;
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let mut buffer = vec![0; 1024 * 1024];
            let start = Instant::now();
            let mut total = 0;
            while start.elapsed() < Duration::from_secs(3) {
                match handle.read_bulk(0x81, &mut buffer, Duration::from_millis(200)) {
                    Ok(n) => total += n,
                    Err(rusb::Error::Timeout) => {}
                    Err(e) => {
                        eprintln!("bulk: {e}");
                        break;
                    }
                }
            }
            total
        });
        std::thread::sleep(Duration::from_millis(100));
        let request = [0x47, 0x4d, 0, 0, 0, 0, 1, 0x90];
        println!(
            "GET_VERSION write: {:?}",
            handle.write_control(0x40, 0, 0, 0, &request, Duration::from_millis(500))
        );
        let mut response = [0; 512];
        match handle.read_control(0xc0, 0, 0, 0, &mut response, Duration::from_millis(500)) {
            Ok(n) => println!("GET_VERSION reply: {:02x?}", &response[..n]),
            Err(e) => println!("control read: {e}"),
        }
        println!("Concurrent depth bytes: {}", reader.join().unwrap());
    });
    handle.release_interface(0)?;
    Ok(())
}

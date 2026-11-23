fn main() -> xtion::Result<()> {
    for device in xtion::discover()? {
        println!("{device:#?}");
    }
    Ok(())
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !oxibar::ipc::run_cli(&args)? {
        oxibar::app::run()?;
    }
    Ok(())
}

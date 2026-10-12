fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [help] if help == "--help" => {
            println!("Usage: oxisession-kiosk --backend cage|hyprland");
            Ok(())
        }
        [flag, backend] if flag == "--backend" => oxi_session::kiosk::run(backend),
        [flag, state, backend] if flag == "--child" => {
            oxi_session::kiosk::child(std::path::Path::new(state), backend)
        }
        _ => Err("invalid kiosk arguments".into()),
    }
}

fn main() {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        oxi_pam::harden_process()?;
        match arguments.as_slice() {
            [argument] if argument == "--watch" => oxi_lock::sleep::watch(),
            [argument] if argument == "--suspend" => oxi_lock::sleep::suspend(),
            [argument] if argument == "--help" => {
                println!("Usage: oxilock-sleep --watch | --suspend");
                Ok(())
            }
            _ => Err("Use --watch or --suspend".into()),
        }
    })();
    if let Err(error) = result {
        eprintln!("oxilock-sleep: {error}");
        std::process::exit(1);
    }
}

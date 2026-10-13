use oxi_session::{catalog::Catalog, frontend, ipc::Client, model};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Select CPU rendering before any worker can start; never probe login-seat GPU drivers.
    unsafe {
        std::env::set_var("ICED_BACKEND", "tiny-skia");
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--help"] {
        println!(
            "Usage: oxisession [--list-sessions]\nFullscreen greetd greeter; requires root-owned GREETD_SOCK."
        );
        return Ok(());
    }
    model::harden_process()?;
    let catalog = Catalog::load()?;
    if args.as_slice() == ["--list-sessions"] {
        for session in catalog.sessions {
            println!("{}\t{}", session.id, session.name);
        }
        return Ok(());
    }
    if !args.is_empty() {
        return Err("invalid oxisession arguments".into());
    }
    frontend::run(Client::connect()?, catalog)
}

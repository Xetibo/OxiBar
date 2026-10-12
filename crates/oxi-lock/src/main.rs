use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--check"] {
        return match oxi_lock::backend::Locker::connect() {
            Ok(_) => {
                println!("ext-session-lock-v1 available");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("oxilock: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if args == ["--help"] {
        println!("oxilock --check\nCheck compositor session-lock support without locking.");
        return ExitCode::SUCCESS;
    }
    eprintln!("oxilock: authentication integration is not enabled yet");
    ExitCode::FAILURE
}

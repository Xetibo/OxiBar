use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--pam-worker", "3"] {
        return if oxi_pam::run_worker(3).is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
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
        println!(
            "oxilock [--ready-fd FD]\noxilock --check\nLock using ext-session-lock-v1 and the current user's oxilock PAM service."
        );
        return ExitCode::SUCCESS;
    }
    let result = (|| -> Result<(), oxi_lock::backend::Error> {
        oxi_pam::harden_process()?;
        oxi_pam::ensure_service().map_err(
            |_| "install an administrator-owned /etc/pam.d/oxilock service before locking",
        )?;
        let readiness = if args.is_empty() {
            None
        } else if args.len() == 2 && args[0] == "--ready-fd" {
            let fd: i32 = args[1].parse()?;
            if fd < 3 {
                return Err("readiness descriptor must be at least 3".into());
            }
            Some(
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(format!("/proc/self/fd/{fd}"))?,
            )
        } else {
            return Err("unknown arguments; use --help".into());
        };
        let app = oxi_lock::application::LockScreen::new()?;
        match oxi_lock::backend::Locker::connect()?.run(app, readiness)? {
            oxi_lock::backend::Exit::Authenticated
            | oxi_lock::backend::Exit::ExternallyUnlocked => Ok(()),
            oxi_lock::backend::Exit::Denied => Err("compositor denied session lock".into()),
        }
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oxilock: {error}");
            ExitCode::FAILURE
        }
    }
}

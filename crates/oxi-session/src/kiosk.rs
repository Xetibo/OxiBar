use std::{
    fs,
    io::{self, Read},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

type Error = Box<dyn std::error::Error>;
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> io::Result<Self> {
        let runtime = PathBuf::from(
            std::env::var_os("XDG_RUNTIME_DIR")
                .ok_or_else(|| invalid("missing greeter runtime directory"))?,
        );
        private_directory(&runtime)?;
        let mut nonce = [0u8; 16];
        fs::File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let name: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = runtime.join(format!("oxisession-kiosk-{name}"));
        fs::create_dir(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        Ok(Self(path))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Compositor(Child);
impl Compositor {
    fn stop(&mut self) {
        if self.0.try_wait().ok().flatten().is_some() {
            return;
        }
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}
impl Drop for Compositor {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn run(backend: &str) -> Result<(), Error> {
    if !["cage", "hyprland"].contains(&backend) {
        return Err("unsupported kiosk backend".into());
    }
    let directory = Directory::new()?;
    let executable = std::env::current_exe()?;
    let mut command = if backend == "cage" {
        let mut command = Command::new(binary("OXI_CAGE", "cage"));
        command
            .args(["-s", "--"])
            .arg(&executable)
            .arg("--child")
            .arg(&directory.0)
            .arg(backend);
        command
    } else {
        let config = directory.0.join("hyprland.conf");
        fs::write(&config, hyprland_config(&executable, &directory.0))?;
        let mut command = Command::new(binary("OXI_HYPRLAND", "Hyprland"));
        command.arg("--config").arg(config);
        command
    };
    for key in [
        "WAYLAND_DISPLAY",
        "WAYLAND_SOCKET",
        "DISPLAY",
        "HYPRLAND_INSTANCE_SIGNATURE",
    ] {
        command.env_remove(key);
    }
    command.env("XDG_SESSION_TYPE", "wayland").process_group(0);
    let parent = unsafe { libc::getpid() };
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                libc::_exit(127);
            }
            Ok(())
        });
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
    }
    let mut compositor = Compositor(command.spawn()?);
    fs::write(directory.0.join("pid"), compositor.0.id().to_string())?;
    let mut acknowledged_at = None;
    let status = loop {
        if STOP.load(Ordering::Relaxed) {
            return Err("kiosk terminated".into());
        }
        if let Some(status) = compositor.0.try_wait()? {
            break status;
        }
        if fs::metadata(directory.0.join("result")).is_ok() {
            let time = acknowledged_at.get_or_insert_with(Instant::now);
            if time.elapsed() > Duration::from_secs(10) {
                return Err("kiosk compositor did not release its seat".into());
            }
        }
        thread::sleep(Duration::from_millis(50));
    };
    let result = fs::read_to_string(directory.0.join("result")).unwrap_or_default();
    if !accepted(&result, status) {
        return Err("greeter or kiosk exited without accepted handoff".into());
    }
    Ok(())
}

pub fn child(state: &Path, backend: &str) -> Result<(), Error> {
    private_directory(state)?;
    if !["cage", "hyprland"].contains(&backend) {
        return Err("unsupported kiosk backend".into());
    }
    if backend == "hyprland" {
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")?;
        if signature.len() > 256 || signature.chars().any(char::is_control) {
            return Err("invalid owned instance".into());
        }
        fs::write(state.join("instance"), signature)?;
    }
    let greeter = std::env::current_exe()?
        .parent()
        .ok_or("missing greeter binary directory")?
        .join("oxisession");
    let result = Command::new(greeter).status();
    let succeeded = result.as_ref().is_ok_and(|status| status.success());
    let temporary = state.join("result.tmp");
    fs::write(
        &temporary,
        if succeeded {
            "accepted\n"
        } else {
            "rejected\n"
        },
    )?;
    fs::rename(temporary, state.join("result"))?;
    if backend == "hyprland" {
        exit_owned_hyprland(state)?;
    }
    if !succeeded {
        return Err("greeter rejected or interrupted handoff".into());
    }
    Ok(())
}

fn exit_owned_hyprland(state: &Path) -> Result<(), Error> {
    let signature = fs::read_to_string(state.join("instance"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let pid = loop {
        if let Ok(pid) = fs::read_to_string(state.join("pid")) {
            break pid.parse::<u32>()?;
        }
        if Instant::now() >= deadline {
            return Err("missing owned compositor identity".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let hyprctl = binary("OXI_HYPRCTL", "hyprctl");
    let instances = Command::new(&hyprctl).args(["instances", "-j"]).output()?;
    let instances: serde_json::Value = serde_json::from_slice(&instances.stdout)?;
    if !owns_instance(&instances, &signature, pid) {
        return Err("refusing to stop an unrelated compositor".into());
    }
    if !Command::new(hyprctl)
        .args(["-i", &signature, "dispatch", "exit"])
        .status()?
        .success()
    {
        return Err("owned compositor exit failed".into());
    }
    Ok(())
}

fn owns_instance(instances: &serde_json::Value, signature: &str, pid: u32) -> bool {
    instances.as_array().is_some_and(|instances| {
        instances.iter().any(|instance| {
            instance["instance"].as_str() == Some(signature)
                && instance["pid"].as_u64() == Some(pid.into())
        })
    })
}
fn accepted(result: &str, status: ExitStatus) -> bool {
    result == "accepted\n" && status.success()
}
fn private_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::getuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "kiosk state directory is not private",
        ));
    }
    Ok(())
}
fn binary(key: &str, fallback: &str) -> std::ffi::OsString {
    std::env::var_os(key).unwrap_or_else(|| fallback.into())
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn hyprland_config(executable: &Path, state: &Path) -> String {
    let executable = shell_words::quote(&executable.to_string_lossy()).into_owned();
    let state = shell_words::quote(&state.to_string_lossy()).into_owned();
    format!(
        "monitor = ,preferred,auto,1\nmisc {{\n disable_hyprland_logo = true\n disable_splash_rendering = true\n}}\nexec-once = {executable} --child {state} hyprland\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    #[test]
    fn compositor_exit_requires_both_accepted_greeter_and_successful_teardown() {
        assert!(!accepted("", ExitStatus::from_raw(0)));
        assert!(!accepted("rejected\n", ExitStatus::from_raw(0)));
        assert!(!accepted("accepted\n", ExitStatus::from_raw(1 << 8)));
        assert!(accepted("accepted\n", ExitStatus::from_raw(0)));
        let instances =
            serde_json::json!([{"instance":"owned","pid":42},{"instance":"desktop","pid":99}]);
        assert!(owns_instance(&instances, "owned", 42));
        assert!(!owns_instance(&instances, "desktop", 42));
        assert!(!owns_instance(&instances, "owned", 99));
        let config = hyprland_config(
            Path::new("/system/path with spaces/kiosk"),
            Path::new("/runtime/private dir"),
        );
        assert!(
            config.contains(
                "'/system/path with spaces/kiosk' --child '/runtime/private dir' hyprland"
            )
        );
        assert!(!config.contains("source ="));
    }
}

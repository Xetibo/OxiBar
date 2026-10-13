pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty()
        && let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR")
        && let Some(display) = oxibar_plugin_api::launch::wayland_display(
            std::path::Path::new(&runtime),
            std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        )
    {
        // SAFETY: daemon startup has not initialized IPC, tracing, or workers.
        unsafe { std::env::set_var("WAYLAND_DISPLAY", display) };
    }
    if !oxibar::ipc::run_cli(&args)? {
        oxibar::app::run()?;
    }
    Ok(())
}

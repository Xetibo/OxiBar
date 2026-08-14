use ::hyprland::{data::Monitor, shared::HyprDataActive};

pub(super) fn active_output_name() -> Option<String> {
    Monitor::get_active().ok().map(|monitor| monitor.name)
}

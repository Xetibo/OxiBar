use super::WindowState;
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    sync::{LazyLock, Mutex},
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::presentation_time::client::{
    wp_presentation::{self, WpPresentation},
    wp_presentation_feedback::{self, WpPresentationFeedback},
};

static TRACE: LazyLock<Option<Mutex<(File, u32)>>> = LazyLock::new(|| {
    let path = std::env::var_os("OXIBAR_PERF_TRACE")?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .ok()
        .map(|file| Mutex::new((file, 0)))
});

pub(crate) fn enabled() -> bool {
    TRACE.is_some()
}
pub(crate) fn record(value: Value) {
    if let Some(trace) = TRACE.as_ref()
        && let Ok(mut guard) = trace.lock()
        && guard.1 < 20_000
    {
        let _ = writeln!(guard.0, "{value}");
        guard.1 += 1;
    }
}

#[derive(Debug)]
pub(crate) struct Feedback {
    pub id: super::id::Id,
    pub namespace: String,
    pub frame: u64,
}

impl<T> Dispatch<WpPresentation, ()> for WindowState<T> {
    fn event(
        _: &mut Self,
        _: &WpPresentation,
        event: wp_presentation::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_presentation::Event::ClockId { clk_id } = event {
            record(json!({"event":"presentation_clock","clock_id":clk_id}));
        }
    }
}
impl<T> Dispatch<WpPresentationFeedback, Feedback> for WindowState<T> {
    fn event(
        _: &mut Self,
        _: &WpPresentationFeedback,
        event: wp_presentation_feedback::Event,
        data: &Feedback,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_presentation_feedback::Event::Presented {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
                refresh,
                ..
            } => {
                let stamp = ((u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo))
                    .saturating_mul(1_000_000_000)
                    .saturating_add(u64::from(tv_nsec));
                record(
                    json!({"event":"presented","surface":format!("{:?}",data.id),"namespace":data.namespace,"frame":data.frame,"stamp_ns":stamp,"refresh_ns":refresh}),
                );
            }
            wp_presentation_feedback::Event::Discarded => record(
                json!({"event":"discarded","surface":format!("{:?}",data.id),"namespace":data.namespace,"frame":data.frame}),
            ),
            _ => {}
        }
    }
}

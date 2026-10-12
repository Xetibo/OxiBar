use std::{
    collections::BTreeSet,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use iced::Task;
use iced_layershell::reexport::{IcedId, KeyboardInteractivity};
use oxibar_plugin_api::HostPasteRequest;

use crate::{Message, app::OxiBar};

#[derive(Default)]
pub(crate) struct PasteState {
    focused: BTreeSet<IcedId>,
    generation: u64,
    pending: Option<Attempt>,
}

struct Attempt {
    plugin: String,
    shortcut: String,
    dismissed: bool,
    running: bool,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Attempt {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl PasteState {
    fn ready(&self) -> bool {
        self.focused.is_empty()
            && self
                .pending
                .as_ref()
                .is_some_and(|attempt| attempt.dismissed && !attempt.running)
    }
}

impl OxiBar {
    pub(crate) fn request_paste(
        &mut self,
        plugin: String,
        request: HostPasteRequest,
    ) -> Task<Message> {
        if !self.plugins.contains_key(&plugin) {
            return Task::none();
        }
        if !matches!(request.shortcut.as_str(), "ctrl+v" | "ctrl+shift+v") {
            return paste_result(plugin, Err("Unsupported paste shortcut".into()));
        }
        if self.paste.pending.is_some() {
            return paste_result(plugin, Err("Another paste is still running".into()));
        }
        self.paste.generation = self.paste.generation.wrapping_add(1);
        let generation = self.paste.generation;
        self.paste.pending = Some(Attempt {
            plugin,
            shortcut: request.shortcut,
            dismissed: false,
            running: false,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        tracing::debug!(focused = ?self.paste.focused, generation, "releasing keyboard for paste");
        let release = self
            .paste
            .focused
            .iter()
            .copied()
            .map(|id| {
                Task::done(Message::SetToastKeyboardInteractivity(
                    id,
                    KeyboardInteractivity::None,
                ))
            })
            .collect::<Vec<_>>();
        let dismiss = Task::batch([
            self.close_bar_dropdown(),
            self.close_control_center(),
            self.close_modal(),
            self.close_panel(),
            self.close_launcher(),
            self.close_oxipaste(),
            self.close_settings(),
            self.close_overview(),
            self.close_session(),
            Task::batch(release),
        ]);
        Task::batch([
            dismiss.chain(Task::done(Message::PasteDismissed(generation))),
            Task::perform(
                async {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                },
                move |_| Message::PasteTimeout(generation),
            ),
        ])
    }

    pub(crate) fn paste_focus(&mut self, id: IcedId, focused: bool) -> Task<Message> {
        tracing::debug!(?id, focused, "native keyboard focus");
        if focused {
            self.paste.focused.insert(id);
        } else {
            self.paste.focused.remove(&id);
        }
        if focused
            && self
                .paste
                .pending
                .as_ref()
                .is_some_and(|attempt| attempt.running)
        {
            return self.finish_paste(self.paste.generation, Err("Paste cancelled because an OxiBar surface regained keyboard focus. Item remains copied.".into()));
        }
        self.start_paste_if_ready()
    }

    pub(crate) fn paste_dismissed(&mut self, generation: u64) -> Task<Message> {
        if generation == self.paste.generation
            && let Some(attempt) = &mut self.paste.pending
        {
            attempt.dismissed = true;
        }
        self.start_paste_if_ready()
    }

    pub(crate) fn paste_timeout(&mut self, generation: u64) -> Task<Message> {
        if self
            .paste
            .pending
            .as_ref()
            .is_some_and(|attempt| !attempt.running)
        {
            self.finish_paste(
                generation,
                Err(
                    "Keyboard focus did not return to the application. Item remains copied.".into(),
                ),
            )
        } else {
            Task::none()
        }
    }

    fn start_paste_if_ready(&mut self) -> Task<Message> {
        if !self.paste.ready() {
            return Task::none();
        }
        let attempt = self.paste.pending.as_mut().expect("ready paste");
        if !self.plugins.contains_key(&attempt.plugin) {
            self.paste.pending = None;
            return Task::none();
        }
        attempt.running = true;
        let shortcut = attempt.shortcut.clone();
        let cancelled = attempt.cancelled.clone();
        let generation = self.paste.generation;
        Task::perform(
            async move {
                let (sender, receiver) = iced::futures::channel::oneshot::channel();
                std::thread::spawn(move || {
                    let _ = sender.send(inject(&shortcut, &cancelled));
                });
                receiver
                    .await
                    .unwrap_or_else(|_| Err("Paste worker stopped. Item remains copied.".into()))
            },
            move |result| Message::PasteFinished(generation, result),
        )
    }

    pub(crate) fn finish_paste(
        &mut self,
        generation: u64,
        result: Result<(), String>,
    ) -> Task<Message> {
        if generation != self.paste.generation {
            return Task::none();
        }
        let Some(attempt) = self.paste.pending.take() else {
            return Task::none();
        };
        paste_result(attempt.plugin.clone(), result)
    }
}

fn paste_result(plugin: String, result: Result<(), String>) -> Task<Message> {
    Task::done(Message::PluginEmission(
        plugin,
        HostPasteRequest::result_message(result),
    ))
}

fn inject(shortcut: &str, cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("Paste cancelled".into());
    }
    let mut command = Command::new("wtype");
    command.args(["-M", "ctrl"]);
    if shortcut == "ctrl+shift+v" {
        command.args(["-M", "shift"]);
    }
    command.args(["-P", "v", "-p", "v"]);
    if shortcut == "ctrl+shift+v" {
        command.args(["-m", "shift"]);
    }
    let mut child = command
        .args(["-m", "ctrl"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not start wtype: {error}. Item remains copied."))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!("Paste failed: {status}. Item remains copied."));
            }
            Ok(None) if Instant::now() < deadline && !cancelled.load(Ordering::Acquire) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Paste cancelled or timed out. Item remains copied.".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_requires_dismissal_and_actual_focus_release() {
        let id = IcedId::unique();
        let mut state = PasteState::default();
        state.focused.insert(id);
        state.pending = Some(Attempt {
            plugin: "OxiPaste".into(),
            shortcut: "ctrl+v".into(),
            dismissed: false,
            running: false,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        assert!(!state.ready());
        state.pending.as_mut().unwrap().dismissed = true;
        assert!(!state.ready());
        state.focused.remove(&id);
        assert!(state.ready());
        state.pending.as_mut().unwrap().running = true;
        assert!(!state.ready());
    }

    #[test]
    fn retirement_cancels_worker_and_wire_requests_are_restricted() {
        let cancelled = Arc::new(AtomicBool::new(false));
        drop(Attempt {
            plugin: "OxiPaste".into(),
            shortcut: "ctrl+v".into(),
            dismissed: true,
            running: true,
            cancelled: cancelled.clone(),
        });
        assert!(cancelled.load(Ordering::Acquire));
        for shortcut in ["ctrl+v", "ctrl+shift+v"] {
            let message = HostPasteRequest {
                shortcut: shortcut.into(),
            }
            .into_message();
            assert_eq!(
                HostPasteRequest::parse_host_request(message.downcast_ref::<String>().unwrap())
                    .unwrap()
                    .shortcut,
                shortcut
            );
        }
        assert!(HostPasteRequest::parse_host_request("oxibar.host.paste:ctrl+v;rm -rf").is_none());
        assert!(inject("ctrl+v", &cancelled).is_err());
    }
}

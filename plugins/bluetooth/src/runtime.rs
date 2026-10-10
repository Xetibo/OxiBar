use iced::Task;
use oxibar_plugin_api::{
    HOST_REQUEST_CLOSE_MODAL, HOST_REQUEST_OPEN_MODAL, HOST_REQUEST_TOGGLE_POPUP, PluginMsg,
};

use super::{Action, Message, Model, PairEvent, PairingState, Reply, msg};

fn host(request: &str) -> Task<PluginMsg> {
    Task::done(std::sync::Arc::new(request.to_owned()) as PluginMsg)
}

pub(super) fn update(model: &mut Model, message: Message) -> Option<Task<PluginMsg>> {
    match message {
        Message::TogglePopup => Some(host(HOST_REQUEST_TOGGLE_POPUP)),
        Message::Refresh => {
            model.client.refresh();
            None
        }
        Message::Snapshot(result) => {
            match result {
                Ok(snapshot) => {
                    if model
                        .adapter
                        .as_ref()
                        .is_none_or(|path| snapshot.adapter(path).is_none())
                    {
                        model.adapter = snapshot
                            .adapters
                            .iter()
                            .find(|adapter| adapter.powered)
                            .or_else(|| snapshot.adapters.first())
                            .map(|adapter| adapter.path.clone());
                    }
                    model.snapshot = snapshot;
                    model.health = None;
                }
                Err(error) => {
                    if model.health.as_ref() != Some(&error) {
                        model.errors.push(error.clone());
                    }
                    model.health = Some(error);
                    model.client.cancel_pair(model.active_pair);
                }
            }
            None
        }
        Message::SelectAdapter(path) => {
            if model.snapshot.adapter(&path).is_some() {
                model.adapter = Some(path);
            }
            None
        }
        Message::Hover(path) => {
            model.hovered = path;
            None
        }
        Message::Scan => {
            let adapter = model
                .adapter
                .as_deref()
                .and_then(|path| model.snapshot.adapter(path))?;
            action(
                model,
                Action::Discovery(adapter.path.clone(), !adapter.scanning),
                "Scanning",
            )
        }
        Message::Power(enabled) => action(
            model,
            Action::Power(model.adapter.clone()?, enabled),
            "Changing adapter power",
        ),
        Message::Disconnect(path) => action(model, Action::Disconnect(path), "Disconnecting"),
        Message::Trust(path, enabled) => {
            action(model, Action::Trust(path, enabled), "Changing trust")
        }
        Message::Forget(path) => action(model, Action::Forget(path), "Removing device"),
        Message::ActionDone { generation, result } => {
            if generation != model.generation {
                return None;
            }
            model.pending = None;
            if let Err(error) = result {
                report(model, error);
            }
            None
        }
        Message::Pair(path) => match model.client.pair(path) {
            Ok((id, events)) => {
                model.active_pair = Some(id);
                model.pair_label = Some("Pairing / connecting".to_owned());
                model.cancel_requested = false;
                model.action_error = None;
                Some(Task::run(events, move |event| {
                    msg(Message::PairEvent(id, event))
                }))
            }
            Err(error) => {
                report(model, error);
                None
            }
        },
        Message::PairEvent(id, event) => {
            if model.active_pair != Some(id) {
                return None;
            }
            match event {
                PairEvent::Prompt(_) | PairEvent::Progress(_) if model.cancel_requested => None,
                PairEvent::Prompt(prompt) => {
                    model.pairing = Some(PairingState {
                        prompt,
                        code: String::new(),
                        waiting: false,
                        error: None,
                    });
                    Some(host(HOST_REQUEST_OPEN_MODAL))
                }
                PairEvent::Progress(label) => {
                    model.pair_label = Some(label);
                    None
                }
                PairEvent::Finished(result) => {
                    let had_modal = model.pairing.take().is_some();
                    model.active_pair = None;
                    model.pair_label = None;
                    if let Err(error) = result
                        && !model.cancel_requested
                        && !error.contains("pairing canceled")
                    {
                        report(model, error);
                    }
                    model.cancel_requested = false;
                    had_modal.then(|| host(HOST_REQUEST_CLOSE_MODAL))
                }
            }
        }
        Message::CodeChanged(value) => {
            if let Some(pairing) = &mut model.pairing {
                pairing.code = value;
                pairing.error = None;
            }
            None
        }
        Message::SubmitCode => {
            let value = model.pairing.as_ref()?.code.clone();
            respond(model, Reply::Text(value));
            None
        }
        Message::Answer(accepted) => {
            respond(
                model,
                if accepted {
                    Reply::Accept
                } else {
                    Reply::Reject
                },
            );
            if !accepted {
                model.cancel_requested = true;
            }
            None
        }
        Message::CancelPair => {
            let had_modal = model.pairing.is_some();
            cancel(model);
            had_modal.then(|| host(HOST_REQUEST_CLOSE_MODAL))
        }
    }
}

pub(super) fn cancel(model: &mut Model) {
    if model.active_pair.is_some() {
        model.client.cancel_pair(model.active_pair);
        model.cancel_requested = true;
        model.pair_label = Some("Canceling pairing".to_owned());
    }
    model.pairing = None;
}

fn respond(model: &mut Model, reply: Reply) {
    if let Some(pairing) = &mut model.pairing {
        if pairing.waiting {
            return;
        }
        match model.client.respond(pairing.prompt.id, reply) {
            Ok(()) => {
                pairing.waiting = true;
                pairing.error = None;
            }
            Err(error) => pairing.error = Some(error),
        }
    }
}

fn action(model: &mut Model, action: Action, label: &str) -> Option<Task<PluginMsg>> {
    let reply = model.client.submit(action);
    model.generation = model.generation.wrapping_add(1);
    let generation = model.generation;
    model.pending = Some(label.to_owned());
    model.action_error = None;
    Some(Task::perform(
        async move {
            reply
                .await
                .unwrap_or_else(|_| Err("bluetooth: native owner stopped".to_owned()))
        },
        move |result| msg(Message::ActionDone { generation, result }),
    ))
}

fn report(model: &mut Model, error: String) {
    model.action_error = Some(error.clone());
    model.errors.push(error);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_action_and_pairing_events_cannot_clear_newer_state() {
        let mut model = Model::new(oxibar_plugin_api::toml::Table::new());
        model.generation = 2;
        model.pending = Some("new action".to_owned());
        assert!(
            update(
                &mut model,
                Message::ActionDone {
                    generation: 1,
                    result: Err("old error".to_owned())
                }
            )
            .is_none()
        );
        assert_eq!(model.pending.as_deref(), Some("new action"));
        model.active_pair = Some(8);
        assert!(
            update(
                &mut model,
                Message::PairEvent(7, PairEvent::Finished(Ok(())))
            )
            .is_none()
        );
        assert_eq!(model.active_pair, Some(8));
        assert!(model.errors.is_empty());
    }

    #[test]
    fn disconnect_retains_cached_devices_and_recovery_clears_health() {
        let mut model = Model::new(oxibar_plugin_api::toml::Table::new());
        model.snapshot.adapters.push(super::super::system::Adapter {
            path: "/org/bluez/hci0".to_owned(),
            name: "Cached radio".to_owned(),
            powered: true,
            discovering: false,
            scanning: false,
        });
        update(&mut model, Message::Snapshot(Err("offline".to_owned())));
        assert_eq!(model.snapshot.adapters[0].name, "Cached radio");
        let snapshot = model.snapshot.clone();
        update(&mut model, Message::Snapshot(Ok(snapshot)));
        assert!(model.health.is_none());
        assert_eq!(model.adapter.as_deref(), Some("/org/bluez/hci0"));
    }

    #[test]
    fn queued_prompt_cannot_reopen_a_canceled_pairing_modal() {
        let mut model = Model::new(oxibar_plugin_api::toml::Table::new());
        model.active_pair = Some(8);
        cancel(&mut model);
        assert!(
            update(
                &mut model,
                Message::PairEvent(
                    8,
                    PairEvent::Prompt(super::super::agent::Prompt {
                        id: 9,
                        device: "/org/bluez/hci0/dev_1".to_owned(),
                        kind: super::super::agent::PromptKind::Pin,
                    })
                )
            )
            .is_none()
        );
        assert!(
            update(
                &mut model,
                Message::PairEvent(8, PairEvent::Progress("Connecting".to_owned()))
            )
            .is_none()
        );
        assert!(model.pairing.is_none());
        assert_eq!(model.pair_label.as_deref(), Some("Canceling pairing"));
    }
}

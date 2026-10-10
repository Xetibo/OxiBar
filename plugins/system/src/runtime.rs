use iced::Task;
use oxibar_plugin_api::{CommandOutcome, HostOsdRequest, PluginMsg, toml::Table};

use crate::{
    HOST_REQUEST_CLOSE_MODAL, HOST_REQUEST_OPEN_MODAL, HOST_REQUEST_TOGGLE_POPUP, Message, Model,
    plugin_message,
    system::{Action, Domain, SessionAction, Snapshot},
    worker,
};

pub fn configure(model: &mut Model, config: &Table) {
    let table = config.get("system").and_then(|value| value.as_table());
    let text = |key| {
        table
            .and_then(|table| table.get(key))
            .and_then(|value| value.as_str())
    };
    let preferred = text("preferred_backlight").unwrap_or_default();
    if model.preferred_config != preferred {
        model.preferred = preferred.to_owned();
        model.preferred_config = preferred.to_owned();
    }
    model.locker = text("lock_command")
        .and_then(|value| shell_words::split(value).ok())
        .filter(|values| values.first().is_some_and(|value| !value.is_empty()));
    model.lock_enabled = table
        .and_then(|table| table.get("lock_enabled"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
}

fn host(value: &'static str) -> Task<PluginMsg> {
    Task::done(std::sync::Arc::new(value.to_owned()) as PluginMsg)
}
fn osd(topic: &'static str, follow: bool) -> Task<PluginMsg> {
    let request = HostOsdRequest::new(topic);
    Task::done(
        (if follow {
            request.follow_focus()
        } else {
            request
        })
        .into_message(),
    )
}
fn failure(model: &mut Model, error: String) {
    model.error = Some(error.clone());
    model.errors.push(error);
}

fn apply_snapshot(model: &mut Model, snapshot: Snapshot) -> Vec<&'static str> {
    if snapshot.sequence < model.snapshot.sequence {
        return Vec::new();
    }
    let mut topics = Vec::new();
    let old = model
        .snapshot
        .backlight(&model.preferred)
        .map(|device| (device.name.clone(), device.percent()));
    let new = snapshot
        .backlight(&model.preferred)
        .map(|device| (device.name.clone(), device.percent()));
    if model.initialized
        && old.is_some()
        && old != new
        && new.is_some()
        && !model.pending.contains_key(&Domain::Brightness)
    {
        topics.push("brightness");
    }
    if model.initialized
        && model.snapshot.active_profile.is_some()
        && model.snapshot.active_profile != snapshot.active_profile
        && snapshot.profile_ready
        && snapshot.active_profile.is_some()
        && !model.pending.contains_key(&Domain::Profile)
    {
        topics.push("profile");
    }
    model.snapshot = snapshot;
    model.initialized = true;
    topics
}

fn begin(model: &mut Model, domain: Domain) -> u64 {
    model.generation = model.generation.wrapping_add(1);
    let generation = model.generation;
    model.pending.insert(domain, generation);
    model.error = None;
    generation
}
fn submit(model: &mut Model, action: Action) -> Result<Task<PluginMsg>, String> {
    action.validate()?;
    match &action {
        Action::Brightness(name, _)
            if !model
                .snapshot
                .backlights
                .iter()
                .any(|device| &device.name == name) =>
        {
            return Err("Backlight is unavailable".to_owned());
        }
        Action::Profile(profile)
            if !model.snapshot.profile_ready || !model.snapshot.profiles.contains(profile) =>
        {
            return Err("Power profile is unavailable".to_owned());
        }
        Action::Session(action) if !model.snapshot.can(*action) => {
            return Err("Session action is unavailable".to_owned());
        }
        _ => {}
    }
    let domain = action.domain();
    if let Action::Brightness(name, percent) = &action {
        model.brightness_intent = Some((name.clone(), *percent));
    }
    let receiver = model.client.submit(action);
    let generation = begin(model, domain);
    Ok(Task::perform(
        async move {
            receiver
                .await
                .unwrap_or_else(|_| Err("System backend stopped".to_owned()))
        },
        move |result| {
            plugin_message(Message::Done {
                domain,
                generation,
                result,
            })
        },
    ))
}
fn lock(model: &mut Model) -> Result<Task<PluginMsg>, String> {
    if !model.lock_enabled {
        return Err("Locking is disabled".to_owned());
    }
    let arguments = model
        .locker
        .clone()
        .ok_or("Configure system.lock_command with a dedicated locker")?;
    let generation = begin(model, Domain::Session);
    let snapshot = model.snapshot.clone();
    Ok(Task::perform(
        async move {
            worker::run(move || worker::launch(arguments))
                .await
                .map(|_| snapshot)
        },
        move |result| {
            plugin_message(Message::Done {
                domain: Domain::Session,
                generation,
                result,
            })
        },
    ))
}

pub fn update(model: &mut Model, message: Message) -> Option<Task<PluginMsg>> {
    match message {
        Message::TogglePopup => Some(host(HOST_REQUEST_TOGGLE_POPUP)),
        Message::Snapshot(snapshot) => {
            let topics = apply_snapshot(model, snapshot);
            (!topics.is_empty())
                .then(|| Task::batch(topics.into_iter().map(|topic| osd(topic, true))))
        }
        Message::Refresh => {
            model.client.refresh();
            None
        }
        Message::SelectBacklight(name) => {
            if model
                .snapshot
                .backlights
                .iter()
                .any(|device| device.name == name)
            {
                model.preferred = name;
            }
            None
        }
        Message::Brightness(percent) => {
            let action = model
                .snapshot
                .backlight(&model.preferred)
                .map(|device| Action::Brightness(device.name.clone(), percent));
            match action
                .ok_or("Backlight is unavailable".to_owned())
                .and_then(|action| submit(model, action))
            {
                Ok(task) => Some(task),
                Err(error) => {
                    failure(model, error);
                    None
                }
            }
        }
        Message::Profile(profile) => match submit(model, Action::Profile(profile)) {
            Ok(task) => Some(task),
            Err(error) => {
                failure(model, error);
                None
            }
        },
        Message::Request(action) => {
            if !model.snapshot.can(action) {
                failure(model, "Session action is unavailable".to_owned());
                return None;
            }
            model.confirmation = Some(action);
            Some(host(HOST_REQUEST_OPEN_MODAL))
        }
        Message::Confirm => {
            let action = model.confirmation.take()?;
            match submit(model, Action::Session(action)) {
                Ok(task) => Some(Task::batch([host(HOST_REQUEST_CLOSE_MODAL), task])),
                Err(error) => {
                    failure(model, error);
                    Some(host(HOST_REQUEST_CLOSE_MODAL))
                }
            }
        }
        Message::Cancel => {
            model.confirmation = None;
            Some(host(HOST_REQUEST_CLOSE_MODAL))
        }
        Message::Lock => match lock(model) {
            Ok(task) => Some(task),
            Err(error) => {
                failure(model, error);
                None
            }
        },
        Message::Done {
            domain,
            generation,
            result,
        } => {
            if model.pending.get(&domain) != Some(&generation) {
                return None;
            }
            if domain == Domain::Brightness {
                model.brightness_intent = None;
            }
            match result {
                Ok(snapshot) => {
                    let topics = apply_snapshot(model, snapshot);
                    model.pending.remove(&domain);
                    let own = match domain {
                        Domain::Brightness => Some(osd("brightness", false)),
                        Domain::Profile => Some(osd("profile", false)),
                        Domain::Session => None,
                    };
                    let tasks = topics
                        .into_iter()
                        .map(|topic| osd(topic, true))
                        .chain(own)
                        .collect::<Vec<_>>();
                    (!tasks.is_empty()).then(|| Task::batch(tasks))
                }
                Err(error) => {
                    model.pending.remove(&domain);
                    failure(model, error);
                    None
                }
            }
        }
    }
}

pub fn percentage(value: &str, current: u8) -> Result<u8, String> {
    let value = value.strip_suffix('%').unwrap_or(value);
    let number: i16 = value
        .parse()
        .map_err(|_| "Expected a brightness percentage or signed step")?;
    if value.starts_with(['+', '-']) {
        Ok(i16::from(current).saturating_add(number).clamp(0, 100) as u8)
    } else if (0..=100).contains(&number) {
        Ok(number as u8)
    } else {
        Err("Brightness must be between 0 and 100".to_owned())
    }
}

pub fn command(
    model: &mut Model,
    verb: &str,
    arguments: &[String],
) -> Result<Task<CommandOutcome>, String> {
    let task = match (verb, arguments) {
        ("brightness", [value]) => {
            let device = model
                .snapshot
                .backlight(&model.preferred)
                .ok_or("Backlight is unavailable")?;
            let current = model
                .brightness_intent
                .as_ref()
                .filter(|(name, _)| name == &device.name)
                .map(|(_, percent)| *percent)
                .unwrap_or_else(|| device.percent());
            let action = Action::Brightness(device.name.clone(), percentage(value, current)?);
            submit(model, action)?
        }
        ("profile", [profile]) => submit(model, Action::Profile(profile.clone()))?,
        ("lock", []) => lock(model)?,
        ("refresh", []) => {
            model.client.refresh();
            Task::done(plugin_message(Message::Refresh))
        }
        (verb, []) if SessionAction::parse(verb).is_some() => {
            let action = SessionAction::parse(verb).unwrap();
            if !model.snapshot.can(action) {
                return Err("Session action is unavailable".to_owned());
            }
            // A resident command opens the same confirmation as the GUI.
            Task::done(plugin_message(Message::Request(action)))
        }
        _ => return Err("Unknown System command or arguments".to_owned()),
    };
    Ok(task.map(|message| {
        let result = message
            .downcast_ref::<Message>()
            .and_then(|message| match message {
                Message::Done { result, .. } => {
                    Some(result.as_ref().map(|_| ()).map_err(Clone::clone))
                }
                _ => None,
            })
            .unwrap_or(Ok(()));
        CommandOutcome { message, result }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentages_support_bounded_steps_and_reject_bad_literals() {
        assert_eq!(percentage("+5", 98).unwrap(), 100);
        assert_eq!(percentage("-20%", 10).unwrap(), 0);
        assert_eq!(percentage("42%", 0).unwrap(), 42);
        for value in ["101", "NaN", "50 60", "", "65535"] {
            assert!(percentage(value, 0).is_err());
        }
    }
}

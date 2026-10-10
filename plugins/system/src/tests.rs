use std::{collections::BTreeMap, sync::atomic::Ordering};

use iced::{Task, futures::StreamExt};
use oxibar_plugin_api::{HostOsdRequest, PluginMsg, toml::Table};

use crate::{
    Message, Model, native, runtime,
    system::{Backlight, Capability, Domain, Limits, SessionAction, Snapshot},
};

pub(super) fn collect<T: Send + 'static>(task: Task<T>) -> Vec<T> {
    let Some(mut stream) = iced_runtime::task::into_stream(task.collect()) else {
        return Vec::new();
    };
    iced::futures::executor::block_on(async {
        match stream.next().await {
            Some(iced_runtime::Action::Output(messages)) => messages,
            Some(_) => panic!("unexpected non-output action"),
            None => Vec::new(),
        }
    })
}

fn fixture() -> Model {
    Model::new(
        native::Client::new(Limits::from_config(&Table::new())),
        &Table::new(),
    )
}

fn snapshot(sequence: u64, percent: u32) -> Snapshot {
    Snapshot {
        sequence,
        backlights: vec![Backlight {
            name: "panel".to_owned(),
            current: percent,
            maximum: 100,
        }],
        profiles: vec!["balanced".to_owned(), "performance".to_owned()],
        active_profile: Some("balanced".to_owned()),
        profile_ready: true,
        login_ready: true,
        capabilities: BTreeMap::from([(SessionAction::PowerOff, Capability::Challenge)]),
        ..Snapshot::default()
    }
}

fn requests(task: Option<Task<PluginMsg>>) -> Vec<HostOsdRequest> {
    task.into_iter()
        .flat_map(collect)
        .filter_map(|message| {
            message
                .downcast_ref::<String>()
                .and_then(|value| HostOsdRequest::parse_host_request(value))
        })
        .collect()
}

#[test]
fn initial_state_is_quiet_and_domain_acknowledgements_preserve_external_changes() {
    let mut model = fixture();
    assert!(runtime::update(&mut model, Message::Snapshot(snapshot(1, 50))).is_none());
    let external = requests(runtime::update(
        &mut model,
        Message::Snapshot(snapshot(2, 65)),
    ));
    assert_eq!(
        external,
        vec![HostOsdRequest::new("brightness").follow_focus()]
    );
    assert!(runtime::update(&mut model, Message::Snapshot(snapshot(1, 10))).is_none());
    assert_eq!(model.snapshot.backlights[0].current, 65);
    model.pending.insert(Domain::Profile, 7);
    let mut updated = snapshot(3, 70);
    updated.active_profile = Some("performance".to_owned());
    let tasks = requests(runtime::update(
        &mut model,
        Message::Done {
            domain: Domain::Profile,
            generation: 7,
            result: Ok(updated),
        },
    ));
    assert_eq!(
        tasks,
        vec![
            HostOsdRequest::new("brightness").follow_focus(),
            HostOsdRequest::new("profile")
        ]
    );
    assert!(model.pending.is_empty());
}

#[test]
fn failed_and_stale_actions_do_not_emit_osds_or_clear_other_domains() {
    let mut model = fixture();
    runtime::update(&mut model, Message::Snapshot(snapshot(1, 50)));
    model.pending.insert(Domain::Brightness, 2);
    model.pending.insert(Domain::Profile, 3);
    assert!(
        runtime::update(
            &mut model,
            Message::Done {
                domain: Domain::Brightness,
                generation: 1,
                result: Ok(snapshot(2, 99))
            }
        )
        .is_none()
    );
    assert_eq!(model.snapshot.backlights[0].current, 50);
    assert!(
        runtime::update(
            &mut model,
            Message::Done {
                domain: Domain::Brightness,
                generation: 2,
                result: Err("permission denied".to_owned())
            }
        )
        .is_none()
    );
    assert_eq!(model.pending.get(&Domain::Profile), Some(&3));
    assert_eq!(model.snapshot.backlights[0].current, 50);
    assert_eq!(model.error.as_deref(), Some("permission denied"));
}

#[test]
fn resident_session_actions_only_request_confirmation_and_live_config_is_idempotent() {
    let mut model = fixture();
    runtime::update(&mut model, Message::Snapshot(snapshot(1, 50)));
    let intent = collect(runtime::command(&mut model, "power-off", &[]).unwrap());
    assert!(model.confirmation.is_none());
    assert!(model.pending.is_empty());
    let message = intent[0].message.downcast_ref::<Message>().unwrap().clone();
    let host = collect(runtime::update(&mut model, message).unwrap());
    assert_eq!(
        host[0].downcast_ref::<String>().map(String::as_str),
        Some(crate::HOST_REQUEST_OPEN_MODAL)
    );
    assert_eq!(model.confirmation, Some(SessionAction::PowerOff));
    runtime::update(&mut model, Message::Cancel);
    assert!(model.confirmation.is_none());
    assert!(runtime::command(&mut model, "hibernate", &[]).is_err());
    let config: Table = oxibar_plugin_api::toml::from_str("[system]\npreferred_backlight='panel'\nlock_enabled=false\nlock_command='locker --flag \"two words\"'").unwrap();
    runtime::configure(&mut model, &config);
    model.preferred = "temporary".to_owned();
    runtime::configure(&mut model, &config);
    assert_eq!(model.preferred, "temporary");
    assert_eq!(
        model.locker.as_ref().unwrap(),
        &["locker", "--flag", "two words"]
    );
    assert!(runtime::command(&mut model, "lock", &[]).is_err());
}

#[test]
fn repeated_relative_brightness_commands_accumulate_the_pending_intent() {
    let mut model = fixture();
    runtime::update(&mut model, Message::Snapshot(snapshot(1, 50)));
    model.client.ready.store(true, Ordering::Release);
    let first = runtime::command(&mut model, "brightness", &["+5".to_owned()]).unwrap();
    let second = runtime::command(&mut model, "brightness", &["+5".to_owned()]).unwrap();
    assert_eq!(model.brightness_intent, Some(("panel".to_owned(), 60)));
    assert_eq!(model.pending.get(&Domain::Brightness), Some(&2));
    assert!(runtime::command(&mut model, "brightness", &["101".to_owned()]).is_err());
    drop((first, second));
}

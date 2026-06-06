//! Notification center plugin implementing `org.freedesktop.Notifications`.

mod oxinoti;

use std::sync::{Arc, Mutex};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use iced::{Element, Task, futures::Stream, stream};
use oxibar_plugin_api::{
    ABI_VERSION, HOST_REQUEST_TOGGLE_PANEL, HostToastRequest, PluginModel, PluginMsg, PluginStream,
    drain_model_errors, plugin_model, toml::Table, with_model_read, with_model_write,
};

use oxinoti::{Event, Notification};

const TOAST_ACTIVE_RECHECK_MILLIS: u64 = 1_000;

#[derive(Debug)]
pub struct Model {
    notifications: Vec<Notification>,
    reply_texts: BTreeMap<u32, String>,
    hovered_notifications: BTreeSet<u32>,
    do_not_disturb: bool,
    timeout: Duration,
    next_toast_generation: u64,
    toast_generations: BTreeMap<u32, u64>,
    expired_toasts: BTreeSet<u32>,
    errors: Vec<String>,
}

impl Model {
    fn new(global_config: Table) -> Self {
        Self {
            notifications: Vec::new(),
            reply_texts: BTreeMap::new(),
            hovered_notifications: BTreeSet::new(),
            do_not_disturb: false,
            timeout: oxinoti::read_timeout(&global_config),
            next_toast_generation: 0,
            toast_generations: BTreeMap::new(),
            expired_toasts: BTreeSet::new(),
            errors: Vec::new(),
        }
    }

    fn upsert(&mut self, notification: Notification) {
        if !notification.allows_inline_reply() {
            self.reply_texts.remove(&notification.id());
        }
        if let Some(existing) = self
            .notifications
            .iter_mut()
            .find(|existing| existing.id() == notification.id())
        {
            *existing = notification;
        } else {
            self.notifications.insert(0, notification);
        }
    }

    fn remove(&mut self, id: u32) {
        self.notifications
            .retain(|notification| notification.id() != id);
        self.toast_generations.remove(&id);
        self.expired_toasts.remove(&id);
        self.reply_texts.remove(&id);
        self.hovered_notifications.remove(&id);
        oxinoti::clear_reply_focus(id);
    }

    fn is_toast_active(&self, id: u32) -> bool {
        self.hovered_notifications.contains(&id)
            || oxinoti::is_reply_focused(id)
            || self
                .reply_texts
                .get(&id)
                .is_some_and(|draft| !draft.trim().is_empty())
    }

    fn close_expired_toast_if_idle(&mut self, id: u32) -> Option<Task<PluginMsg>> {
        if !self.expired_toasts.contains(&id) || self.is_toast_active(id) {
            return None;
        }
        self.expired_toasts.remove(&id);
        self.toast_generations.remove(&id);
        Some(close_toast_task(id))
    }

    fn next_toast_generation(&mut self, id: u32) -> u64 {
        self.next_toast_generation += 1;
        self.toast_generations
            .insert(id, self.next_toast_generation);
        self.next_toast_generation
    }

    fn toast_timeout(&self, notification: &Notification) -> Duration {
        if notification.expire_timeout > 0 {
            Duration::from_millis(notification.expire_timeout as u64)
        } else {
            self.timeout
        }
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Notifications"
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(global_config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    (plugin_model(Model::new(global_config)), None)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, msg_in: PluginMsg) -> Option<Task<PluginMsg>> {
    let event = msg_in.downcast_ref::<Event>()?.clone();
    with_model_write::<Model, _>(&model, |model| match event {
        Event::TogglePanel => Some(Task::done(
            Arc::new(HOST_REQUEST_TOGGLE_PANEL.to_owned()) as PluginMsg
        )),
        Event::ToggleDoNotDisturb => {
            model.do_not_disturb = !model.do_not_disturb;
            if model.do_not_disturb {
                Some(Task::batch(
                    model
                        .notifications
                        .iter()
                        .map(|notification| close_toast_task(notification.id())),
                ))
            } else {
                None
            }
        }
        Event::SetDoNotDisturb(enabled) => {
            model.do_not_disturb = enabled;
            if enabled {
                Some(Task::batch(
                    model
                        .notifications
                        .iter()
                        .map(|notification| close_toast_task(notification.id())),
                ))
            } else {
                None
            }
        }
        Event::ClearAll => {
            let close_tasks = model
                .notifications
                .iter()
                .map(|notification| close_toast_task(notification.id()))
                .collect::<Vec<_>>();
            for notification in &model.notifications {
                oxinoti::clear_reply_focus(notification.id());
            }
            model.notifications.clear();
            model.toast_generations.clear();
            model.expired_toasts.clear();
            model.reply_texts.clear();
            model.hovered_notifications.clear();
            Some(Task::batch(close_tasks))
        }
        Event::Add(notification) => {
            let notification = *notification;
            let id = notification.id();
            let generation = model.next_toast_generation(id);
            let timeout = model.toast_timeout(&notification);
            let (width, height) = notification.toast_size();
            let toast_id = notification.toast_id();
            model.expired_toasts.remove(&id);
            model.upsert(notification);
            if model.do_not_disturb {
                None
            } else {
                Some(
                    Task::done(Arc::new(
                        HostToastRequest::show(toast_id, width, height).to_host_request_string(),
                    ) as PluginMsg)
                    .chain(Task::perform(
                        async move {
                            std::thread::sleep(timeout);
                            (id, generation)
                        },
                        |(id, generation)| oxinoti::msg(Event::ToastExpired(id, generation)),
                    )),
                )
            }
        }
        Event::Remove(id) => {
            model.remove(id);
            Some(close_toast_task(id))
        }
        Event::Close(id) => {
            model.remove(id);
            oxinoti::spawn_close_notification(id);
            Some(close_toast_task(id))
        }
        Event::Invoke(id, action) => {
            model.remove(id);
            oxinoti::spawn_invoke_action(id, action);
            Some(close_toast_task(id))
        }
        Event::ReplyChanged(id, text) => {
            if text.trim().is_empty() {
                model.reply_texts.remove(&id);
            } else {
                model.reply_texts.insert(id, text);
            }
            model.close_expired_toast_if_idle(id)
        }
        Event::SubmitReply(id) => {
            let can_reply = model
                .notifications
                .iter()
                .any(|notification| notification.id() == id && notification.allows_inline_reply());
            if !can_reply {
                model.reply_texts.remove(&id);
                None
            } else {
                let text = model.reply_texts.remove(&id).unwrap_or_default();
                let text = text.trim().to_owned();
                if text.is_empty() {
                    model.close_expired_toast_if_idle(id)
                } else {
                    model.remove(id);
                    oxinoti::spawn_inline_reply(id, text);
                    Some(close_toast_task(id))
                }
            }
        }
        Event::HoverChanged(id, hovered) => {
            if hovered {
                model.hovered_notifications.insert(id);
                None
            } else {
                model.hovered_notifications.remove(&id);
                model.close_expired_toast_if_idle(id)
            }
        }
        Event::ToastExpired(id, generation) => {
            if model.toast_generations.get(&id).copied() != Some(generation) {
                None
            } else if model.is_toast_active(id) {
                model.expired_toasts.insert(id);
                Some(recheck_expired_toast_task(id, generation))
            } else {
                model.expired_toasts.remove(&id);
                model.toast_generations.remove(&id);
                Some(close_toast_task(id))
            }
        }
        Event::Error(error) => {
            model.errors.push(error);
            None
        }
    })
    .flatten()
}

fn close_toast_task(id: u32) -> Task<PluginMsg> {
    Task::done(
        Arc::new(HostToastRequest::close(id.to_string()).to_host_request_string()) as PluginMsg,
    )
}

fn recheck_expired_toast_task(id: u32, generation: u64) -> Task<PluginMsg> {
    Task::perform(
        async move {
            std::thread::sleep(Duration::from_millis(TOAST_ACTIVE_RECHECK_MILLIS));
            (id, generation)
        },
        |(id, generation)| oxinoti::msg(Event::ToastExpired(id, generation)),
    )
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_focused_index: usize, _model: PluginModel) -> Option<Task<PluginMsg>> {
    Some(Task::done(oxinoti::msg(Event::TogglePanel)))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let count = model.notifications.len();
        vec![oxinoti::bar_button(count).into()]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn panel_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        vec![oxinoti::panel_view(
            &model.notifications,
            &model.reply_texts,
            &model.hovered_notifications,
            model.do_not_disturb,
        )]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn toast_view(
    model: PluginModel,
    toast_id: &str,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        oxinoti::toast_view(
            &model.notifications,
            &model.reply_texts,
            &model.hovered_notifications,
            toast_id,
        )
        .into_iter()
        .collect()
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let s = stream::channel(
        64,
        move |output: iced::futures::channel::mpsc::Sender<PluginMsg>| async move {
            let output = Arc::new(Mutex::new(output));
            std::thread::spawn(move || oxinoti::run_server(output));
            std::future::pending::<()>().await;
        },
    );
    Box::into_raw(Box::new(s)) as *mut PluginStream
}

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::fdo::RequestNameReply;

    fn notification(id: u32, summary: &str) -> Notification {
        Notification::new_for_test(id, summary)
    }

    #[test]
    fn strip_markup_removes_simple_tags_and_entities() {
        assert_eq!(oxinoti::clean_markup("<b>Hello</b> world"), "Hello world");
        assert_eq!(oxinoti::clean_markup("plain"), "plain");
        assert_eq!(oxinoti::clean_markup("Tom &amp; Jerry"), "Tom & Jerry");
    }

    #[test]
    fn request_name_reply_reports_conflicting_notification_daemon() {
        assert!(oxinoti::request_name_error(RequestNameReply::PrimaryOwner).is_none());
        assert!(oxinoti::request_name_error(RequestNameReply::AlreadyOwner).is_none());

        let exists_error = oxinoti::request_name_error(RequestNameReply::Exists).unwrap();
        assert!(exists_error.contains("another notification daemon"));
        assert!(oxinoti::request_name_error(RequestNameReply::InQueue).is_some());
    }

    #[test]
    fn notification_primary_action_prefers_default_then_first_non_reply() {
        let mut default_action = notification(1, "default");
        default_action.actions = vec!["default".to_owned(), "Open".to_owned()];
        assert_eq!(default_action.primary_action().as_deref(), Some("default"));
        assert!(!default_action.allows_inline_reply());

        let mut named_action = notification(2, "named");
        named_action.actions = vec!["open".to_owned(), "Open".to_owned()];
        assert_eq!(named_action.primary_action().as_deref(), Some("open"));

        let mut reply_action = notification(3, "reply");
        reply_action.actions = vec!["inline-reply".to_owned(), "Reply".to_owned()];
        assert_eq!(reply_action.primary_action(), None);
        assert!(reply_action.allows_inline_reply());
    }

    #[test]
    fn update_adds_replaces_removes_and_clears_notifications() {
        let (plugin_model, init_task) = model(Table::new());
        assert!(init_task.is_none());
        assert_eq!(name(), "Notifications");
        assert_eq!(abi_version(), ABI_VERSION);

        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::Add(Box::new(notification(1, "first")))),
        );
        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::Add(Box::new(notification(1, "replaced")))),
        );
        {
            let guard = plugin_model.read().unwrap();
            let model = guard.downcast_ref::<Model>().unwrap();
            assert_eq!(model.notifications.len(), 1);
            assert_eq!(model.notifications[0].summary, "replaced");
        }

        let _ = update(plugin_model.clone(), oxinoti::msg(Event::Remove(1)));
        {
            let guard = plugin_model.read().unwrap();
            let model = guard.downcast_ref::<Model>().unwrap();
            assert!(model.notifications.is_empty());
        }

        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::Add(Box::new(notification(2, "second")))),
        );
        let _ = update(plugin_model.clone(), oxinoti::msg(Event::ClearAll));
        let guard = plugin_model.read().unwrap();
        let model = guard.downcast_ref::<Model>().unwrap();
        assert!(model.notifications.is_empty());
    }

    #[test]
    fn dnd_suppresses_toast_task_but_keeps_notification() {
        let (plugin_model, _) = model(Table::new());
        assert!(
            update(
                plugin_model.clone(),
                oxinoti::msg(Event::ToggleDoNotDisturb)
            )
            .is_some()
        );

        let task = update(
            plugin_model.clone(),
            oxinoti::msg(Event::Add(Box::new(notification(7, "quiet")))),
        );

        assert!(task.is_none());
        let guard = plugin_model.read().unwrap();
        let model = guard.downcast_ref::<Model>().unwrap();
        assert_eq!(model.notifications.len(), 1);
        assert!(model.do_not_disturb);
    }

    #[test]
    fn reads_notification_timeout_from_config() {
        let mut notifications = Table::new();
        notifications.insert(
            "timeout".to_owned(),
            oxibar_plugin_api::toml::Value::Integer(7),
        );
        let mut global = Table::new();
        global.insert(
            "notifications".to_owned(),
            oxibar_plugin_api::toml::Value::Table(notifications),
        );

        assert_eq!(oxinoti::read_timeout(&global), Duration::from_secs(7));
        assert_eq!(oxinoti::read_timeout(&Table::new()), Duration::from_secs(3));
    }

    #[test]
    fn hovered_notification_does_not_expire_toast_generation() {
        let (plugin_model, _) = model(Table::new());
        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::Add(Box::new(notification(9, "hovered")))),
        );
        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::HoverChanged(9, true)),
        );

        let task = update(
            plugin_model.clone(),
            oxinoti::msg(Event::ToastExpired(9, 1)),
        );

        assert!(task.is_some());
        let guard = plugin_model.read().unwrap();
        let model = guard.downcast_ref::<Model>().unwrap();
        assert_eq!(model.toast_generations.get(&9), Some(&1));
        assert!(model.expired_toasts.contains(&9));
        drop(guard);

        let task = update(
            plugin_model.clone(),
            oxinoti::msg(Event::HoverChanged(9, false)),
        );

        assert!(task.is_some());
        let guard = plugin_model.read().unwrap();
        let model = guard.downcast_ref::<Model>().unwrap();
        assert!(!model.toast_generations.contains_key(&9));
        assert!(!model.expired_toasts.contains(&9));
        assert!(
            model
                .notifications
                .iter()
                .any(|notification| notification.id() == 9)
        );
    }

    #[test]
    fn inline_reply_notification_expires_when_idle() {
        let (plugin_model, _) = model(Table::new());
        let mut notification = notification(10, "reply");
        notification.actions = vec!["inline-reply".to_owned(), "Reply".to_owned()];
        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::Add(Box::new(notification))),
        );

        let task = update(
            plugin_model.clone(),
            oxinoti::msg(Event::ToastExpired(10, 1)),
        );

        assert!(task.is_some());
        let guard = plugin_model.read().unwrap();
        let model = guard.downcast_ref::<Model>().unwrap();
        assert!(!model.toast_generations.contains_key(&10));
        assert!(!model.expired_toasts.contains(&10));
        assert!(
            model
                .notifications
                .iter()
                .any(|notification| notification.id() == 10)
        );
    }

    #[test]
    fn reply_draft_defers_expiry_until_cleared() {
        let (plugin_model, _) = model(Table::new());
        let mut notification = notification(11, "draft");
        notification.actions = vec!["inline-reply".to_owned(), "Reply".to_owned()];
        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::Add(Box::new(notification))),
        );
        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::ReplyChanged(11, "hello".to_owned())),
        );

        let task = update(
            plugin_model.clone(),
            oxinoti::msg(Event::ToastExpired(11, 1)),
        );

        assert!(task.is_some());
        {
            let guard = plugin_model.read().unwrap();
            let model = guard.downcast_ref::<Model>().unwrap();
            assert_eq!(model.toast_generations.get(&11), Some(&1));
            assert!(model.expired_toasts.contains(&11));
        }

        let task = update(
            plugin_model.clone(),
            oxinoti::msg(Event::ReplyChanged(11, String::new())),
        );

        assert!(task.is_some());
        let guard = plugin_model.read().unwrap();
        let model = guard.downcast_ref::<Model>().unwrap();
        assert!(!model.toast_generations.contains_key(&11));
        assert!(!model.expired_toasts.contains(&11));
        assert!(
            model
                .notifications
                .iter()
                .any(|notification| notification.id() == 11)
        );
    }

    #[test]
    fn focused_reply_defers_expiry_until_focus_clears() {
        let (plugin_model, _) = model(Table::new());
        let mut notification = notification(12, "focused");
        notification.actions = vec!["inline-reply".to_owned(), "Reply".to_owned()];
        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::Add(Box::new(notification))),
        );
        oxinoti::set_reply_focus(12, true);

        let task = update(
            plugin_model.clone(),
            oxinoti::msg(Event::ToastExpired(12, 1)),
        );

        assert!(task.is_some());
        {
            let guard = plugin_model.read().unwrap();
            let model = guard.downcast_ref::<Model>().unwrap();
            assert_eq!(model.toast_generations.get(&12), Some(&1));
            assert!(model.expired_toasts.contains(&12));
        }

        oxinoti::set_reply_focus(12, false);
        let task = update(
            plugin_model.clone(),
            oxinoti::msg(Event::ToastExpired(12, 1)),
        );

        assert!(task.is_some());
        let guard = plugin_model.read().unwrap();
        let model = guard.downcast_ref::<Model>().unwrap();
        assert!(!model.toast_generations.contains_key(&12));
        assert!(!model.expired_toasts.contains(&12));
        assert!(
            model
                .notifications
                .iter()
                .any(|notification| notification.id() == 12)
        );
    }

    #[test]
    fn views_and_error_drain_are_deterministic() {
        let (plugin_model, _) = model(Table::new());
        assert_eq!(view(plugin_model.clone()).unwrap().len(), 1);
        assert_eq!(panel_view(plugin_model.clone()).unwrap().len(), 1);

        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::ToggleDoNotDisturb),
        );
        {
            let guard = plugin_model.read().unwrap();
            let model = guard.downcast_ref::<Model>().unwrap();
            assert!(model.do_not_disturb);
        }

        let _ = update(
            plugin_model.clone(),
            oxinoti::msg(Event::Error("dbus failed".to_owned())),
        );
        assert_eq!(errors(plugin_model.clone()), vec!["dbus failed"]);
        assert!(errors(plugin_model).is_empty());
    }
}

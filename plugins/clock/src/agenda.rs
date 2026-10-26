use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{NaiveDate, NaiveTime};
use iced::{
    Element, Length, Task,
    widget::{Column, Row},
};
use oxibar_plugin_api::PluginMsg;
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::oxi_plugin::{self, text},
};
use serde::{Deserialize, Serialize};

use super::{Message, Model, msg};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Event {
    id: String,
    date: String,
    title: String,
    time: String,
    location: String,
    description: String,
}
impl Event {
    pub fn calendar_event(&self) -> super::CalendarEvent {
        super::CalendarEvent {
            date: NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").expect("validated event date"),
            summary: if self.time.is_empty() {
                self.title.clone()
            } else {
                format!("{} · {}", self.time, self.title)
            },
            description: (!self.description.is_empty()).then(|| self.description.clone()),
            location: (!self.location.is_empty()).then(|| self.location.clone()),
        }
    }
    pub fn upcoming(&self, now: chrono::NaiveDateTime) -> bool {
        let date = NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").expect("validated event date");
        date > now.date()
            || date == now.date()
                && (self.time.is_empty()
                    || NaiveTime::parse_from_str(&self.time, "%H:%M")
                        .is_ok_and(|time| time >= now.time()))
    }
    fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 128
            || self.title.trim().is_empty()
            || self.title.len() > 512
        {
            return Err("Enter an event title (up to 512 bytes).".into());
        }
        NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").map_err(|_| "Use date YYYY-MM-DD.")?;
        if !self.time.is_empty() {
            NaiveTime::parse_from_str(&self.time, "%H:%M")
                .map_err(|_| "Use time HH:MM, or leave blank for an all-day event.")?;
        }
        if self.location.len() > 2048 || self.description.len() > 16_384 {
            return Err("Event details are too long.".into());
        }
        Ok(())
    }
}
#[derive(Debug, Default)]
pub struct Agenda {
    pub events: Vec<Event>,
    day: Option<NaiveDate>,
    draft: Option<Event>,
    delete_confirmation: Option<String>,
    loaded: bool,
    pending: bool,
    error: Option<String>,
}
#[derive(Debug, Clone)]
pub enum Action {
    Loaded(Result<Vec<Event>, String>),
    New,
    Edit(String),
    CancelEdit,
    Title(String),
    Date(String),
    Time(String),
    Location(String),
    Description(String),
    Save,
    Delete(String),
    ConfirmDelete,
    Saved(Result<Vec<Event>, String>),
    Close,
    External,
}
fn action(action: Action) -> PluginMsg {
    msg(Message::Agenda(action))
}
pub fn load_task() -> Task<PluginMsg> {
    Task::perform(
        oxibar_plugin_api::worker::run(|| load(&storage_path()?)),
        |result| action(Action::Loaded(result)),
    )
}
pub fn open(model: &mut Model, day: NaiveDate) -> Task<PluginMsg> {
    model.agenda.day = Some(day);
    model.agenda.draft = None;
    model.agenda.delete_confirmation = None;
    Task::done(
        std::sync::Arc::new(oxibar_plugin_api::HOST_REQUEST_OPEN_MODAL.to_owned()) as PluginMsg,
    )
}
pub fn closed(agenda: &mut Agenda) {
    agenda.day = None;
    agenda.draft = None;
    agenda.delete_confirmation = None;
}
pub fn update(model: &mut Model, event: Action) -> Option<Task<PluginMsg>> {
    let agenda = &mut model.agenda;
    match event {
        Action::Loaded(result) => match result {
            Ok(events) => {
                agenda.events = events;
                agenda.loaded = true;
                agenda.error = None;
            }
            Err(error) => {
                agenda.error = Some(error.clone());
                model.errors.push(error);
            }
        },
        Action::New if !agenda.pending && agenda.loaded => {
            let day = agenda.day?;
            agenda.draft = Some(Event {
                id: new_id(),
                date: day.to_string(),
                title: String::new(),
                time: String::new(),
                location: String::new(),
                description: String::new(),
            });
            agenda.delete_confirmation = None;
            agenda.error = None;
        }
        Action::Edit(id) if !agenda.pending => {
            agenda.draft = agenda.events.iter().find(|event| event.id == id).cloned();
            agenda.delete_confirmation = None;
            agenda.error = None;
        }
        Action::CancelEdit if !agenda.pending => {
            agenda.draft = None;
            agenda.delete_confirmation = None;
            agenda.error = None;
        }
        Action::Title(value) if !agenda.pending => {
            if let Some(draft) = &mut agenda.draft {
                draft.title = value;
            }
        }
        Action::Date(value) if !agenda.pending => {
            if let Some(draft) = &mut agenda.draft {
                draft.date = value;
            }
        }
        Action::Time(value) if !agenda.pending => {
            if let Some(draft) = &mut agenda.draft {
                draft.time = value;
            }
        }
        Action::Location(value) if !agenda.pending => {
            if let Some(draft) = &mut agenda.draft {
                draft.location = value;
            }
        }
        Action::Description(value) if !agenda.pending => {
            if let Some(draft) = &mut agenda.draft {
                draft.description = value;
            }
        }
        Action::Save if !agenda.pending && agenda.loaded => {
            let mut event = agenda.draft.clone()?;
            event.title = event.title.trim().to_owned();
            event.time = event.time.trim().to_owned();
            event.date = event.date.trim().to_owned();
            if let Err(error) = event.validate() {
                agenda.error = Some(error);
                return None;
            }
            let mut events = agenda.events.clone();
            events.retain(|existing| existing.id != event.id);
            events.push(event);
            return save_task(agenda, events);
        }
        Action::Delete(id) if !agenda.pending && agenda.loaded => {
            if agenda.events.iter().any(|event| event.id == id) {
                agenda.delete_confirmation = Some(id);
            }
        }
        Action::ConfirmDelete if !agenda.pending && agenda.loaded => {
            let id = agenda.delete_confirmation.as_ref()?;
            let events = agenda
                .events
                .iter()
                .filter(|event| event.id != *id)
                .cloned()
                .collect();
            return save_task(agenda, events);
        }
        Action::Saved(result) => {
            agenda.pending = false;
            match result {
                Ok(events) => {
                    agenda.events = events;
                    agenda.draft = None;
                    agenda.delete_confirmation = None;
                    agenda.error = None;
                }
                Err(error) => {
                    agenda.error = Some(error.clone());
                    model.errors.push(error);
                }
            }
        }
        Action::Close => {
            closed(agenda);
            return Some(Task::done(std::sync::Arc::new(
                oxibar_plugin_api::HOST_REQUEST_CLOSE_MODAL.to_owned(),
            ) as PluginMsg));
        }
        Action::External => {
            let day = agenda.day?;
            let command = model.calendar_command.clone();
            return Some(Task::perform(
                super::worker::run(move || super::open_calendar_day(day, command)),
                |result| msg(Message::CalendarOpenResult(result)),
            ));
        }
        _ => {}
    }
    None
}
fn save_task(agenda: &mut Agenda, mut events: Vec<Event>) -> Option<Task<PluginMsg>> {
    events.sort_by(|a, b| (&a.date, &a.time, &a.title).cmp(&(&b.date, &b.time, &b.title)));
    let expected = agenda.events.clone();
    agenda.pending = true;
    agenda.error = None;
    Some(Task::perform(
        oxibar_plugin_api::worker::run(move || {
            save(&storage_path()?, &expected, &events)?;
            Ok(events)
        }),
        |result| action(Action::Saved(result)),
    ))
}
pub fn view(model: &Model) -> Element<'static, PluginMsg> {
    let agenda = &model.agenda;
    let day = agenda.day.unwrap_or_else(|| model.now.date_naive());
    let mut header = Row::new()
        .spacing(OXITHEME.padding_md)
        .align_y(iced::Alignment::Center)
        .push(
            text(day.format("%A, %B %-d, %Y").to_string())
                .size(OXITHEME.font_xl)
                .width(Length::Fill),
        );
    if model.calendar_command.is_some() {
        header = header.push(button("Open external").on_press(action(Action::External)));
    }
    header = header.push(button("Close").on_press(action(Action::Close)));
    let mut content = Column::new()
        .spacing(OXITHEME.padding_lg)
        .width(Length::Fill);
    if let Some(draft) = &agenda.draft {
        content = content
            .push(text("Event details").size(OXITHEME.font_lg))
            .push(field("Title", &draft.title, Action::Title, agenda.pending))
            .push(
                Row::new()
                    .spacing(OXITHEME.padding_md)
                    .push(field(
                        "Date (YYYY-MM-DD)",
                        &draft.date,
                        Action::Date,
                        agenda.pending,
                    ))
                    .push(field(
                        "Time (HH:MM, optional)",
                        &draft.time,
                        Action::Time,
                        agenda.pending,
                    )),
            )
            .push(field(
                "Location",
                &draft.location,
                Action::Location,
                agenda.pending,
            ))
            .push(field(
                "Description",
                &draft.description,
                Action::Description,
                agenda.pending,
            ));
        let mut save = button("Save event");
        let mut cancel = button("Cancel");
        if !agenda.pending {
            save = save.on_press(action(Action::Save));
            cancel = cancel.on_press(action(Action::CancelEdit));
        }
        content = content.push(
            Row::new()
                .spacing(OXITHEME.padding_md)
                .push(save)
                .push(cancel),
        );
    } else {
        let mut add = button("Add event");
        if agenda.loaded && !agenda.pending {
            add = add.on_press(action(Action::New));
        }
        content = content.push(
            Row::new()
                .spacing(OXITHEME.padding_md)
                .push(text("Events").size(OXITHEME.font_lg).width(Length::Fill))
                .push(add),
        );
        let local = agenda
            .events
            .iter()
            .filter(|event| event.date == day.to_string())
            .collect::<Vec<_>>();
        let remote = model
            .calendar_events
            .iter()
            .filter(|event| event.date == day)
            .collect::<Vec<_>>();
        if local.is_empty() && remote.is_empty() {
            content = content.push(text("No events for this day.").style(oxi_plugin::text_muted));
        }
        for event in local {
            let mut edit = button("Edit");
            let mut delete = button("Delete");
            if !agenda.pending {
                edit = edit.on_press(action(Action::Edit(event.id.clone())));
                delete = delete.on_press(action(Action::Delete(event.id.clone())));
            }
            content = content.push(oxi_plugin::settings_card(
                Column::new()
                    .spacing(OXITHEME.padding_md)
                    .push(event_details(&event.calendar_event(), "Local calendar"))
                    .push(
                        Row::new()
                            .spacing(OXITHEME.padding_sm)
                            .push(edit)
                            .push(delete),
                    ),
            ));
        }
        for event in remote {
            content = content.push(oxi_plugin::settings_card(event_details(
                event,
                "CalDAV · read-only",
            )));
        }
        if agenda.delete_confirmation.is_some() {
            let mut delete = button("Delete event");
            let mut cancel = button("Cancel");
            if !agenda.pending {
                delete = delete.on_press(action(Action::ConfirmDelete));
                cancel = cancel.on_press(action(Action::CancelEdit));
            }
            content = content
                .push(text("Delete this event from the local calendar?"))
                .push(
                    Row::new()
                        .spacing(OXITHEME.padding_md)
                        .push(delete)
                        .push(cancel),
                );
        }
    }
    if agenda.pending {
        content = content.push(text("Saving…").style(oxi_plugin::text_muted));
    }
    if !agenda.loaded && agenda.error.is_none() {
        content = content.push(text("Loading local calendar…").style(oxi_plugin::text_muted));
    }
    if let Some(error) = &agenda.error {
        content = content.push(text(error.clone()).style(oxi_plugin::text_muted));
    }
    if let Some(error) = &model.calendar_sync_error {
        content = content.push(text(format!("CalDAV: {error}")).style(oxi_plugin::text_muted));
    }
    oxi_plugin::settings_card(
        Column::new()
            .spacing(OXITHEME.padding_xl)
            .push(header)
            .push(iced::widget::scrollable(content).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .into()
}
fn button(label: &'static str) -> oxiced::widgets::oxi_control::Button<'static, PluginMsg> {
    oxi_plugin::surface_button(text(label), false)
}
fn field(
    label: &str,
    value: &str,
    change: fn(String) -> Action,
    disabled: bool,
) -> Element<'static, PluginMsg> {
    let mut input = iced::widget::text_input(label, value)
        .style(oxiced::widgets::oxi_text_input::text_input_style)
        .padding(OXITHEME.padding_sm)
        .size(OXITHEME.font_md);
    if !disabled {
        input = input.on_input(move |value| action(change(value)));
    }
    Column::new()
        .spacing(OXITHEME.padding_sm)
        .push(text(label.to_owned()).size(OXITHEME.font_sm))
        .push(input)
        .width(Length::Fill)
        .into()
}
fn event_details(event: &super::CalendarEvent, source: &str) -> Element<'static, PluginMsg> {
    let mut column = Column::new()
        .spacing(OXITHEME.padding_md)
        .push(text(event.summary.clone()).size(OXITHEME.font_lg))
        .push(
            text(source.to_owned())
                .size(OXITHEME.font_sm)
                .style(oxi_plugin::text_muted),
        );
    if let Some(location) = &event.location {
        column = column.push(text(location.clone()));
    }
    if let Some(description) = &event.description {
        column = column.push(text(description.clone()));
    }
    column.into()
}
fn storage_path() -> Result<PathBuf, String> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".local/state"))
        })
        .ok_or("Cannot find local calendar state directory.")?;
    Ok(root.join("oxibar/calendar/events.json"))
}
fn new_id() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:x}-{:x}-{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
fn load(path: &Path) -> Result<Vec<Event>, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Cannot read local calendar: {error}")),
    };
    if metadata.len() > 4 * 1024 * 1024 {
        return Err("Local calendar exceeds 4 MiB.".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Local calendar exceeds 4 MiB.".into());
    }
    let events: Vec<Event> = serde_json::from_slice(&bytes).map_err(|error| {
        format!("Cannot parse local calendar; original file preserved: {error}")
    })?;
    validate_events(&events)?;
    Ok(events)
}
fn validate_events(events: &[Event]) -> Result<(), String> {
    if events.len() > 10_000 {
        return Err("Local calendar has too many events.".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for event in events {
        event.validate()?;
        if !ids.insert(&event.id) {
            return Err("Local calendar contains duplicate event identifiers.".into());
        }
    }
    Ok(())
}
fn save(path: &Path, expected: &[Event], events: &[Event]) -> Result<(), String> {
    validate_events(events)?;
    if load(path)? != expected {
        return Err(
            "Local calendar changed on disk. Reopen the application before saving again.".into(),
        );
    }
    let parent = path.parent().ok_or("Invalid local calendar path")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(events).map_err(|error| error.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Local calendar exceeds 4 MiB.".into());
    }
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    file.write_all(&bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|error| error.to_string())?;
    file.persist(path).map_err(|error| error.to_string())?;
    // A completed rename is committed even if this filesystem cannot sync directories.
    let _ = fs::File::open(parent).and_then(|directory| directory.sync_all());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn next_event_skips_elapsed_times_and_closing_does_not_cancel_committed_results() {
        let mut event = Event {
            id: new_id(),
            date: "2026-10-20".into(),
            title: "Planning".into(),
            time: "14:30".into(),
            location: String::new(),
            description: String::new(),
        };
        let now = NaiveDate::from_ymd_opt(2026, 10, 20)
            .unwrap()
            .and_hms_opt(15, 0, 0)
            .unwrap();
        assert!(!event.upcoming(now));
        event.time.clear();
        assert!(event.upcoming(now));
        event.date = "2026-10-21".into();
        assert!(event.upcoming(now));
        let (model, _) = crate::model(oxibar_plugin_api::toml::Table::new());
        oxibar_plugin_api::with_model_write::<Model, _>(&model, |model| {
            model.agenda.pending = true;
            model.agenda.day = Some(now.date());
            model.agenda.draft = Some(event.clone());
            closed(&mut model.agenda);
            assert!(model.agenda.pending);
            update(model, Action::Saved(Ok(vec![event.clone()])));
            assert_eq!(model.agenda.events, [event]);
            assert!(model.agenda.day.is_none());
            assert!(model.agenda.draft.is_none());
            assert!(!model.agenda.pending);
        });
    }
    #[test]
    fn local_events_round_trip_and_refuse_corrupt_or_changed_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("calendar/events.json");
        let event = Event {
            id: new_id(),
            date: "2026-10-20".into(),
            title: "Planning".into(),
            time: "14:30".into(),
            location: "Office".into(),
            description: "Agenda".into(),
        };
        let events = vec![event.clone()];
        save(&path, &[], &events).unwrap();
        assert_eq!(load(&path).unwrap(), events);
        assert!(save(&path, &[], &[]).is_err());
        let mut invalid = event;
        invalid.time = "25:90".into();
        assert!(save(&path, &events, &[invalid]).is_err());
        fs::write(&path, b"broken").unwrap();
        assert!(load(&path).is_err());
        assert!(save(&path, &events, &[]).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken");
    }
}

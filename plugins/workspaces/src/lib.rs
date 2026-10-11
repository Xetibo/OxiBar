//! Workspace controls consume the host's single compositor event broker.
use iced::{Element, Task, widget::Row};
use oxibar_plugin_api::{
    ABI_VERSION, CompositorSnapshot, HOST_REQUEST_WORKSPACE_PREFIX, PluginModel, PluginMsg,
    PluginStream, PluginViewContext, drain_model_errors, plugin_model, toml::Table,
    with_model_read, with_model_write,
};
use oxiced::widgets::oxi_plugin::text;
use oxiced::{theme::theme_impl::OXITHEME, widgets::oxi_plugin};
use std::sync::Arc;

#[derive(Debug, Default)]
struct Model {
    snapshot: CompositorSnapshot,
    errors: Vec<String>,
}
#[derive(Debug, Clone)]
enum Message {
    Activate(i64),
}

oxibar_plugin_api::export_theme_changed!();

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}
#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Workspaces"
}
#[unsafe(no_mangle)]
pub extern "Rust" fn model(_: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    (plugin_model(Model::default()), None)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, message: PluginMsg) -> Option<Task<PluginMsg>> {
    let Message::Activate(id) = message.downcast_ref::<Message>()?;
    if *id <= 0 {
        return None;
    }
    with_model_read::<Model, _>(&model, |model| {
        model
            .snapshot
            .workspaces
            .iter()
            .any(|workspace| workspace.id == *id)
    })
    .ok()
    .filter(|valid| *valid)?;
    Some(Task::done(
        Arc::new(format!("{HOST_REQUEST_WORKSPACE_PREFIX}{id}")) as PluginMsg,
    ))
}
#[unsafe(no_mangle)]
pub extern "Rust" fn launch(index: usize, model: PluginModel) -> Option<Task<PluginMsg>> {
    let id = with_model_read::<Model, _>(&model, |model| {
        let mut ids: Vec<_> = model
            .snapshot
            .workspaces
            .iter()
            .map(|workspace| workspace.id)
            .filter(|id| *id > 0)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.get(index).copied()
    })
    .ok()
    .flatten()?;
    update(model, Arc::new(Message::Activate(id)))
}
#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    Box::into_raw(Box::new(Box::pin(iced::futures::stream::pending::<
        PluginMsg,
    >()))) as *mut PluginStream
}
#[unsafe(no_mangle)]
pub extern "Rust" fn compositor_changed(
    model: PluginModel,
    snapshot: CompositorSnapshot,
) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |model| model.snapshot = snapshot);
    None
}
#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    render(model, None)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn view_with_context(
    model: PluginModel,
    context: PluginViewContext,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    render(model, context.output_name.as_deref())
}

fn render(
    model: PluginModel,
    output: Option<&str>,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let mut row = Row::new().spacing(OXITHEME.padding_xs);
        for workspace in model
            .snapshot
            .workspaces
            .iter()
            .filter(|workspace| output.is_none_or(|output| workspace.output == output))
        {
            let active =
                model.snapshot.active_workspaces.get(&workspace.output) == Some(&workspace.id);
            row = row.push(
                oxi_plugin::bar_button(text(workspace.name.clone()).size(OXITHEME.font_md))
                    .style(oxi_plugin::selectable_row_style(active))
                    .on_press(Arc::new(Message::Activate(workspace.id)) as PluginMsg),
            );
        }
        vec![row.into()]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_view_is_quiet_and_empty() {
        let (model, task) = model(Table::new());
        assert!(task.is_none());
        assert!(view(model).is_ok());
    }
    #[test]
    fn host_snapshot_is_shared_and_unknown_workspace_is_rejected() {
        let (model, _) = model(Table::new());
        let mut snapshot = CompositorSnapshot::default();
        snapshot
            .workspaces
            .push(oxibar_plugin_api::CompositorWorkspace {
                id: 2,
                name: "2".into(),
                output: "DP-1".into(),
                windows: 1,
            });
        compositor_changed(model.clone(), snapshot);
        assert!(launch(0, model.clone()).is_some());
        assert!(launch(1, model.clone()).is_none());
        assert!(update(model.clone(), Arc::new(Message::Activate(99))).is_none());
        assert!(update(model, Arc::new(Message::Activate(2))).is_some());
    }
}

use iced::Task;
use oxibar_plugin_api::CommandOutcome;

use crate::{
    app::OxiBar,
    ipc,
    messages::{Message, map_plugin_message},
};

impl OxiBar {
    pub(crate) fn run_command(
        &mut self,
        plugin: String,
        command: String,
        arguments: Vec<String>,
        reply: ipc::Reply,
    ) -> Task<Message> {
        let result = (|| {
            let plugin = self
                .plugins
                .keys()
                .find(|name| name.eq_ignore_ascii_case(&plugin))
                .cloned()
                .ok_or("Unknown action provider")?;
            let (model, funcs) = &self.plugins[&plugin];
            let handler = funcs.command.ok_or("Provider has no command capability")?;
            let task = unsafe { handler(model.clone(), command, arguments) }?;
            Ok::<_, String>((plugin, task))
        })();
        match result {
            Ok((plugin, task)) => {
                let output = self.overlay_output();
                task.map(move |outcome| Message::CommandFinished {
                    plugin: plugin.clone(),
                    outcome,
                    reply: reply.clone(),
                    output: output.clone(),
                })
            }
            Err(error) => {
                reply.send(ipc::response(
                    self.config.snapshot.revision,
                    self.status_data(),
                    Some(&error),
                ));
                Task::none()
            }
        }
    }

    pub(crate) fn finish_command(
        &mut self,
        plugin: String,
        outcome: CommandOutcome,
        reply: ipc::Reply,
        output: Option<String>,
    ) -> Task<Message> {
        let task = self.update(Message::OnCommandOutput(
            output,
            Box::new(map_plugin_message(plugin, outcome.message)),
        ));
        reply.send(ipc::response(
            self.config.snapshot.revision,
            self.status_data(),
            outcome.result.as_ref().err().map(String::as_str),
        ));
        task
    }
}

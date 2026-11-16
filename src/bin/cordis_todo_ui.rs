//! GPUI view driven by a cordis-rs plugin.
//!
//! Mounts the reusable `dsh-tool-todo` port on a Cordis context, then renders
//! the `todos` projection in a GPUI window. UI buttons call `todo_write`
//! through the shared `tools` seam, so the visible state comes from the plugin
//! rather than from a local copy in this binary.

use std::sync::Arc;

use cordis_rs::plugin_ports::{MountedTodo, RawTodo, SessionEvent, TodoWriteArgs, ToolExecution};
use cordis_rs::{Context, Error, Result};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

struct TodoPanel {
    mounted: Arc<MountedTodo>,
    status: SharedString,
}

impl TodoPanel {
    fn new(mounted: MountedTodo) -> Self {
        Self {
            mounted: Arc::new(mounted),
            status: "Ready".into(),
        }
    }

    fn write(&mut self, content: &str, status: &str) {
        let args = TodoWriteArgs {
            todos: vec![RawTodo::new(content, status)],
        };
        let exec = ToolExecution::agent("gpui");
        let result = self.mounted.tools.invoke_blocking("todo_write", args, exec);
        self.status = match result {
            Ok(result) => SharedString::from(result.render()),
            Err(error) => SharedString::from(format!("Error: {error}")),
        };
    }

    fn clear(&mut self) {
        self.mounted.projections.append(SessionEvent::TurnStart);
        self.status = "Cleared (turn/start)".into();
    }

    fn todo_list(&self) -> Vec<(String, String)> {
        self.mounted
            .projections
            .todos_view()
            .unwrap_or_default()
            .into_iter()
            .map(|item| (item.status.as_str().to_string(), item.content))
            .collect()
    }
}

fn color_for(status: &str) -> gpui::Rgba {
    match status {
        "in_progress" => rgb(0x8ec5ff),
        "completed" => rgb(0x9ce0b0),
        _ => rgb(0x9098a8),
    }
}

impl Render for TodoPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let mut pane = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_3()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — tool-todo"));

        let items = self.todo_list();
        if items.is_empty() {
            pane = pane.child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x24242c))
                    .text_color(rgb(0x80808c))
                    .child("No todos yet. Use the buttons below to call todo_write."),
            );
        } else {
            for (status, content) in items {
                pane = pane.child(
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(rgb(0x24242c))
                        .child(
                            div()
                                .text_sm()
                                .text_color(color_for(&status))
                                .child(SharedString::from(status)),
                        )
                        .child(div().child(SharedString::from(content))),
                );
            }
        }

        pane.child(
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .id("add-todo")
                        .p_2()
                        .rounded_md()
                        .bg(rgb(0x2b3a4a))
                        .text_color(rgb(0xe8e8ef))
                        .child("+ prepare step")
                        .on_click(cx.listener(|panel, _event, _window, _cx| {
                            panel.write("Prepare workspace", "pending");
                        })),
                )
                .child(
                    div()
                        .id("start-todo")
                        .p_2()
                        .rounded_md()
                        .bg(rgb(0x2b3a4a))
                        .text_color(rgb(0xe8e8ef))
                        .child("start")
                        .on_click(cx.listener(|panel, _event, _window, _cx| {
                            panel.write("Prepare workspace", "in_progress");
                        })),
                )
                .child(
                    div()
                        .id("finish-todo")
                        .p_2()
                        .rounded_md()
                        .bg(rgb(0x2b3a4a))
                        .text_color(rgb(0xe8e8ef))
                        .child("finish")
                        .on_click(cx.listener(|panel, _event, _window, _cx| {
                            panel.write("Prepare workspace", "completed");
                        })),
                )
                .child(
                    div()
                        .id("clear-todos")
                        .p_2()
                        .rounded_md()
                        .bg(rgb(0x3a2b2b))
                        .text_color(rgb(0xe8e8ef))
                        .child("clear")
                        .on_click(cx.listener(|panel, _event, _window, _cx| {
                            panel.clear();
                        })),
                ),
        )
        .child(
            div()
                .mt_auto()
                .p_3()
                .rounded_md()
                .bg(rgb(0x1e1e24))
                .text_color(rgb(0x9ce0b0))
                .child(self.status.clone()),
        )
    }
}

fn main() -> Result<()> {
    // Mount the plugin before entering GPUI's event loop. The blocking calls
    // above never await, so no Tokio runtime is needed inside a click handler.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::Other(format!("failed to build runtime: {error}")))?;
    let mounted = runtime.block_on(async {
        let ctx = Context::root("gpui-todo-ui");
        cordis_rs::plugin_ports::mount(&ctx, false).await
    })?;

    if mounted.tools.names() != ["todo_write"] {
        return Err(Error::Other(format!(
            "unexpected tools: {:?}",
            mounted.tools.names()
        )));
    }

    eprintln!(
        "cordis_todo_ui: mounted `tool-todo`, tools = {:?}",
        mounted.tools.names()
    );
    Application::new().run(move |cx: &mut App| {
        match cx.open_window(Default::default(), |_window, cx| {
            cx.new(|_cx| TodoPanel::new(mounted.clone()))
        }) {
            Ok(_window) => eprintln!("cordis_todo_ui: GPUI window created"),
            Err(error) => eprintln!("cordis_todo_ui: failed to create window: {error}"),
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mounted_port_drives_todos_through_the_tools_seam() {
        let ctx = Context::root("gpui-todo-test");
        let mounted = cordis_rs::plugin_ports::mount(&ctx, false).await.unwrap();
        assert_eq!(mounted.tools.names(), ["todo_write"]);
        assert!(mounted.projections.is_registered("todos"));

        let result = mounted
            .tools
            .invoke(
                "todo_write",
                TodoWriteArgs {
                    todos: vec![RawTodo::new("Prepare workspace", "in_progress")],
                },
                ToolExecution::agent("gpui"),
            )
            .await
            .unwrap();
        assert_eq!(result.counts.in_progress, 1);
        assert_eq!(
            result.render(),
            "Updated todo list: 0 pending, 1 in progress, 0 completed."
        );

        let view = mounted.projections.todos_view().unwrap();
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].content, "Prepare workspace");
        assert_eq!(
            view[0].status,
            cordis_rs::plugin_ports::TodoStatus::InProgress
        );

        mounted.projections.append(SessionEvent::TurnStart);
        assert!(mounted.projections.todos_view().is_none());
    }
}

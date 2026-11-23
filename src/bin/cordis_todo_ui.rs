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
    /// Last tool-call error, kept until the next successful operation so a
    /// later status update does not hide a failed write (swarm review round 283).
    error: Option<SharedString>,
}

impl TodoPanel {
    fn new(mounted: MountedTodo) -> Self {
        Self {
            mounted: Arc::new(mounted),
            status: "Ready".into(),
            error: None,
        }
    }

    fn write(&mut self, content: &str, status: &str) {
        let args = TodoWriteArgs {
            todos: vec![RawTodo::new(content, status)],
        };
        let exec = ToolExecution::agent("gpui");
        let result = self.mounted.tools.invoke_blocking("todo_write", args, exec);
        self.status = match result {
            Ok(result) => {
                self.error = None;
                SharedString::from(result.render())
            }
            Err(error) => {
                let line = SharedString::from(status_error_line(&error.to_string()));
                self.error = Some(line.clone());
                line
            }
        };
    }

    fn clear(&mut self) {
        self.mounted.projections.append(SessionEvent::TurnStart);
        self.status = "Cleared (turn/start)".into();
        self.error = None;
    }

    /// Rows for the todo pane. `todos_view() == None` with a registered
    /// projection is the normal "nothing written yet" empty state; an
    /// unregistered projection is a fault and is reported to `render`, which
    /// shows it instead of silently rendering an empty list.
    fn todo_list(&self) -> std::result::Result<Vec<(String, String)>, String> {
        if !self.mounted.projections.is_registered("todos") {
            return Err("todo projection `todos` is not registered".to_string());
        }
        Ok(self
            .mounted
            .projections
            .todos_view()
            .map_or_else(empty_todo_rows, |todos| {
                todos
                    .into_iter()
                    .map(|item| (item.status.as_str().to_string(), item.content))
                    .collect()
            }))
    }
}

/// The registered projection has received no `TodoWrite` yet: an empty pane,
/// not a fault. Named explicitly so a missing registration cannot be hidden
/// by a silent default conversion.
fn empty_todo_rows() -> Vec<(String, String)> {
    Vec::new()
}

/// Text shown when `todo_write` fails. Kept pure so the existing `Error: …`
/// status hook stays pinned by a regression test.
fn status_error_line(error: &str) -> String {
    format!("Error: {error}")
}

/// Text shown when the `todos` projection itself is unavailable. The old
/// `unwrap_or_default()` made this indistinguishable from an empty list.
fn projection_error_line(error: &str) -> String {
    format!("projection error: {error}")
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

        let (items, projection_error) = match self.todo_list() {
            Ok(items) => (items, None),
            Err(error) => (Vec::new(), Some(projection_error_line(&error))),
        };
        if let Some(error) = &projection_error {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xff6b6b))
                    .child(SharedString::from(error.clone())),
            );
        }
        if let Some(error) = &self.error {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xff6b6b))
                    .child(error.clone()),
            );
        }
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
        cordis_rs::plugin_ports::mount_tool_todo(&ctx, false).await
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
    use cordis_rs::plugin_ports::{SessionProjections, Tools};

    #[test]
    fn status_error_line_preserves_existing_error_hook() {
        assert_eq!(status_error_line("boom"), "Error: boom");
    }

    #[test]
    fn projection_error_line_is_distinct_from_the_empty_hint() {
        let message = projection_error_line("todo projection `todos` is not registered");
        assert_eq!(
            message,
            "projection error: todo projection `todos` is not registered"
        );
        assert_ne!(
            message,
            "No todos yet. Use the buttons below to call todo_write."
        );
    }

    #[test]
    fn unregistered_projection_is_reported_instead_of_looking_empty() {
        let panel = TodoPanel::new(MountedTodo {
            tools: Tools::default(),
            projections: SessionProjections::default(),
        });
        let error = panel
            .todo_list()
            .expect_err("an unregistered projection must be reported");
        assert_eq!(error, "todo projection `todos` is not registered");
        assert_eq!(
            projection_error_line(&error),
            "projection error: todo projection `todos` is not registered"
        );
    }

    #[tokio::test]
    async fn registered_projection_keeps_row_hook_and_empty_state() {
        let ctx = Context::root("gpui-todo-row-hook-test");
        let mounted = cordis_rs::plugin_ports::mount_tool_todo(&ctx, false)
            .await
            .expect("mount tool-todo");

        // A registered projection with no writes yet stays an empty list, not
        // an error: the existing "No todos yet" pane hook must survive.
        assert!(
            TodoPanel::new(mounted.clone())
                .todo_list()
                .expect("registered projection")
                .is_empty()
        );

        mounted
            .tools
            .invoke(
                "todo_write",
                TodoWriteArgs {
                    todos: vec![RawTodo::new("Prepare workspace", "in_progress")],
                },
                ToolExecution::agent("gpui"),
            )
            .await
            .expect("todo_write should succeed");

        assert_eq!(
            TodoPanel::new(mounted)
                .todo_list()
                .expect("registered projection"),
            vec![("in_progress".to_string(), "Prepare workspace".to_string())]
        );
    }

    #[tokio::test]
    async fn mounted_port_drives_todos_through_the_tools_seam() {
        let ctx = Context::root("gpui-todo-test");
        let mounted = cordis_rs::plugin_ports::mount_tool_todo(&ctx, false)
            .await
            .unwrap();
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

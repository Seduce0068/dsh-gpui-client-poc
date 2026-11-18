//! GPUI demo: enable/disable a cordis-rs plugin with a click.
//!
//! Mounts a plugin via `Fiber::mount` directly, renders its state, and toggles
//! it with a click. Unload removes the plugin's effect-scoped registrations;
//! remount restores them. Demonstrates reversible Cordis plugin registration.

use std::sync::{Arc, Mutex};

use cordis_rs::core_services::{provide_fs, provide_tools};
use cordis_rs::ports::tool_fs::ToolFs;
use cordis_rs::{Context, Fiber, Lifecycle};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

/// Stable GPUI element id for the toggle button (existing DOM hook).
const TOGGLE_ELEMENT_ID: &str = "toggle";

/// Status label format, kept pure so the existing DOM hook is regression-tested.
fn status_line(label: &str) -> String {
    format!("tool-fs: {label}")
}

fn unload_error_message(error: &str) -> String {
    format!("unload error: {error}")
}

fn mount_error_message(error: &str) -> String {
    format!("mount error: {error}")
}

struct ToggleState {
    fiber: Option<Fiber>,
    ctx: Context,
}

struct TogglePanel {
    state: Arc<Mutex<ToggleState>>,
    status: SharedString,
    /// Most recent unload/mount failure, rendered in the window instead of
    /// being silently discarded. Kept separate from `status` so `refresh()`
    /// cannot overwrite it.
    error: Option<SharedString>,
}

impl TogglePanel {
    fn refresh(&mut self) {
        let state = self.state.lock().expect("toggle state poisoned");
        let label = match state.fiber.as_ref().map(|fiber| fiber.state()) {
            Some(Lifecycle::Active) => "active",
            _ => "unloaded",
        };
        self.status = SharedString::from(status_line(label));
    }

    fn toggle(&mut self) {
        self.error = None;
        let error_message = {
            let mut state = self.state.lock().expect("toggle state poisoned");
            if let Some(fiber) = state.fiber.take() {
                futures::executor::block_on(fiber.unload())
                    .err()
                    .map(|error| unload_error_message(&error.to_string()))
            } else {
                match futures::executor::block_on(Fiber::mount(
                    &state.ctx,
                    Box::new(ToolFs {
                        config: cordis_rs::plugin_ports::ToolFsConfig::default(),
                    }),
                )) {
                    Ok(fiber) => {
                        state.fiber = Some(fiber);
                        None
                    }
                    Err(error) => Some(mount_error_message(&error.to_string())),
                }
            }
        };
        if let Some(message) = error_message {
            eprintln!("cordis_composition_toggle_ui: {message}");
            self.error = Some(SharedString::from(message));
        }
        self.refresh();
    }
}

impl Render for TogglePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let status = self.status.clone();
        let label = status.to_string();
        let mut pane = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_3()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — plugin toggle"));
        if let Some(error) = &self.error {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xff6b6b))
                    .child(error.clone()),
            );
        }
        pane.child(div().text_sm().child(status)).child(
            div()
                .id(TOGGLE_ELEMENT_ID)
                .p_3()
                .rounded_md()
                .bg(rgb(0x2b3a4a))
                .text_color(rgb(0xe8e8ef))
                .child(SharedString::from(format!("click to toggle ({label})")))
                .on_click(cx.listener(|panel, _event, _window, _cx| panel.toggle())),
        )
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::root("toggle-ui");
    let _ = provide_tools(&ctx).await?;
    let _ = provide_fs(&ctx).await?;
    let fiber = Fiber::mount(
        &ctx,
        Box::new(ToolFs {
            config: cordis_rs::plugin_ports::ToolFsConfig::default(),
        }),
    )
    .await?;
    assert_eq!(fiber.state(), Lifecycle::Active);

    let state = Arc::new(Mutex::new(ToggleState {
        fiber: Some(fiber),
        ctx: ctx.clone(),
    }));

    Application::new().run(move |cx: &mut App| {
        let mut panel = TogglePanel {
            state: state.clone(),
            status: SharedString::from(status_line("active")),
            error: None,
        };
        panel.refresh();
        let panel = cx.new(|_cx| panel);
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_composition_toggle_ui: GPUI window created");
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fiber_unload_and_remount_roundtrip() {
        let ctx = Context::root("toggle-test");
        let _ = provide_tools(&ctx).await.unwrap();
        let _ = provide_fs(&ctx).await.unwrap();
        let fiber = Fiber::mount(
            &ctx,
            Box::new(ToolFs {
                config: cordis_rs::plugin_ports::ToolFsConfig::default(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(fiber.state(), Lifecycle::Active);
        fiber.unload().await.unwrap();
        assert_eq!(fiber.state(), Lifecycle::Unloaded);
        let again = Fiber::mount(
            &ctx,
            Box::new(ToolFs {
                config: cordis_rs::plugin_ports::ToolFsConfig::default(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(again.state(), Lifecycle::Active);
    }

    #[test]
    fn toggle_hook_and_status_labels_are_preserved() {
        assert_eq!(TOGGLE_ELEMENT_ID, "toggle");
        assert_eq!(status_line("active"), "tool-fs: active");
        assert_eq!(status_line("unloaded"), "tool-fs: unloaded");
    }

    #[test]
    fn unload_failure_has_a_visible_message_instead_of_being_discarded() {
        assert_eq!(unload_error_message("boom"), "unload error: boom");
        assert_eq!(mount_error_message("boom"), "mount error: boom");
    }

    #[test]
    fn refresh_keeps_the_visible_error_after_a_toggle() {
        let ctx = Context::root("toggle-test-error");
        let state = Arc::new(Mutex::new(ToggleState { fiber: None, ctx }));
        let mut panel = TogglePanel {
            state,
            status: SharedString::from(status_line("active")),
            error: Some(SharedString::from(unload_error_message("boom"))),
        };
        panel.refresh();
        assert_eq!(panel.status.to_string(), status_line("unloaded"));
        assert_eq!(
            panel.error.as_ref().map(|error| error.to_string()),
            Some(unload_error_message("boom"))
        );
    }
}

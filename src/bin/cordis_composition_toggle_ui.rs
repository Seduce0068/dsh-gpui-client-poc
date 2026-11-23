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
    App, Application, AsyncApp, Context as GpuiContext, Entity, Render, SharedString, Window, div,
    prelude::*, rgb,
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

/// Single source for the window-creation log line. The title demo uses the
/// identical helper so both binaries report window setup the same way.
fn window_log_line(opened: bool, detail: &str) -> String {
    if opened {
        "GPUI window created".to_string()
    } else {
        format!("failed to create window: {detail}")
    }
}

/// The one place this binary creates its window. Keeping the result checked
/// (instead of `let _ = cx.open_window(...)`) preserves the existing
/// success/failure messages while making the wording regression-testable.
fn open_panel_window(panel: Entity<TogglePanel>, cx: &mut App) {
    match cx.open_window(Default::default(), |_window, _cx| panel.clone()) {
        Ok(_window) => eprintln!(
            "cordis_composition_toggle_ui: {}",
            window_log_line(true, "")
        ),
        Err(error) => eprintln!(
            "cordis_composition_toggle_ui: {}",
            window_log_line(false, &error.to_string())
        ),
    }
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

    /// Kick off the toggle on the GPUI executor instead of blocking the UI
    /// thread with `futures::executor::block_on` while holding the state lock.
    fn toggle(&mut self, cx: &mut App, panel: gpui::WeakEntity<TogglePanel>) {
        self.error = None;
        let state = self.state.clone();
        cx.spawn(async move |app: &mut AsyncApp| {
            let outcome = toggle_once(&state).await;
            if let Err(message) = &outcome {
                eprintln!("cordis_composition_toggle_ui: {message}");
            }
            let _ = panel.update(app, |panel, _cx| {
                panel.error = outcome.err().map(SharedString::from);
                panel.refresh();
            });
        })
        .detach();
    }
}

/// One toggle attempt, shared by the spawned UI task and the regression test.
/// Each lock is released before the async mount/unload so no `MutexGuard`
/// survives an await point.
async fn toggle_once(state: &Arc<Mutex<ToggleState>>) -> Result<(), String> {
    let fiber = state.lock().expect("toggle state poisoned").fiber.take();
    if let Some(fiber) = fiber {
        if let Err(error) = fiber.unload().await {
            // Keep the fiber on a failed unload so the UI state stays truthful
            // (swarm review round 283); otherwise a retry would mount a second
            // fiber while the first one's effects are still live.
            state.lock().expect("toggle state poisoned").fiber = Some(fiber);
            Err(unload_error_message(&error.to_string()))
        } else {
            Ok(())
        }
    } else {
        let ctx = state.lock().expect("toggle state poisoned").ctx.clone();
        match Fiber::mount(
            &ctx,
            Box::new(ToolFs {
                config: cordis_rs::plugin_ports::ToolFsConfig::default(),
            }),
        )
        .await
        {
            Ok(fiber) => {
                state.lock().expect("toggle state poisoned").fiber = Some(fiber);
                Ok(())
            }
            Err(error) => Err(mount_error_message(&error.to_string())),
        }
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
                .on_click(cx.listener(|panel, _event, _window, cx| {
                    let weak = cx.weak_entity();
                    panel.toggle(cx, weak);
                })),
        )
    }
}

/// Async setup is confined to a short-lived current-thread runtime; after the
/// window opens all further cordis work runs on the GPUI executor via
/// `cx.spawn`. This keeps the execution model identical to the title demo.
async fn build_state() -> std::result::Result<Arc<Mutex<ToggleState>>, Box<dyn std::error::Error>> {
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

    Ok(Arc::new(Mutex::new(ToggleState {
        fiber: Some(fiber),
        ctx,
    })))
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let state = runtime.block_on(build_state())?;

    Application::new().run(move |cx: &mut App| {
        let mut panel = TogglePanel {
            state: state.clone(),
            status: SharedString::from(status_line("active")),
            error: None,
        };
        panel.refresh();
        let panel = cx.new(|_cx| panel);
        open_panel_window(panel, cx);
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

    #[tokio::test]
    async fn spawned_toggle_once_roundtrip_keeps_state_in_sync() {
        let ctx = Context::root("toggle-test-spawned");
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
        let state = Arc::new(Mutex::new(ToggleState {
            fiber: Some(fiber),
            ctx,
        }));

        // The same async path the spawned click handler uses must unload and
        // remount without leaving the shared state stale.
        toggle_once(&state).await.unwrap();
        assert!(state.lock().expect("toggle state poisoned").fiber.is_none());

        toggle_once(&state).await.unwrap();
        let guard = state.lock().expect("toggle state poisoned");
        assert_eq!(
            guard.fiber.as_ref().map(Fiber::state),
            Some(Lifecycle::Active)
        );
    }

    #[test]
    fn window_log_line_is_shared_and_preserves_messages() {
        assert_eq!(window_log_line(true, ""), "GPUI window created");
        assert_eq!(
            window_log_line(false, "boom"),
            "failed to create window: boom"
        );
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

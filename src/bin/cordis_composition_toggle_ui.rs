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

struct ToggleState {
    fiber: Option<Fiber>,
    ctx: Context,
}

struct TogglePanel {
    state: Arc<Mutex<ToggleState>>,
    status: SharedString,
}

impl TogglePanel {
    fn refresh(&mut self) {
        let state = self.state.lock().expect("toggle state poisoned");
        let label = match state.fiber.as_ref().map(|fiber| fiber.state()) {
            Some(Lifecycle::Active) => "active",
            _ => "unloaded",
        };
        self.status = SharedString::from(format!("tool-fs: {label}"));
    }

    fn toggle(&mut self) {
        {
            let mut state = self.state.lock().expect("toggle state poisoned");
            if let Some(fiber) = state.fiber.take() {
                let _ = futures::executor::block_on(fiber.unload());
            } else {
                match futures::executor::block_on(Fiber::mount(
                    &state.ctx,
                    Box::new(ToolFs {
                        config: cordis_rs::plugin_ports::ToolFsConfig::default(),
                    }),
                )) {
                    Ok(fiber) => state.fiber = Some(fiber),
                    Err(error) => {
                        self.status = SharedString::from(format!("mount error: {error}"));
                    }
                }
            }
        }
        self.refresh();
    }
}

impl Render for TogglePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let status = self.status.clone();
        let label = status.to_string();
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_3()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — plugin toggle"))
            .child(div().text_sm().child(status))
            .child(
                div()
                    .id("toggle")
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
            status: SharedString::from("tool-fs: active"),
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
}

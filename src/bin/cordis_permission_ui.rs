//! GPUI demo: switch permission presets and run a tool under each.
//!
//! Mirrors the CLI permission_flow_demo with a clickable UI: three preset
//! buttons, the current approval/sandbox state, and the result of running a
//! `bash` tool through the approval-gated pipeline.

use std::sync::{Arc, Mutex};

use cordis_rs::Context;
use cordis_rs::core_services::{
    ApprovalDecision, ToolDefinition, ToolError, ToolExecContext, ToolOutput, provide_approval,
    provide_permission_presets, provide_tools,
};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

const PRESETS: [&str; 3] = ["read-only", "workspace-write", "danger-full-access"];

struct PermissionState {
    preset: String,
    approval: String,
    sandbox: String,
    result: String,
}

struct PermissionPanel {
    state: Arc<Mutex<PermissionState>>,
}

impl PermissionPanel {
    fn switch(&mut self, preset: &str) -> Result<(), String> {
        // Recreate the runtime pieces per switch so the policy is fresh.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime.block_on(async {
            let ctx = Context::root("permission-ui");
            let (registry, pipeline) = provide_tools(&ctx).await.map_err(|e| e.to_string())?;
            let approval = provide_approval(&ctx).await.map_err(|e| e.to_string())?;
            let presets = provide_permission_presets(&ctx)
                .await
                .map_err(|e| e.to_string())?;
            presets.bind_approval(approval.clone());
            pipeline.with_approval(approval.clone());

            let _answerer =
                approval.add_answerer("user", Arc::new(|_request| Some(ApprovalDecision::Allow)));
            let _tool = registry.register(ToolDefinition::new(
                "bash",
                "Run a command",
                Arc::new(|_args, _exec| {
                    Box::pin(async { Ok(ToolOutput::text("command output")) })
                        as cordis_rs::events::BoxFuture<'static, Result<ToolOutput, ToolError>>
                }),
            ));

            let current = presets.switch(preset).map_err(|e| e.to_string())?;
            let result = pipeline
                .run(
                    &registry,
                    "bash",
                    serde_json::json!({}),
                    ToolExecContext::agent("ui"),
                )
                .await;

            let mut guard = self.state.lock().unwrap();
            guard.preset = preset.to_string();
            guard.approval = current.approval.as_str().to_string();
            guard.sandbox = current.sandbox.as_str().to_string();
            guard.result = match result {
                Ok(output) => format!("ALLOWED: {}", output.rendered.join("")),
                Err(error) => format!("DENIED: {error}"),
            };
            Ok(())
        })
    }
}

impl Render for PermissionPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let (preset, approval, sandbox, result) = {
            let state = self.state.lock().unwrap();
            (
                state.preset.clone(),
                state.approval.clone(),
                state.sandbox.clone(),
                state.result.clone(),
            )
        };
        let mut pane = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_3()
            .text_color(rgb(0xe8e8ef))
            .child(
                div()
                    .text_xl()
                    .child("cordis-rs × GPUI — permission presets"),
            )
            .child(div().text_sm().child(SharedString::from(format!(
                "preset: {preset} | approval: {approval} | sandbox: {sandbox}"
            ))))
            .child(
                div()
                    .text_sm()
                    .text_color(if result.starts_with("ALLOWED") {
                        rgb(0x9ce0b0)
                    } else {
                        rgb(0xff8c8c)
                    })
                    .child(SharedString::from(result)),
            );
        for (index, preset) in PRESETS.iter().enumerate() {
            let name = preset.to_string();
            pane = pane.child(
                div()
                    .id(("preset", index))
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x2b3a4a))
                    .text_color(rgb(0xe8e8ef))
                    .child(SharedString::from(format!("switch to {name}")))
                    .on_click(cx.listener(move |panel, _event, _window, _cx| {
                        if let Err(error) = panel.switch(&name) {
                            panel.state.lock().unwrap().result = format!("ERROR: {error}");
                        }
                    })),
            );
        }
        pane
    }
}

fn main() {
    let state = Arc::new(Mutex::new(PermissionState {
        preset: "(none)".to_string(),
        approval: "never".to_string(),
        sandbox: "workspace-write".to_string(),
        result: "choose a preset, then the tool runs".to_string(),
    }));
    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| PermissionPanel {
            state: state.clone(),
        });
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_permission_ui: GPUI window created");
    });
}

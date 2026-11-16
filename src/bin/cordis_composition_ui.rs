//! GPUI view of a real DSH composition mounted on cordis-rs.
//!
//! Loads `fixtures/text-turn.cordis.yml` (copied from the upstream SDK
//! snapshot) and boots every entry whose plugin id has a factory, then renders
//! the mounted fibers and their states in a window.

use cordis_rs::core_services::{
    provide_commands, provide_fs, provide_llm, provide_skills, provide_subagents, provide_tools,
};
use cordis_rs::loader::{Composition, PluginFactory, PluginRegistry, VarTable};
use cordis_rs::plugin_ports::{
    DeepSeekConfig, MessageFeedbackConfig, SkillFsConfig, ToolBashConfig, ToolFsConfig,
    ToolSubagentConfig, mount_feedback_producer, mount_llm_deepseek, mount_message_feedback,
    mount_session_persistence_jsonl, mount_skill_filesystem, mount_tool_bash, mount_tool_fs,
    mount_tool_subagent, turn_stopping_event,
};
use cordis_rs::{Context, Lifecycle};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

struct CompositionPanel {
    lines: Vec<SharedString>,
}

impl CompositionPanel {
    fn new(lines: Vec<String>) -> Self {
        Self {
            lines: lines.into_iter().map(SharedString::from).collect(),
        }
    }
}

impl Render for CompositionPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let mut pane = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_2()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — real composition"));
        for line in &self.lines {
            pane = pane.child(div().text_sm().child(line.clone()));
        }
        pane
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let yaml = include_str!("../../fixtures/text-turn.cordis.yml");
    let vars = VarTable::new()
        .with_home_path("/tmp/cordis-gpui")
        .with_env_value("DSH_SNAPSHOT_FEEDBACK", "1");
    let composition = Composition::parse_with_vars(yaml, &vars)?;

    let ctx = Context::root("gpui-composition");
    let _ = provide_tools(&ctx).await?;
    let fs = provide_fs(&ctx).await?;
    let _ = provide_skills(&ctx).await?;
    let _ = provide_subagents(&ctx).await?;
    let _ = provide_llm(&ctx).await?;
    let _ = provide_commands(&ctx).await?;

    let mut registry = PluginRegistry::new();
    let c = ctx.clone();
    let f = fs.clone();
    registry.register(
        "@deepseek-ai/dsh-session-persistence-jsonl",
        PluginFactory::new(move |_ctx, _config| {
            let c = c.clone();
            async move {
                mount_session_persistence_jsonl(&c, "/tmp/cordis-gpui/sessions").await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    registry.register(
        "@deepseek-ai/dsh-skill-filesystem",
        PluginFactory::new(move |_ctx, _config| {
            let c = c.clone();
            async move {
                mount_skill_filesystem(&c, SkillFsConfig::default()).await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    let mut env = std::collections::BTreeMap::new();
    env.insert("DEEPSEEK_API_KEY".to_string(), "sk-demo".to_string());
    registry.register(
        "@deepseek-ai/dsh-llm-deepseek-api-key",
        PluginFactory::new(move |_ctx, _config| {
            let c = c.clone();
            let env = env.clone();
            async move {
                mount_llm_deepseek(&c, DeepSeekConfig::default(), env).await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    registry.register(
        "@deepseek-ai/dsh-message-feedback",
        PluginFactory::new(move |_ctx, _config| {
            let c = c.clone();
            async move {
                mount_message_feedback(&c, MessageFeedbackConfig::default()).await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    registry.register(
        "@deepseek-ai/dsh-tool-subagent",
        PluginFactory::new(move |_ctx, _config| {
            let c = c.clone();
            async move {
                mount_tool_subagent(
                    &c,
                    ToolSubagentConfig {
                        provider: "spawn".to_string(),
                        ..Default::default()
                    },
                )
                .await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    registry.register(
        "./feedback-producer.mjs",
        PluginFactory::new(move |_ctx, _config| {
            let c = c.clone();
            async move {
                mount_feedback_producer(&c, turn_stopping_event()).await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    let f2 = f.clone();
    registry.register(
        "@deepseek-ai/dsh-tool-fs",
        PluginFactory::new(move |_ctx, _config| {
            let c = c.clone();
            let f = f2.clone();
            async move {
                mount_tool_fs(&c, f, ToolFsConfig::default()).await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    registry.register(
        "@deepseek-ai/dsh-tool-bash",
        PluginFactory::new(move |_ctx, _config| {
            let c = c.clone();
            async move {
                mount_tool_bash(
                    &c,
                    cordis_rs::core_services::Shell::new(),
                    ToolBashConfig::default(),
                )
                .await?;
                Ok(())
            }
        }),
    );

    let report = composition.mount_available(&ctx, &registry).await?;

    let mut lines = vec![format!(
        "entries: {} / mounted: {} / skipped: {}",
        composition.entries.len(),
        report.mounted.len(),
        report.skipped_unknown.len()
    )];
    for fiber in &report.mounted {
        lines.push(format!(
            "  {} = {:?}",
            fiber.name(),
            if fiber.state() == Lifecycle::Active {
                "active"
            } else {
                "inactive"
            }
        ));
    }
    for (id, _) in &report.skipped_unknown {
        lines.push(format!("  (skipped) {id}"));
    }
    eprintln!(
        "cordis_composition_ui: {} fibers mounted",
        report.mounted.len()
    );

    let _ = std::fs::remove_dir_all("/tmp/cordis-gpui");
    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| CompositionPanel::new(lines.clone()));
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_composition_ui: GPUI window created");
    });
    Ok(())
}

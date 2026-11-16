//! GPUI demo: toggle individual entries of a real DSH composition.
//!
//! Loads the upstream sdk/text-turn fixture, mounts every mapped entry as a
//! Fiber, and renders one clickable row per entry. Clicking unloads that
//! fiber; clicking again remounts it from the entry's factory.

use std::sync::{Arc, Mutex};

use cordis_rs::core_services::{
    provide_commands, provide_fs, provide_llm, provide_skills, provide_subagents, provide_tools,
};
use cordis_rs::loader::{Composition, PluginFactory, PluginRegistry, VarTable};
use cordis_rs::plugin_ports::{
    DeepSeekConfig, MessageFeedbackConfig, SkillFsConfig, ToolSubagentConfig,
    mount_feedback_producer, mount_llm_deepseek, mount_message_feedback,
    mount_session_persistence_jsonl, mount_skill_filesystem, mount_tool_subagent,
    turn_stopping_event,
};
use cordis_rs::{Context, Fiber, Lifecycle};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

struct EntrySlot {
    id: String,
    fiber: Option<Fiber>,
}

struct ToggleState {
    entries: Vec<EntrySlot>,
    ctx: Context,
    registry: PluginRegistry,
    composition: Composition,
    /// Persisted enable/disable choices, saved on every toggle.
    saved: cordis_rs::loader::CompositionState,
    state_path: std::path::PathBuf,
}

struct Panel {
    state: Arc<Mutex<ToggleState>>,
    lines: Vec<SharedString>,
}

impl Panel {
    fn refresh(&mut self) {
        let state = self.state.lock().expect("toggle state poisoned");
        self.lines = state
            .entries
            .iter()
            .map(|slot| {
                let label = match slot.fiber.as_ref().map(|fiber| fiber.state()) {
                    Some(Lifecycle::Active) => "active",
                    _ => "unloaded",
                };
                SharedString::from(format!("{}: {label}", slot.id))
            })
            .collect();
    }

    fn toggle(&mut self, index: usize) {
        // Collect the data needed for a remount before taking a mutable slot
        // borrow, so `state` is not borrowed both ways at once.
        let (ctx, plugin) = {
            let state = self.state.lock().expect("toggle state poisoned");
            let Some(slot) = state.entries.get(index) else {
                return;
            };
            if slot.fiber.is_some() {
                (None, None)
            } else {
                let entry = state
                    .composition
                    .entries
                    .iter()
                    .find(|entry| entry.id == slot.id)
                    .cloned();
                let plugin = entry.and_then(|entry| entry.to_plugin(&state.registry).ok());
                (Some(state.ctx.clone()), plugin)
            }
        };

        let error_message = {
            let mut state = self.state.lock().expect("toggle state poisoned");
            if let Some(slot) = state.entries.get_mut(index) {
                if let Some(fiber) = slot.fiber.take() {
                    let _ = futures::executor::block_on(fiber.unload());
                    None
                } else if let (Some(ctx), Some(plugin)) = (ctx, plugin) {
                    match futures::executor::block_on(Fiber::mount(&ctx, Box::new(plugin))) {
                        Ok(fiber) => {
                            slot.fiber = Some(fiber);
                            None
                        }
                        Err(error) => Some(format!("error: {error}")),
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(message) = error_message {
            self.lines.push(SharedString::from(message));
        }

        // Persist the new enable/disable state after a successful toggle.
        {
            let mut state = self.state.lock().expect("toggle state poisoned");
            let choice = state
                .entries
                .get(index)
                .map(|slot| (slot.id.clone(), slot.fiber.is_some()));
            if let Some((id, enabled)) = choice {
                state.saved.set(id, enabled);
            }
            let path = state.state_path.clone();
            let _ = state.saved.save(&path);
        }
        self.refresh();
    }
}

impl Render for Panel {
    fn render(&mut self, _window: &mut Window, cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let mut pane = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_2()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — entry toggles"));
        for (index, line) in self.lines.clone().into_iter().enumerate() {
            pane = pane.child(
                div()
                    .id(("entry", index))
                    .p_2()
                    .rounded_md()
                    .bg(rgb(0x2a2a33))
                    .child(line)
                    .on_click(cx.listener(move |panel, _event, _window, _cx| {
                        panel.toggle(index);
                    })),
            );
        }
        pane
    }
}

fn build_registry(ctx: &Context) -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    let c = ctx.clone();
    registry.register(
        "@deepseek-ai/dsh-session-persistence-jsonl",
        PluginFactory::new(move |_ctx, _config| {
            let c = c.clone();
            async move {
                mount_session_persistence_jsonl(&c, "/tmp/cordis-entries/sessions").await?;
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
    registry
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let yaml = include_str!("../../fixtures/text-turn.cordis.yml");
    let vars = VarTable::new()
        .with_home_path("/tmp/cordis-entries")
        .with_env_value("DSH_SNAPSHOT_FEEDBACK", "1");
    let composition = Composition::parse_with_vars(yaml, &vars)?;

    let ctx = Context::root("entries-toggle");
    let _ = provide_tools(&ctx).await?;
    let _ = provide_fs(&ctx).await?;
    let _ = provide_skills(&ctx).await?;
    let _ = provide_subagents(&ctx).await?;
    let _ = provide_llm(&ctx).await?;
    let _ = provide_commands(&ctx).await?;
    let registry = build_registry(&ctx);

    let report = composition.mount_available(&ctx, &registry).await?;

    // Restore previously saved enable/disable choices.
    let state_path = std::path::PathBuf::from("/tmp/cordis-entries-toggle.json");
    let saved = cordis_rs::loader::CompositionState::load(&state_path);

    let mut entries = Vec::new();
    for fiber in report.mounted {
        let id = fiber.name().to_string();
        // YAML default is enabled; an explicit saved `false` disables it.
        if !saved.is_enabled(&id, true) {
            let _ = fiber.unload().await;
            entries.push(EntrySlot { id, fiber: None });
        } else {
            entries.push(EntrySlot {
                id,
                fiber: Some(fiber),
            });
        }
    }
    for (id, _) in report.skipped_unknown {
        entries.push(EntrySlot { id, fiber: None });
    }
    let state = Arc::new(Mutex::new(ToggleState {
        entries,
        ctx: ctx.clone(),
        registry,
        composition,
        saved,
        state_path,
    }));

    Application::new().run(move |cx: &mut App| {
        let mut panel = Panel {
            state: state.clone(),
            lines: Vec::new(),
        };
        panel.refresh();
        let panel = cx.new(|_cx| panel);
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_entries_toggle_ui: GPUI window created");
    });
    Ok(())
}

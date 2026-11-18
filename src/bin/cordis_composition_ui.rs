//! GPUI view of a real DSH composition mounted on cordis-rs.
//!
//! Loads `fixtures/text-turn.cordis.yml` (copied from the upstream SDK
//! snapshot) and boots every entry whose plugin id has a factory, then renders
//! the mounted fibers and their states in a window.

use cordis_rs::config::Value;
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

/// Base directory for persistent state, following the XDG state-home
/// convention: `$XDG_STATE_HOME` when it is absolute, otherwise
/// `$HOME/.local/state` (temp dir only as a last resort).
fn xdg_state_home() -> std::path::PathBuf {
    if let Some(raw) = std::env::var_os("XDG_STATE_HOME") {
        let candidate = std::path::PathBuf::from(&raw);
        if !raw.is_empty() && candidate.is_absolute() {
            return candidate;
        }
    }
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| std::path::PathBuf::from(home).join(".local/state"))
        .unwrap_or_else(std::env::temp_dir)
}

/// Build a per-process directory beneath `base`. The pid component keeps
/// concurrent demo runs from sharing and clobbering each other's state.
fn per_process_state_dir(base: &std::path::Path, app: &str) -> std::path::PathBuf {
    base.join(app).join(format!("pid-{}", std::process::id()))
}

/// Resolve and create a state directory under `base`. `create_dir_all` is
/// idempotent and non-destructive, unlike the old startup wipe that erased
/// whatever a previous run had written.
fn prepare_state_dir_at(
    base: &std::path::Path,
    app: &str,
) -> Result<std::path::PathBuf, std::io::Error> {
    let dir = per_process_state_dir(base, app);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn prepare_state_dir(app: &str) -> Result<std::path::PathBuf, std::io::Error> {
    prepare_state_dir_at(&xdg_state_home(), app)
}

// The loader hands each factory a `config::Value` built from the YAML entry.
// These accessors keep the conversion to each port's typed config explicit and
// testable; absent or wrongly typed fields fall back to the port's defaults.
fn config_str<'a>(config: &'a Value, key: &str) -> Option<&'a str> {
    config.as_object()?.get(key)?.as_str()
}

fn config_i64(config: &Value, key: &str) -> Option<i64> {
    config.as_object()?.get(key)?.as_i64()
}

fn config_bool(config: &Value, key: &str) -> Option<bool> {
    config.as_object()?.get(key)?.as_bool()
}

fn skill_fs_config(config: &Value) -> SkillFsConfig {
    let mut parsed = SkillFsConfig::default();
    if let Some(roots) = config
        .as_object()
        .and_then(|map| map.get("roots"))
        .and_then(Value::as_array)
    {
        parsed.roots = roots
            .iter()
            .filter_map(Value::as_str)
            .map(std::path::PathBuf::from)
            .collect();
    }
    if let Some(rank) = config_i64(config, "rank") {
        parsed.rank = rank;
    }
    // `includeDefaultRoots: false` needs no action: this host never injects
    // default skill roots, so the empty `roots` default already honours it.
    parsed
}

fn deepseek_config(config: &Value) -> DeepSeekConfig {
    let mut parsed = DeepSeekConfig::default();
    if let Some(value) = config_str(config, "apiKeyEnv") {
        parsed.api_key_env = value.to_string();
    }
    if let Some(value) = config_str(config, "baseUrl") {
        parsed.base_url = value.to_string();
    }
    if let Some(value) = config_str(config, "model") {
        parsed.model = value.to_string();
    }
    if let Some(flag) = config_bool(config, "thinking") {
        parsed.thinking = flag;
    } else if let Some(value) = config_str(config, "thinking") {
        parsed.thinking = value == "enabled";
    }
    // `reasoningEffort` has no field in this Rust port; the model is selected
    // through `model`, so there is nothing to apply here.
    parsed
}

fn message_feedback_config(config: &Value) -> MessageFeedbackConfig {
    let mut parsed = MessageFeedbackConfig::default();
    if let Some(bytes) =
        config_i64(config, "maxNoteBytes").or_else(|| config_i64(config, "max_note_bytes"))
    {
        parsed.max_note_bytes = bytes.max(0) as usize;
    }
    parsed
}

fn tool_subagent_config(config: &Value) -> ToolSubagentConfig {
    let mut parsed = ToolSubagentConfig::default();
    if let Some(value) = config_str(config, "provider") {
        parsed.provider = value.to_string();
    }
    if let Some(value) = config_str(config, "toolName").or_else(|| config_str(config, "tool_name"))
    {
        parsed.tool_name = value.to_string();
    }
    if let Some(depth) = config_i64(config, "maxDepth").or_else(|| config_i64(config, "max_depth"))
    {
        parsed.max_depth = depth.max(0) as usize;
    }
    parsed
}

fn session_root(config: &Value, fallback: &std::path::Path) -> std::path::PathBuf {
    config_str(config, "root")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| fallback.to_path_buf())
}

/// Lines describing mount failures so they are visible in the panel instead
/// of being silently dropped from `MountReport::failed`.
fn failed_lines(failed: &[(String, String)]) -> Vec<String> {
    failed
        .iter()
        .map(|(id, error)| format!("  (failed) {id}: {error}"))
        .collect()
}

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
    let state_dir = prepare_state_dir("cordis-composition-ui")?;
    let home = state_dir.join("home");
    let sessions = state_dir.join("sessions");
    let yaml = include_str!("../../fixtures/text-turn.cordis.yml");
    let vars = VarTable::new()
        .with_home_path(&home)
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
    let s = sessions.clone();
    registry.register(
        "@deepseek-ai/dsh-session-persistence-jsonl",
        PluginFactory::new(move |_ctx, config| {
            let c = c.clone();
            let root = session_root(config, &s);
            async move {
                mount_session_persistence_jsonl(&c, &root).await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    registry.register(
        "@deepseek-ai/dsh-skill-filesystem",
        PluginFactory::new(move |_ctx, config| {
            let c = c.clone();
            let parsed = skill_fs_config(config);
            async move {
                mount_skill_filesystem(&c, parsed).await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    let mut env = std::collections::BTreeMap::new();
    env.insert("DEEPSEEK_API_KEY".to_string(), "sk-demo".to_string());
    registry.register(
        "@deepseek-ai/dsh-llm-deepseek-api-key",
        PluginFactory::new(move |_ctx, config| {
            let c = c.clone();
            let env = env.clone();
            let parsed = deepseek_config(config);
            async move {
                mount_llm_deepseek(&c, parsed, env).await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    registry.register(
        "@deepseek-ai/dsh-message-feedback",
        PluginFactory::new(move |_ctx, config| {
            let c = c.clone();
            let parsed = message_feedback_config(config);
            async move {
                mount_message_feedback(&c, parsed).await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    registry.register(
        "@deepseek-ai/dsh-tool-subagent",
        PluginFactory::new(move |_ctx, config| {
            let c = c.clone();
            let parsed = tool_subagent_config(config);
            async move {
                mount_tool_subagent(&c, parsed).await?;
                Ok(())
            }
        }),
    );
    let c = ctx.clone();
    registry.register(
        "./feedback-producer.mjs",
        // The producer port exposes a fixed turn-stopping event; its YAML
        // entry has no config fields to apply.
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
        "entries: {} / mounted: {} / skipped: {} / failed: {}",
        composition.entries.len(),
        report.mounted.len(),
        report.skipped_unknown.len(),
        report.failed.len()
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
    lines.extend(failed_lines(&report.failed));
    for (id, error) in &report.failed {
        eprintln!("cordis_composition_ui: mount failed for {id}: {error}");
    }
    eprintln!(
        "cordis_composition_ui: {} fibers mounted",
        report.mounted.len()
    );

    // The per-process state directory is intentionally left in place: no
    // startup directory wipe, so a run never erases another run's state.
    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| CompositionPanel::new(lines.clone()));
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_composition_ui: GPUI window created");
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_base(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "dsh-composition-state-test-{}-{label}",
            std::process::id()
        ))
    }

    #[test]
    fn state_dir_is_scoped_to_base_and_per_process() {
        let base = test_base("scope");
        let dir = per_process_state_dir(&base, "cordis-composition-ui");
        assert!(dir.starts_with(&base));
        assert!(dir.ends_with(format!("pid-{}", std::process::id())));
        assert_eq!(
            dir.parent().and_then(|p| p.file_name()).unwrap(),
            "cordis-composition-ui"
        );
        assert!(xdg_state_home().is_absolute());
    }

    #[test]
    fn prepare_state_dir_keeps_existing_files() {
        let base = test_base("preserve");
        let first = prepare_state_dir_at(&base, "cordis-composition-ui").unwrap();
        let sentinel = first.join("keep-me.txt");
        std::fs::write(&sentinel, "persist").unwrap();

        let second = prepare_state_dir_at(&base, "cordis-composition-ui").unwrap();
        assert_eq!(first, second);
        assert_eq!(
            std::fs::read_to_string(second.join("keep-me.txt")).unwrap(),
            "persist"
        );

        std::fs::remove_file(&sentinel).unwrap();
        let _ = std::fs::remove_dir(&second);
        let _ = std::fs::remove_dir(second.parent().unwrap());
        let _ = std::fs::remove_dir(&base);
    }

    fn fixture_composition() -> Composition {
        let home = std::env::temp_dir().join(format!(
            "dsh-composition-fixture-home-{}",
            std::process::id()
        ));
        let vars = VarTable::new()
            .with_home_path(&home)
            .with_env_value("DSH_SNAPSHOT_FEEDBACK", "1");
        Composition::parse_with_vars(include_str!("../../fixtures/text-turn.cordis.yml"), &vars)
            .unwrap()
    }

    #[test]
    fn factory_configs_are_applied_from_fixture() {
        let composition = fixture_composition();
        let config_of = |id: &str| -> Value {
            composition
                .entries
                .iter()
                .find(|entry| entry.id == id)
                .unwrap()
                .config
                .clone()
        };

        let subagent = tool_subagent_config(&config_of("tool-subagent"));
        assert_eq!(subagent.provider, "spawn");
        assert_eq!(subagent.tool_name, "subagent");
        assert_eq!(subagent.max_depth, 1);

        let feedback = message_feedback_config(&config_of("snapshot-message-feedback"));
        assert_eq!(feedback.max_note_bytes, 1024);

        let llm = deepseek_config(&config_of("llm-deepseek"));
        assert!(llm.thinking, "`thinking: enabled` must become true");

        let fallback = std::path::Path::new("/tmp/fallback-sessions");
        let root = session_root(&config_of("session-persistence-jsonl"), fallback);
        assert_ne!(root, fallback, "the YAML `root` must win over the fallback");

        let skills = skill_fs_config(&config_of("skill-filesystem"));
        assert!(skills.roots.is_empty(), "includeDefaultRoots: false");
    }

    #[test]
    fn failed_mount_reports_are_rendered() {
        let failed = vec![
            ("plugin-a".to_string(), "mount: boom".to_string()),
            ("plugin-b".to_string(), "validation: bad".to_string()),
        ];
        let lines = failed_lines(&failed);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "  (failed) plugin-a: mount: boom");
        assert_eq!(lines[1], "  (failed) plugin-b: validation: bad");
        assert!(failed_lines(&[]).is_empty());
    }
}

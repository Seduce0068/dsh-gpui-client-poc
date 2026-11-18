//! GPUI demo: toggle individual entries of a real DSH composition.
//!
//! Loads the upstream sdk/text-turn fixture, mounts every mapped entry as a
//! Fiber, and renders one clickable row per entry. Clicking unloads that
//! fiber; clicking again remounts it from the entry's factory.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use cordis_rs::config::Value;
use cordis_rs::core_services::{
    provide_commands, provide_fs, provide_llm, provide_skills, provide_subagents, provide_tools,
};
use cordis_rs::loader::{Composition, CompositionState, PluginFactory, PluginRegistry, VarTable};
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
/// idempotent and non-destructive, unlike a startup directory wipe.
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

// Convert the loader's `config::Value` into each port's typed config. Missing
// or wrongly typed fields keep the port's defaults; every registered factory
// consumes its config instead of discarding it with `_config`.
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
    // `includeDefaultRoots: false` is already honoured: this host never
    // injects default skill roots, so the empty `roots` default applies.
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

/// Format `MountReport::failed` entries so the window shows why a plugin
/// could not be mounted instead of dropping the error entirely.
fn failed_lines(failed: &[(String, String)]) -> Vec<String> {
    failed
        .iter()
        .map(|(id, error)| format!("(failed) {id}: {error}"))
        .collect()
}

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
    saved: CompositionState,
    state_path: std::path::PathBuf,
}

struct Panel {
    state: Arc<Mutex<ToggleState>>,
    lines: Vec<SharedString>,
    /// Most recent mount/save failure, rendered in the window instead of
    /// being silently discarded.
    error: Option<SharedString>,
    /// Startup `MountReport::failed` rows, kept visible instead of dropped.
    failed: Vec<SharedString>,
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
        self.error = None;
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
            eprintln!("cordis_entries_toggle_ui: mount failed: {message}");
            self.error = Some(SharedString::from(message));
        }

        // Persist the new enable/disable state after a successful toggle.
        // `CompositionState::save` already writes a sibling temp file and
        // renames it into place, so the atomic-write mechanism is reused as-is;
        // the old code's only gap was discarding its Result with `let _ =`.
        let save_error = {
            let mut state = self.state.lock().expect("toggle state poisoned");
            let choice = state
                .entries
                .get(index)
                .map(|slot| (slot.id.clone(), slot.fiber.is_some()));
            if let Some((id, enabled)) = choice {
                state.saved.set(id, enabled);
            }
            let path = state.state_path.clone();
            state.saved.save(&path).err().map(|error| error.to_string())
        };
        if let Some(message) = save_error {
            eprintln!("cordis_entries_toggle_ui: state save failed: {message}");
            self.error = Some(SharedString::from(format!("save error: {message}")));
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
        if let Some(error) = &self.error {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xff6b6b))
                    .child(error.clone()),
            );
        }
        for failure in &self.failed {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xffa94d))
                    .child(failure.clone()),
            );
        }
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

fn build_registry(ctx: &Context, sessions: &std::path::Path) -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    let c = ctx.clone();
    let s = sessions.to_path_buf();
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
    let state_dir = prepare_state_dir("cordis-entries-toggle")?;
    let home = state_dir.join("home");
    let sessions = state_dir.join("sessions");
    let state_path = state_dir.join("entries-state.json");
    let yaml = include_str!("../../fixtures/text-turn.cordis.yml");
    let vars = VarTable::new()
        .with_home_path(&home)
        .with_env_value("DSH_SNAPSHOT_FEEDBACK", "1");
    let composition = Composition::parse_with_vars(yaml, &vars)?;

    let ctx = Context::root("entries-toggle");
    let _ = provide_tools(&ctx).await?;
    let _ = provide_fs(&ctx).await?;
    let _ = provide_skills(&ctx).await?;
    let _ = provide_subagents(&ctx).await?;
    let _ = provide_llm(&ctx).await?;
    let _ = provide_commands(&ctx).await?;
    let registry = build_registry(&ctx, &sessions);

    // Restore previously saved enable/disable choices. `load` maps a missing
    // or malformed file to empty overrides, so flag a malformed existing file
    // instead of silently falling back to the YAML defaults.
    if state_path.exists()
        && std::fs::read_to_string(&state_path)
            .is_ok_and(|text| serde_json::from_str::<serde_json::Value>(&text).is_err())
    {
        eprintln!(
            "cordis_entries_toggle_ui: ignoring malformed state file {}",
            state_path.display()
        );
    }
    let saved = CompositionState::load(&state_path);

    // `enabled_with` layers saved overrides onto each entry's YAML `disabled`
    // default. The old code hardcoded `saved.is_enabled(&id, true)`, so a
    // YAML `disabled: true` entry was mounted anyway.
    let enabled: BTreeSet<String> = composition
        .enabled_with(&saved)
        .into_iter()
        .map(|entry| entry.id)
        .collect();

    let report = composition.mount_available(&ctx, &registry).await?;

    // Keep only fibers that the YAML + persisted state enable; unload the
    // rest instead of showing them as active.
    let mut mounted: BTreeMap<String, Fiber> = BTreeMap::new();
    for fiber in report.mounted {
        let id = fiber.name().to_string();
        if enabled.contains(&id) {
            mounted.insert(id, fiber);
        } else {
            let _ = fiber.unload().await;
        }
    }

    // Surface `MountReport::failed` to the window and stderr rather than
    // dropping the entries (and their retry slots) silently.
    let mut failed: Vec<(String, String)> = report
        .failed
        .iter()
        .filter(|(id, _)| enabled.contains(id))
        .cloned()
        .collect();

    // `mount_available` only walks YAML-enabled rows. A saved override may
    // re-enable a YAML-disabled row, which was never mounted; mount it here.
    let attempted: BTreeSet<String> = mounted
        .keys()
        .cloned()
        .chain(failed.iter().map(|(id, _)| id.clone()))
        .chain(report.skipped_unknown.iter().map(|(id, _)| id.clone()))
        .collect();
    for entry in composition.enabled_with(&saved) {
        if attempted.contains(&entry.id) {
            continue;
        }
        match entry.to_plugin(&registry) {
            Ok(plugin) => match Fiber::mount(&ctx, Box::new(plugin)).await {
                Ok(fiber) => {
                    mounted.insert(entry.id.clone(), fiber);
                }
                Err(error) => failed.push((entry.id.clone(), format!("mount: {error}"))),
            },
            Err(error) => failed.push((entry.id.clone(), error.to_string())),
        }
    }

    for (id, error) in &failed {
        eprintln!("cordis_entries_toggle_ui: mount failed for {id}: {error}");
    }
    let failed_view: Vec<SharedString> = failed_lines(&failed)
        .into_iter()
        .map(SharedString::from)
        .collect();

    // Give every composition row a slot; disabled rows stay unloaded but can
    // still be toggled back on, so no entry vanishes from the window.
    let entries: Vec<EntrySlot> = composition
        .entries
        .iter()
        .map(|entry| EntrySlot {
            id: entry.id.clone(),
            fiber: mounted.remove(&entry.id),
        })
        .collect();
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
            error: None,
            failed: failed_view,
        };
        panel.refresh();
        let panel = cx.new(|_cx| panel);
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_entries_toggle_ui: GPUI window created");
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_base(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "dsh-entries-state-test-{}-{label}",
            std::process::id()
        ))
    }

    #[test]
    fn state_dir_is_scoped_to_base_and_per_process() {
        let base = test_base("scope");
        let dir = per_process_state_dir(&base, "cordis-entries-toggle");
        assert!(dir.starts_with(&base));
        assert!(dir.ends_with(format!("pid-{}", std::process::id())));
        assert_eq!(
            dir.parent().and_then(|p| p.file_name()).unwrap(),
            "cordis-entries-toggle"
        );
        assert!(xdg_state_home().is_absolute());
    }

    #[test]
    fn prepare_state_dir_keeps_existing_files() {
        let base = test_base("preserve");
        let first = prepare_state_dir_at(&base, "cordis-entries-toggle").unwrap();
        let sentinel = first.join("keep-me.txt");
        std::fs::write(&sentinel, "persist").unwrap();

        let second = prepare_state_dir_at(&base, "cordis-entries-toggle").unwrap();
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

    #[test]
    fn state_json_save_is_atomic_and_surfaces_errors() {
        let base = test_base("json");
        let dir = prepare_state_dir_at(&base, "cordis-entries-toggle").unwrap();
        let path = dir.join("entries-state.json");
        let mut saved = CompositionState::new();
        saved.set("entry-a", true);
        saved.set("entry-b", false);

        // `CompositionState::save` reuses the sibling temp file + rename
        // protocol: the final file appears in one step and the temp is gone.
        saved.save(&path).unwrap();
        assert!(!path.with_extension("tmp").exists());
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["entry-a"], serde_json::json!(true));
        assert_eq!(value["entry-b"], serde_json::json!(false));
        assert_eq!(CompositionState::load(&path), saved);

        // A failed write must produce Err so the UI can visualize it.
        let blocked = dir.join("not-a-directory");
        std::fs::write(&blocked, "occupied").unwrap();
        assert!(saved.save(blocked.join("state.json")).is_err());

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(&blocked).unwrap();
        let _ = std::fs::remove_dir(&dir);
        let _ = std::fs::remove_dir(dir.parent().unwrap());
        let _ = std::fs::remove_dir(&base);
    }

    fn parsed_composition(feedback_flag: &str) -> Composition {
        let home =
            std::env::temp_dir().join(format!("dsh-entries-fixture-home-{}", std::process::id()));
        let vars = VarTable::new()
            .with_home_path(&home)
            .with_env_value("DSH_SNAPSHOT_FEEDBACK", feedback_flag);
        Composition::parse_with_vars(include_str!("../../fixtures/text-turn.cordis.yml"), &vars)
            .unwrap()
    }

    #[test]
    fn enabled_with_respects_yaml_disabled_and_overrides() {
        let ids = |composition: &Composition, state: &CompositionState| -> BTreeSet<String> {
            composition
                .enabled_with(state)
                .into_iter()
                .map(|entry| entry.id)
                .collect()
        };

        // With the feedback flag off, the two `disabled: true` rows must be
        // excluded even though no saved override exists.
        let off = parsed_composition("0");
        let default_state = CompositionState::new();
        let off_ids = ids(&off, &default_state);
        assert!(!off_ids.contains("snapshot-message-feedback"));
        assert!(!off_ids.contains("snapshot-feedback-producer"));
        assert!(off_ids.contains("tool-subagent"));

        // With the flag on, the YAML default enables both rows.
        let on = parsed_composition("1");
        let on_ids = ids(&on, &default_state);
        assert!(on_ids.contains("snapshot-message-feedback"));
        assert!(on_ids.contains("snapshot-feedback-producer"));

        // A persisted `true` override can re-enable a YAML-disabled row.
        let mut override_state = CompositionState::new();
        override_state.set("snapshot-message-feedback", true);
        assert!(ids(&off, &override_state).contains("snapshot-message-feedback"));

        // A persisted `false` override disables an otherwise enabled row.
        let mut disable_state = CompositionState::new();
        disable_state.set("tool-subagent", false);
        assert!(!ids(&on, &disable_state).contains("tool-subagent"));
    }

    #[test]
    fn factory_configs_are_applied_from_fixture() {
        let composition = parsed_composition("1");
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
        assert_ne!(
            session_root(&config_of("session-persistence-jsonl"), fallback),
            fallback,
            "the YAML `root` must win over the fallback"
        );

        assert!(
            skill_fs_config(&config_of("skill-filesystem"))
                .roots
                .is_empty()
        );
    }

    #[test]
    fn failed_mount_reports_are_rendered() {
        let failed = vec![("plugin-a".to_string(), "mount: boom".to_string())];
        let lines = failed_lines(&failed);
        assert_eq!(lines, vec!["(failed) plugin-a: mount: boom"]);
        assert!(failed_lines(&[]).is_empty());
    }
}

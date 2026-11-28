//! GPUI demo: toggle individual entries of a real DSH composition.
//!
//! Loads the upstream sdk/text-turn fixture, mounts every mapped entry as a
//! Fiber, and renders one clickable row per entry. Clicking unloads that
//! fiber; clicking again remounts it from the entry's factory.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex};

use cordis_rs::config::Value;
use cordis_rs::core_services::{
    provide_commands, provide_fs, provide_llm, provide_skills, provide_subagents, provide_tools,
};
use cordis_rs::loader::{Composition, CompositionState, PluginFactory, PluginRegistry, VarTable};
use cordis_rs::plugin_ports::{
    DEFAULT_API_KEY_ENV, DeepSeekConfig, MessageFeedbackConfig, SkillFsConfig, ToolSubagentConfig,
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

/// The two state locations used by one run.
///
/// `stable` is derived from the app id and the user, never the pid, so a
/// restarted process resolves the same directory and reads the toggle choices
/// saved by its predecessor. `process` is pid-scoped run scratch, so
/// concurrent runs still cannot clobber each other's temporary files.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StateDirs {
    stable: std::path::PathBuf,
    process: std::path::PathBuf,
}

/// Reduce an environment-provided user id to one safe path component. Without
/// this, a value such as `../../etc` could escape `base` through `Path::join`.
fn sanitize_id_component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "unknown".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Stable user id from the usual environment names, with a safe fallback.
fn stable_user() -> String {
    std::env::var_os("USER")
        .or_else(|| std::env::var_os("USERNAME"))
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Pure path computation, kept separate from I/O so the restart case (same
/// app + user, different pid) can be tested directly.
fn state_dirs_for(base: &std::path::Path, app: &str, user: &str, pid: u32) -> StateDirs {
    let root = base.join(app);
    StateDirs {
        stable: root.join(format!("user-{}", sanitize_id_component(user))),
        process: root.join(format!("pid-{pid}")),
    }
}

/// Resolve and create both directories under `base`. `create_dir_all` is
/// idempotent and non-destructive: a run never erases an earlier run's state.
fn prepare_state_dir_at(base: &std::path::Path, app: &str) -> Result<StateDirs, std::io::Error> {
    let dirs = state_dirs_for(base, app, &stable_user(), std::process::id());
    std::fs::create_dir_all(&dirs.stable)?;
    std::fs::create_dir_all(&dirs.process)?;
    Ok(dirs)
}

fn prepare_state_dir(app: &str) -> Result<StateDirs, std::io::Error> {
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
        // A negative or out-of-range value is a config typo; keep the default
        // instead of clamping to zero, which MessageFeedback rejects
        // (same rule as cordis_composition_ui.rs).
        if let Ok(bytes) = usize::try_from(bytes) {
            if bytes > 0 {
                parsed.max_note_bytes = bytes;
            }
        }
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
        // Mirror cordis_composition_ui.rs: a converted 0 is a valid depth and
        // is adopted; only negative/out-of-range values keep the default.
        if let Ok(depth) = usize::try_from(depth) {
            parsed.max_depth = depth;
        }
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

/// Failure labels shared by the click handler and its regression tests. The
/// wording mirrors `cordis_composition_toggle_ui.rs` so both UIs report the
/// same kind of failure the same way.
fn unload_error_message(error: &str) -> String {
    format!("unload error: {error}")
}

fn mount_error_message(error: &str) -> String {
    format!("mount error: {error}")
}

fn plugin_error_message(error: &str) -> String {
    format!("plugin error: {error}")
}

/// CLI flag for the DeepSeek credential. The environment is the primary
/// source (argv is visible to other users on many systems); the flag exists
/// for launchers that can only inject secrets as arguments.
const API_KEY_FLAG: &str = "--api-key";

#[derive(Debug, Clone, PartialEq, Eq)]
enum CredentialSource {
    Cli,
    Env(String),
}

impl CredentialSource {
    /// Source label for logs; the credential value itself is never logged.
    fn label(&self) -> String {
        match self {
            Self::Cli => API_KEY_FLAG.to_owned(),
            Self::Env(name) => format!("env:{name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedCredential {
    key: String,
    source: CredentialSource,
}

/// Every variant is fatal at startup: no placeholder key may ever replace a
/// missing credential (fail-closed).
#[derive(Debug, Clone, PartialEq, Eq)]
enum CredentialSetupError {
    Missing { env_name: String },
    Empty { source: String },
    MalformedFlag(String),
    UnknownFlag(String),
    DuplicateFlag,
}

impl fmt::Display for CredentialSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { env_name } => write!(
                f,
                "DeepSeek credential not configured: pass {API_KEY_FLAG} <KEY> or export {env_name}"
            ),
            Self::Empty { source } => write!(f, "DeepSeek credential from {source} is empty"),
            Self::MalformedFlag(flag) => write!(f, "{flag} requires a non-empty value"),
            Self::UnknownFlag(flag) => write!(f, "unknown argument: {flag}"),
            Self::DuplicateFlag => write!(f, "{API_KEY_FLAG} specified more than once"),
        }
    }
}

impl std::error::Error for CredentialSetupError {}

/// Resolve the credential from CLI (`--api-key VALUE` / `--api-key=VALUE`) or
/// from the environment variable named by the composition's `apiKeyEnv`.
/// `lookup` is injected so the logic is testable without touching the process
/// environment. Absence, blank values and malformed flags are all errors;
/// there is deliberately no demo fallback.
fn resolve_credential<I>(
    args: I,
    lookup: impl Fn(&str) -> Option<String>,
    env_name: &str,
) -> Result<ResolvedCredential, CredentialSetupError>
where
    I: IntoIterator<Item = String>,
{
    let mut from_cli: Option<String> = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if let Some(tail) = arg.strip_prefix(API_KEY_FLAG) {
            let value = if let Some(value) = tail.strip_prefix('=') {
                value.to_owned()
            } else {
                if !tail.is_empty() {
                    return Err(CredentialSetupError::MalformedFlag(arg));
                }
                iter.next()
                    .ok_or_else(|| CredentialSetupError::MalformedFlag(arg.clone()))?
            };
            // A value that looks like another flag is a typo; a blank value is
            // classified later as `Empty` so the message names the source.
            if value.starts_with('-') {
                return Err(CredentialSetupError::MalformedFlag(arg));
            }
            if from_cli.replace(value).is_some() {
                return Err(CredentialSetupError::DuplicateFlag);
            }
        } else {
            return Err(CredentialSetupError::UnknownFlag(arg));
        }
    }

    let (key, source) = if let Some(key) = from_cli {
        (key, CredentialSource::Cli)
    } else {
        (
            lookup(env_name).ok_or_else(|| CredentialSetupError::Missing {
                env_name: env_name.to_owned(),
            })?,
            CredentialSource::Env(env_name.to_owned()),
        )
    };
    let key = key.trim().to_owned();
    if key.is_empty() {
        return Err(CredentialSetupError::Empty {
            source: source.label(),
        });
    }
    Ok(ResolvedCredential { key, source })
}

/// Env snapshot handed to the LLM port: exactly one entry under the name the
/// composition configured. The port's own `resolve_api_key` then succeeds only
/// because the host supplied a validated, non-empty credential.
fn llm_env(key_env_name: &str, resolved: &ResolvedCredential) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert(key_env_name.to_owned(), resolved.key.clone());
    env
}

/// `apiKeyEnv` from the fixture config (default `DEEPSEEK_API_KEY`) decides
/// which variable the port reads; the host and the port agree by construction.
fn llm_key_env_name(composition: &Composition) -> String {
    composition
        .entries
        .iter()
        .find(|entry| entry.id == "llm-deepseek")
        .map(|entry| deepseek_config(&entry.config).api_key_env)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_API_KEY_ENV.to_owned())
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
        // borrow, so `state` is not borrowed both ways at once. A `to_plugin`
        // error is carried forward instead of being dropped with `.ok()`, so a
        // failed remount is reported just like the startup path's failed rows.
        let (ctx, plugin, plugin_error) = {
            let state = self.state.lock().expect("toggle state poisoned");
            let Some(slot) = state.entries.get(index) else {
                return;
            };
            if slot.fiber.is_some() {
                (None, None, None)
            } else {
                let entry = state
                    .composition
                    .entries
                    .iter()
                    .find(|entry| entry.id == slot.id)
                    .cloned();
                match entry {
                    Some(entry) => match entry.to_plugin(&state.registry) {
                        Ok(plugin) => (Some(state.ctx.clone()), Some(plugin), None),
                        Err(error) => (None, None, Some(plugin_error_message(&error.to_string()))),
                    },
                    None => (None, None, None),
                }
            }
        };

        let error_message = if let Some(message) = plugin_error {
            Some(message)
        } else {
            let mut state = self.state.lock().expect("toggle state poisoned");
            if let Some(slot) = state.entries.get_mut(index) {
                if let Some(fiber) = slot.fiber.take() {
                    match futures::executor::block_on(fiber.unload()) {
                        Ok(()) => None,
                        Err(error) => {
                            // Keep the fiber in its slot on a failed unload so
                            // a retry cannot mount a second fiber over effects
                            // that are still live (same as the sibling UI).
                            slot.fiber = Some(fiber);
                            Some(unload_error_message(&error.to_string()))
                        }
                    }
                } else if let (Some(ctx), Some(plugin)) = (ctx, plugin) {
                    match futures::executor::block_on(Fiber::mount(&ctx, Box::new(plugin))) {
                        Ok(fiber) => {
                            slot.fiber = Some(fiber);
                            None
                        }
                        Err(error) => Some(mount_error_message(&error.to_string())),
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
            // A failed toggle must never persist a choice that disagrees with
            // the live slots (e.g. a `disabled` from a failed unload).
            self.refresh();
            return;
        }

        // Persist the new enable/disable state only after a successful toggle.
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

fn build_registry(
    ctx: &Context,
    sessions: &std::path::Path,
    port_env: BTreeMap<String, String>,
) -> PluginRegistry {
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
    let env = port_env.clone();
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
    let dirs = prepare_state_dir("cordis-entries-toggle")?;
    let home = dirs.stable.join("home");
    let sessions = dirs.stable.join("sessions");
    let state_path = dirs.stable.join("entries-state.json");
    eprintln!(
        "cordis_entries_toggle_ui: persistent state {} (run scratch {})",
        dirs.stable.display(),
        dirs.process.display()
    );
    let yaml = include_str!("../../fixtures/text-turn.cordis.yml");
    let vars = VarTable::new()
        .with_home_path(&home)
        .with_env_value("DSH_SNAPSHOT_FEEDBACK", "1");
    let composition = Composition::parse_with_vars(yaml, &vars)?;

    // Fail-closed credential gate: no key from CLI/env means the binary exits
    // here, rather than mounting the LLM port with a fabricated demo secret.
    let key_env_name = llm_key_env_name(&composition);
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    let credential = resolve_credential(
        cli_args,
        // var_os + to_string_lossy keeps a non-UTF-8 value distinguishable
        // from an unset variable (swarm review round 283).
        |name| std::env::var_os(name).map(|value| value.to_string_lossy().into_owned()),
        &key_env_name,
    )?;
    eprintln!(
        "cordis_entries_toggle_ui: DeepSeek credential supplied via {} (value not logged)",
        credential.source.label()
    );
    let port_env = llm_env(&key_env_name, &credential);

    let ctx = Context::root("entries-toggle");
    let _ = provide_tools(&ctx).await?;
    let _ = provide_fs(&ctx).await?;
    let _ = provide_skills(&ctx).await?;
    let _ = provide_subagents(&ctx).await?;
    let _ = provide_llm(&ctx).await?;
    let _ = provide_commands(&ctx).await?;
    let registry = build_registry(&ctx, &sessions, port_env);

    // Restore previously saved enable/disable choices. Use `try_load` so a
    // schema-invalid file (e.g. `{"a": "yes"}`) is flagged too: the old JSON
    // syntax-only check missed those, and `load` silently fell back to empty
    // overrides, losing the user's disable settings (swarm review R5-VU05).
    // A single `try_load` also avoids reading the file twice (TOCTOU).
    let saved = match CompositionState::try_load(&state_path) {
        Ok(saved) => saved,
        Err(error) => {
            if state_path.exists() {
                eprintln!(
                    "cordis_entries_toggle_ui: saved enable/disable choices were lost; using YAML defaults: {error}"
                );
            }
            CompositionState::default()
        }
    };

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
    fn state_dirs_use_stable_user_id_and_pid_scratch() {
        let base = test_base("dirs");
        let dirs = state_dirs_for(&base, "cordis-entries-toggle", "koppe", 4321);
        assert_eq!(dirs.stable, base.join("cordis-entries-toggle/user-koppe"));
        assert_eq!(dirs.process, base.join("cordis-entries-toggle/pid-4321"));
        assert!(dirs.stable.starts_with(&base));
        assert!(dirs.process.starts_with(&base));
        assert!(xdg_state_home().is_absolute());
    }

    #[test]
    fn saved_toggle_state_is_read_after_restart_pid_change() {
        // A restart is exactly this: same app + user, new pid. The enable/disable
        // choices saved by the old process must still be loaded by the new one.
        let base = test_base("restart");
        let before = state_dirs_for(&base, "cordis-entries-toggle", "koppe", 1000);
        let after = state_dirs_for(&base, "cordis-entries-toggle", "koppe", 2000);
        assert_eq!(before.stable, after.stable);
        assert_ne!(before.process, after.process);

        std::fs::create_dir_all(&before.stable).unwrap();
        let path = before.stable.join("entries-state.json");
        let mut saved = CompositionState::new();
        saved.set("tool-subagent", false);
        saved.save(&path).unwrap();

        // The restarted process resolves the same path via its new pid dir.
        let restored_path = after.stable.join("entries-state.json");
        assert_eq!(restored_path, path);
        let restored = CompositionState::load(&restored_path);
        assert_eq!(restored, saved);
        assert!(!restored.is_enabled("tool-subagent", true));

        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_dir(&after.stable);
        let _ = std::fs::remove_dir(after.stable.parent().unwrap());
        let _ = std::fs::remove_dir(&base);
    }

    #[test]
    fn user_id_is_sanitized_to_one_safe_component() {
        assert_eq!(sanitize_id_component("koppe"), "koppe");
        assert_eq!(sanitize_id_component("a/b c"), "a_b_c");
        assert_eq!(sanitize_id_component("../../etc"), "etc");
        assert_eq!(sanitize_id_component(".."), "unknown");
        assert_eq!(sanitize_id_component(""), "unknown");

        let base = test_base("sanitize");
        let dirs = state_dirs_for(&base, "cordis-entries-toggle", "../../etc", 7);
        assert!(
            dirs.stable.starts_with(&base),
            "user id must not escape base"
        );
        assert_eq!(dirs.stable.file_name().unwrap(), "user-etc");
    }

    #[test]
    fn prepare_state_dir_keeps_existing_files() {
        let base = test_base("preserve");
        let first = prepare_state_dir_at(&base, "cordis-entries-toggle").unwrap();
        assert!(first.stable.is_dir());
        assert!(first.process.is_dir());
        let sentinel = first.stable.join("keep-me.txt");
        std::fs::write(&sentinel, "persist").unwrap();

        let second = prepare_state_dir_at(&base, "cordis-entries-toggle").unwrap();
        assert_eq!(first, second);
        assert_eq!(
            std::fs::read_to_string(second.stable.join("keep-me.txt")).unwrap(),
            "persist"
        );

        std::fs::remove_file(&sentinel).unwrap();
        let _ = std::fs::remove_dir(&second.stable);
        let _ = std::fs::remove_dir(&second.process);
        let _ = std::fs::remove_dir(second.stable.parent().unwrap());
        let _ = std::fs::remove_dir(&base);
    }

    #[test]
    fn state_json_save_is_atomic_and_surfaces_errors() {
        let base = test_base("json");
        let dirs = prepare_state_dir_at(&base, "cordis-entries-toggle").unwrap();
        let dir = &dirs.stable;
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
        let _ = std::fs::remove_dir(dir);
        let _ = std::fs::remove_dir(&dirs.process);
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

    fn cli_args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    fn env_lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn api_key_cli_flag_wins_over_environment() {
        let resolved = resolve_credential(
            cli_args(&["--api-key", "cli-secret"]),
            env_lookup(&[("DEEPSEEK_API_KEY", "env-secret")]),
            "DEEPSEEK_API_KEY",
        )
        .unwrap();
        assert_eq!(resolved.key, "cli-secret");
        assert_eq!(resolved.source, CredentialSource::Cli);
    }

    #[test]
    fn api_key_equals_form_env_name_and_config_override_are_honoured() {
        let from_equals = resolve_credential(
            cli_args(&["--api-key=cli-secret"]),
            |_| None,
            "DEEPSEEK_API_KEY",
        )
        .unwrap();
        assert_eq!(from_equals.key, "cli-secret");

        let from_env = resolve_credential(
            cli_args(&[]),
            env_lookup(&[("CUSTOM_KEY", "custom-secret")]),
            "CUSTOM_KEY",
        )
        .unwrap();
        assert_eq!(from_env.key, "custom-secret");
        assert_eq!(
            from_env.source,
            CredentialSource::Env("CUSTOM_KEY".to_owned())
        );

        // The env name itself comes from the fixture config (`apiKeyEnv`).
        let mut config = BTreeMap::new();
        config.insert(
            "apiKeyEnv".to_owned(),
            Value::String("CUSTOM_KEY".to_owned()),
        );
        assert_eq!(
            deepseek_config(&Value::Object(config)).api_key_env,
            "CUSTOM_KEY"
        );
    }

    #[test]
    fn missing_api_key_fails_closed_without_a_placeholder() {
        let error = resolve_credential(cli_args(&[]), |_| None, "DEEPSEEK_API_KEY").unwrap_err();
        assert_eq!(
            error,
            CredentialSetupError::Missing {
                env_name: "DEEPSEEK_API_KEY".to_owned()
            }
        );
        let message = error.to_string();
        assert!(message.contains("DEEPSEEK_API_KEY"));
        assert!(
            !message.contains("sk-demo"),
            "no demo credential may be suggested"
        );
    }

    #[test]
    fn blank_cli_or_env_value_is_rejected() {
        assert_eq!(
            resolve_credential(
                cli_args(&["--api-key", "  \t "]),
                |_| None,
                "DEEPSEEK_API_KEY"
            )
            .unwrap_err(),
            CredentialSetupError::Empty {
                source: API_KEY_FLAG.to_owned()
            }
        );
        assert_eq!(
            resolve_credential(
                cli_args(&[]),
                |_| Some("   ".to_owned()),
                "DEEPSEEK_API_KEY"
            )
            .unwrap_err(),
            CredentialSetupError::Empty {
                source: "env:DEEPSEEK_API_KEY".to_owned()
            }
        );
    }

    #[test]
    fn malformed_unknown_and_duplicated_flags_are_rejected() {
        assert!(matches!(
            resolve_credential(cli_args(&["--api-key"]), |_| None, "DEEPSEEK_API_KEY").unwrap_err(),
            CredentialSetupError::MalformedFlag(_)
        ));
        assert!(matches!(
            resolve_credential(
                cli_args(&["--api-ke", "secret"]),
                |_| None,
                "DEEPSEEK_API_KEY"
            )
            .unwrap_err(),
            CredentialSetupError::UnknownFlag(_)
        ));
        assert_eq!(
            resolve_credential(
                cli_args(&["--api-key", "a", "--api-key", "b"]),
                |_| None,
                "DEEPSEEK_API_KEY"
            )
            .unwrap_err(),
            CredentialSetupError::DuplicateFlag
        );
    }

    #[test]
    fn credential_env_map_uses_the_configured_name_and_resolved_key() {
        let composition = parsed_composition("1");
        let name = llm_key_env_name(&composition);
        assert_eq!(
            name, "DEEPSEEK_API_KEY",
            "fixture has no apiKeyEnv override"
        );
        let resolved =
            resolve_credential(cli_args(&["--api-key", "real-key"]), |_| None, &name).unwrap();
        let env = llm_env(&name, &resolved);
        assert_eq!(env.len(), 1);
        assert_eq!(
            env.get("DEEPSEEK_API_KEY").map(String::as_str),
            Some("real-key")
        );
        assert!(!env.values().any(|value| value == "sk-demo"));
    }

    #[test]
    fn failed_mount_reports_are_rendered() {
        let failed = vec![("plugin-a".to_string(), "mount: boom".to_string())];
        let lines = failed_lines(&failed);
        assert_eq!(lines, vec!["(failed) plugin-a: mount: boom"]);
        assert!(failed_lines(&[]).is_empty());
    }

    fn int_config(key: &str, value: i64) -> Value {
        let mut map = BTreeMap::new();
        map.insert(key.to_owned(), Value::Integer(value));
        Value::Object(map)
    }

    #[test]
    fn numeric_config_negative_zero_and_extreme_values_keep_defaults() {
        let feedback_default = MessageFeedbackConfig::default().max_note_bytes;
        let depth_default = ToolSubagentConfig::default().max_depth;

        // Positive, in-range values are still adopted (both key spellings).
        assert_eq!(
            message_feedback_config(&int_config("maxNoteBytes", 2048)).max_note_bytes,
            2048
        );
        assert_eq!(
            message_feedback_config(&int_config("max_note_bytes", 4096)).max_note_bytes,
            4096
        );
        assert_eq!(
            tool_subagent_config(&int_config("maxDepth", 3)).max_depth,
            3
        );
        assert_eq!(
            tool_subagent_config(&int_config("max_depth", 4)).max_depth,
            4
        );

        // Negative values used to be clamped to 0 by `max(0) as usize`; the
        // port defaults must survive instead.
        for negative in [i64::MIN, -1] {
            assert_eq!(
                message_feedback_config(&int_config("maxNoteBytes", negative)).max_note_bytes,
                feedback_default
            );
            assert_eq!(
                tool_subagent_config(&int_config("maxDepth", negative)).max_depth,
                depth_default
            );
        }

        // A zero byte limit would be rejected by MessageFeedback, so its
        // default survives; a zero subagent depth is valid and is adopted,
        // matching cordis_composition_ui.rs.
        assert_eq!(
            message_feedback_config(&int_config("maxNoteBytes", 0)).max_note_bytes,
            feedback_default
        );
        assert_eq!(
            tool_subagent_config(&int_config("maxDepth", 0)).max_depth,
            0
        );

        // Extreme i64 value: adopted on 64-bit, out-of-range on 32-bit. Either
        // way it must never wrap or truncate into a small number.
        let extreme = i64::MAX;
        assert_eq!(
            message_feedback_config(&int_config("maxNoteBytes", extreme)).max_note_bytes,
            usize::try_from(extreme).unwrap_or(feedback_default)
        );
        assert_eq!(
            tool_subagent_config(&int_config("maxDepth", extreme)).max_depth,
            usize::try_from(extreme).unwrap_or(depth_default)
        );
    }

    #[test]
    fn toggle_roundtrip_keeps_slots_and_saved_state_in_sync() {
        let base = test_base("roundtrip");
        std::fs::create_dir_all(&base).unwrap();
        let state_path = base.join("entries-state.json");
        let ctx = Context::root("entries-toggle-test-roundtrip");
        let mut registry = PluginRegistry::new();
        registry.register(
            "noop",
            PluginFactory::new(move |_ctx, _config| async move { Ok::<(), cordis_rs::Error>(()) }),
        );
        let composition = Composition {
            entries: vec![cordis_rs::loader::Entry {
                id: "noop-entry".to_owned(),
                plugin: "noop".to_owned(),
                config: Value::Object(BTreeMap::new()),
                disabled: false,
                inject: Vec::new(),
            }],
            warnings: Vec::new(),
        };
        let plugin = composition.entries[0].to_plugin(&registry).unwrap();
        let fiber = futures::executor::block_on(Fiber::mount(&ctx, Box::new(plugin))).unwrap();
        assert_eq!(fiber.state(), Lifecycle::Active);

        let state = Arc::new(Mutex::new(ToggleState {
            entries: vec![EntrySlot {
                id: "noop-entry".to_owned(),
                fiber: Some(fiber),
            }],
            ctx,
            registry,
            composition,
            saved: CompositionState::new(),
            state_path: state_path.clone(),
        }));
        let mut panel = Panel {
            state,
            lines: Vec::new(),
            error: None,
            failed: Vec::new(),
        };

        // First click unloads and persists `false`.
        panel.toggle(0);
        assert!(panel.error.is_none());
        {
            let guard = panel.state.lock().expect("toggle state poisoned");
            assert!(guard.entries[0].fiber.is_none());
            assert!(!guard.saved.is_enabled("noop-entry", true));
        }
        let first: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
        assert_eq!(first["noop-entry"], serde_json::json!(false));

        // Second click remounts from the entry's factory and persists `true`.
        panel.toggle(0);
        assert!(panel.error.is_none());
        {
            let guard = panel.state.lock().expect("toggle state poisoned");
            assert_eq!(
                guard.entries[0].fiber.as_ref().map(Fiber::state),
                Some(Lifecycle::Active)
            );
            assert!(guard.saved.is_enabled("noop-entry", false));
        }
        let second: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
        assert_eq!(second["noop-entry"], serde_json::json!(true));

        std::fs::remove_file(&state_path).unwrap();
        std::fs::remove_dir(&base).unwrap();
    }

    #[test]
    fn to_plugin_failure_is_visible_and_does_not_persist_disabled() {
        let base = test_base("to-plugin-error");
        std::fs::create_dir_all(&base).unwrap();
        let state_path = base.join("entries-state.json");
        let mut saved = CompositionState::new();
        saved.set("stable", true);
        saved.save(&state_path).unwrap();
        let before = std::fs::read(&state_path).unwrap();

        // An entry pointing at an unregistered plugin makes `to_plugin` fail
        // before any mount is attempted.
        let composition = Composition {
            entries: vec![cordis_rs::loader::Entry {
                id: "ghost".to_owned(),
                plugin: "not-registered".to_owned(),
                config: Value::Object(BTreeMap::new()),
                disabled: false,
                inject: Vec::new(),
            }],
            warnings: Vec::new(),
        };
        let state = Arc::new(Mutex::new(ToggleState {
            entries: vec![EntrySlot {
                id: "ghost".to_owned(),
                fiber: None,
            }],
            ctx: Context::root("entries-toggle-test-plugin-error"),
            registry: PluginRegistry::new(),
            composition,
            saved: saved.clone(),
            state_path: state_path.clone(),
        }));
        let mut panel = Panel {
            state,
            lines: Vec::new(),
            error: None,
            failed: Vec::new(),
        };

        panel.toggle(0);
        let message = panel
            .error
            .as_ref()
            .expect("to_plugin error must be shown")
            .to_string();
        assert!(message.contains("not-registered"), "{message}");
        {
            let guard = panel.state.lock().expect("toggle state poisoned");
            assert!(guard.entries[0].fiber.is_none());
            assert!(!guard.saved.overrides.contains_key("ghost"));
            assert_eq!(
                guard.saved, saved,
                "failed toggle must not add a disable override"
            );
        }
        assert_eq!(
            std::fs::read(&state_path).unwrap(),
            before,
            "failed toggle must not rewrite the saved state"
        );

        std::fs::remove_file(&state_path).unwrap();
        std::fs::remove_dir(&base).unwrap();
    }

    #[test]
    fn unload_failure_restores_fiber_and_skips_save() {
        let base = test_base("unload-error");
        std::fs::create_dir_all(&base).unwrap();
        let state_path = base.join("entries-state.json");
        let mut saved = CompositionState::new();
        saved.set("stable", true);
        saved.save(&state_path).unwrap();
        let before = std::fs::read(&state_path).unwrap();

        let ctx = Context::root("entries-toggle-test-unload-error");
        futures::executor::block_on(async {
            let _ = provide_tools(&ctx).await.unwrap();
            let _ = provide_fs(&ctx).await.unwrap();
        });
        let fiber = futures::executor::block_on(Fiber::mount(
            &ctx,
            Box::new(cordis_rs::ports::tool_fs::ToolFs {
                config: cordis_rs::plugin_ports::ToolFsConfig::default(),
            }),
        ))
        .unwrap();
        assert_eq!(fiber.state(), Lifecycle::Active);

        // Inject a failing disposer into the fiber's own effect list so
        // `unload()` returns Err, exercising the failure branch for real.
        futures::executor::block_on(fiber.context().effect(|scope| async move {
            scope.on_dispose(|| async {
                Err::<(), cordis_rs::Error>(cordis_rs::Error::Domain(
                    "injected",
                    "injected unload failure".to_owned(),
                ))
            });
            Ok::<(), cordis_rs::Error>(())
        }))
        .unwrap();

        let state = Arc::new(Mutex::new(ToggleState {
            entries: vec![EntrySlot {
                id: "tool-fs-entry".to_owned(),
                fiber: Some(fiber),
            }],
            ctx,
            registry: PluginRegistry::new(),
            composition: Composition::default(),
            saved: saved.clone(),
            state_path: state_path.clone(),
        }));
        let mut panel = Panel {
            state,
            lines: Vec::new(),
            error: None,
            failed: Vec::new(),
        };

        panel.toggle(0);
        let message = panel
            .error
            .as_ref()
            .expect("unload error must be shown")
            .to_string();
        assert!(message.contains("injected unload failure"), "{message}");
        {
            let guard = panel.state.lock().expect("toggle state poisoned");
            assert!(
                guard.entries[0].fiber.is_some(),
                "a failed unload must keep the fiber in its slot"
            );
            assert_eq!(
                guard.saved, saved,
                "failed unload must not persist disabled"
            );
        }
        assert_eq!(
            std::fs::read(&state_path).unwrap(),
            before,
            "failed unload must not rewrite the saved state"
        );

        std::fs::remove_file(&state_path).unwrap();
        std::fs::remove_dir(&base).unwrap();
    }
}

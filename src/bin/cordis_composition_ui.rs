//! GPUI view of a real DSH composition mounted on cordis-rs.
//!
//! Loads `fixtures/text-turn.cordis.yml` (copied from the upstream SDK
//! snapshot) and boots every entry whose plugin id has a factory, then renders
//! the mounted fibers and their states in a window.

use std::collections::BTreeMap;
use std::fmt;

use cordis_rs::config::Value;
use cordis_rs::core_services::{
    provide_commands, provide_fs, provide_llm, provide_skills, provide_subagents, provide_tools,
};
use cordis_rs::loader::{Composition, PluginFactory, PluginRegistry, VarTable};
use cordis_rs::plugin_ports::{
    DEFAULT_API_KEY_ENV, DeepSeekConfig, MessageFeedbackConfig, SkillFsConfig, ToolBashConfig,
    ToolFsConfig, ToolSubagentConfig, mount_feedback_producer, mount_llm_deepseek,
    mount_message_feedback, mount_session_persistence_jsonl, mount_skill_filesystem,
    mount_tool_bash, mount_tool_fs, mount_tool_subagent, turn_stopping_event,
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

/// The two state locations used by one run.
///
/// `stable` is derived from the app id and the user, never the pid, so a
/// restarted process resolves the same directory and can read state written
/// by its predecessor. `process` is pid-scoped run scratch, so concurrent
/// runs still cannot clobber each other's temporary files.
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
        // A negative or out-of-range value is a config typo; keep the default
        // instead of clamping to zero, which MessageFeedback rejects
        // (swarm review round 283).
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

/// Lines describing mount failures so they are visible in the panel instead
/// of being silently dropped from `MountReport::failed`.
fn failed_lines(failed: &[(String, String)]) -> Vec<String> {
    failed
        .iter()
        .map(|(id, error)| format!("  (failed) {id}: {error}"))
        .collect()
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
    let dirs = prepare_state_dir("cordis-composition-ui")?;
    let home = dirs.stable.join("home");
    let sessions = dirs.stable.join("sessions");
    eprintln!(
        "cordis_composition_ui: persistent state {} (run scratch {})",
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
        "cordis_composition_ui: DeepSeek credential supplied via {} (value not logged)",
        credential.source.label()
    );
    let port_env = llm_env(&key_env_name, &credential);

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

    // Both state directories are intentionally left in place: no startup
    // wipe, so neither a concurrent run nor a later restart erases state.
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
    fn state_dirs_use_stable_user_id_and_pid_scratch() {
        let base = test_base("dirs");
        let dirs = state_dirs_for(&base, "cordis-composition-ui", "testuser", 4321);
        assert_eq!(dirs.stable, base.join("cordis-composition-ui/user-testuser"));
        assert_eq!(dirs.process, base.join("cordis-composition-ui/pid-4321"));
        assert!(dirs.stable.starts_with(&base));
        assert!(dirs.process.starts_with(&base));
        assert!(xdg_state_home().is_absolute());
    }

    #[test]
    fn stable_dir_is_unchanged_across_restart_pids() {
        // A restart is exactly this: same app + user, new pid. State written by
        // the old process must still be found by the new one.
        let base = test_base("restart");
        let before = state_dirs_for(&base, "cordis-composition-ui", "testuser", 1000);
        let after = state_dirs_for(&base, "cordis-composition-ui", "testuser", 2000);
        assert_eq!(before.stable, after.stable);
        assert_ne!(before.process, after.process);

        std::fs::create_dir_all(&before.stable).unwrap();
        let state_file = before.stable.join("entries-state.json");
        std::fs::write(&state_file, "{\"tool-subagent\":true}").unwrap();
        let reread = after.stable.join("entries-state.json");
        assert!(reread.exists(), "restarted process must see the old state");
        assert_eq!(
            std::fs::read_to_string(&reread).unwrap(),
            "{\"tool-subagent\":true}"
        );

        std::fs::remove_file(&state_file).unwrap();
        let _ = std::fs::remove_dir(&after.stable);
        let _ = std::fs::remove_dir(after.stable.parent().unwrap());
        let _ = std::fs::remove_dir(&base);
    }

    #[test]
    fn user_id_is_sanitized_to_one_safe_component() {
        assert_eq!(sanitize_id_component("testuser"), "testuser");
        assert_eq!(sanitize_id_component("a/b c"), "a_b_c");
        assert_eq!(sanitize_id_component("../../etc"), "etc");
        assert_eq!(sanitize_id_component(".."), "unknown");
        assert_eq!(sanitize_id_component(""), "unknown");

        let base = test_base("sanitize");
        let dirs = state_dirs_for(&base, "cordis-composition-ui", "../../etc", 7);
        assert!(
            dirs.stable.starts_with(&base),
            "user id must not escape base"
        );
        assert_eq!(dirs.stable.file_name().unwrap(), "user-etc");
    }

    #[test]
    fn prepare_state_dir_keeps_existing_files() {
        let base = test_base("preserve");
        let first = prepare_state_dir_at(&base, "cordis-composition-ui").unwrap();
        assert!(first.stable.is_dir());
        assert!(first.process.is_dir());
        let sentinel = first.stable.join("keep-me.txt");
        std::fs::write(&sentinel, "persist").unwrap();

        let second = prepare_state_dir_at(&base, "cordis-composition-ui").unwrap();
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
    fn negative_numeric_config_keeps_defaults() {
        let config_with = |key: &str, value: i64| {
            let mut map = std::collections::BTreeMap::new();
            map.insert(key.to_string(), Value::Integer(value));
            Value::Object(map)
        };
        let feedback = message_feedback_config(&config_with("maxNoteBytes", -1));
        assert_eq!(
            feedback.max_note_bytes,
            MessageFeedbackConfig::default().max_note_bytes
        );
        let subagent = tool_subagent_config(&config_with("maxDepth", -5));
        assert_eq!(subagent.max_depth, 1);
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
        let value_with = |text: &str| {
            let mut map = std::collections::BTreeMap::new();
            map.insert("thinking".to_string(), Value::String(text.to_string()));
            Value::Object(map)
        };
        assert!(!deepseek_config(&value_with("ENABLED")).thinking);
        assert!(!deepseek_config(&value_with("yes")).thinking);
        assert!(deepseek_config(&value_with("enabled")).thinking);

        let fallback = std::path::Path::new("/tmp/fallback-sessions");
        let root = session_root(&config_of("session-persistence-jsonl"), fallback);
        assert_ne!(root, fallback, "the YAML `root` must win over the fallback");

        let skills = skill_fs_config(&config_of("skill-filesystem"));
        assert!(skills.roots.is_empty(), "includeDefaultRoots: false");
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
        let composition = fixture_composition();
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

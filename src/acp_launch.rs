//! Shared, offline-safe helpers for locating `dsh` and building its child PATH.
//!
//! All ACP probes historically carried six identical copies of these helpers.
//! `DSH_BIN` wins, ambient `PATH` is never consulted, and every returned path
//! is absolute. This module is included from each binary with
//! `#[path = "../acp_launch.rs"] mod acp_launch;`; it is not a Cargo target.
//!
//! Redaction, stderr correlation and `flush` are shared here as well. `flush`
//! routes through `emit`, whose production fallback is `eprintln!`, so the
//! observable stdout/stderr split is unchanged across all six binaries.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// System dirs safe to expose to `dsh` and its children. `node` for the
/// `#!/usr/bin/env node` shebang lives in `/usr/bin`.
pub(crate) const MINIMAL_PATH_DIRS: &[&str] = &["/usr/local/bin", "/usr/bin", "/bin"];

/// Absolute directories searched for `dsh` when `DSH_BIN` is unset.
/// The ambient `PATH` is intentionally never consulted.
pub(crate) fn trusted_dsh_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home.filter(|home| !home.as_os_str().is_empty()) {
        dirs.push(home.join(".npm-global/bin"));
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".local/share/mise/shims"));
        dirs.push(home.join("bin"));
    }
    dirs.extend(MINIMAL_PATH_DIRS.iter().map(PathBuf::from));
    dirs
}

/// Ordered absolute candidates for `dsh`. `DSH_BIN` wins; a relative value is
/// resolved against `cwd`, so every candidate is absolute.
pub(crate) fn dsh_candidates(
    dsh_bin: Option<&Path>,
    cwd: &Path,
    search_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(bin) = dsh_bin.filter(|bin| !bin.as_os_str().is_empty()) {
        candidates.push(if bin.is_absolute() {
            bin.to_path_buf()
        } else {
            cwd.join(bin)
        });
    }
    candidates.extend(search_dirs.iter().map(|dir| dir.join("dsh")));
    candidates
}

/// Resolve `dsh` to the first existing absolute candidate.
pub(crate) fn resolve_dsh_from(
    dsh_bin: Option<&Path>,
    cwd: &Path,
    search_dirs: &[PathBuf],
) -> Result<PathBuf, String> {
    dsh_candidates(dsh_bin, cwd, search_dirs)
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "dsh executable not found in trusted locations; set DSH_BIN to its absolute path"
                .to_owned()
        })
}

/// Minimal `PATH` for the child: the resolved binary's own directory (so
/// sibling helpers and the `node` shebang resolve) plus fixed system dirs.
pub(crate) fn minimal_child_path(dsh_path: &Path) -> Result<OsString, String> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = dsh_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        dirs.push(parent.to_path_buf());
    }
    dirs.extend(MINIMAL_PATH_DIRS.iter().map(PathBuf::from));
    std::env::join_paths(&dirs).map_err(|error| format!("cannot build PATH: {error}"))
}

/// Resolve from the process environment into `(absolute dsh path, child PATH)`.
pub(crate) fn dsh_command_parts() -> Result<(PathBuf, OsString), String> {
    let dsh_bin = std::env::var_os("DSH_BIN").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let cwd = std::env::current_dir().map_err(|error| format!("cannot read cwd: {error}"))?;
    let dsh_path = resolve_dsh_from(dsh_bin.as_deref(), &cwd, &trusted_dsh_dirs(home.as_deref()))?;
    let child_path = minimal_child_path(&dsh_path)?;
    Ok((dsh_path, child_path))
}

// ---- Secret redaction -----------------------------------------------------

/// Characters that can wrap a key or a value in a URL / JSON / shell log line.
pub(crate) const REDACTION_WRAPPERS: &[char] =
    &['"', '\'', '`', '(', ')', '[', ']', '{', '}', ',', '\\'];

/// Reduce the left-hand side of a `key=value` / `key:value` token to a bare
/// key. Wrappers are stripped from both ends first, then the last URL/JSON
/// separator is dropped, so `?token`, `"api_key"`, `{"api_key"` and
/// `['access_token']` all match the same key list. Kept pure so URL / JSON /
/// quoted boundaries are regression-testable without a subprocess.
pub(crate) fn redaction_key(raw: &str) -> &str {
    let unwrapped = raw.trim_matches(|c: char| REDACTION_WRAPPERS.contains(&c));
    unwrapped
        .rfind(['?', '&', '(', '[', '{'])
        .map_or(unwrapped, |index| &unwrapped[index + 1..])
}

/// Mask credential-like values in a subprocess log line.
///
/// A pure function: the redaction policy must be testable without spawning
/// `dsh` or touching the ambient environment.
pub(crate) fn redact_secrets(line: &str) -> String {
    const SECRET_KEYS: &[&str] = &[
        "api_key",
        "api-key",
        "apikey",
        "access_token",
        "refresh_token",
        "token",
        "secret",
        "password",
        "authorization",
        "credential",
    ];
    let mut out = String::with_capacity(line.len());
    let mut tokens = line.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        if !out.is_empty() {
            out.push(' ');
        }
        let lower = token.to_ascii_lowercase();
        // `Bearer <token>` / `Authorization: Bearer <token>`.
        if lower == "bearer" && tokens.peek().is_some() {
            let _ = tokens.next();
            out.push_str("[REDACTED]");
            continue;
        }
        let mut masked = false;
        for sep in ['=', ':'] {
            if let Some((key, _value)) = token.split_once(sep) {
                let key = redaction_key(key);
                if SECRET_KEYS
                    .iter()
                    .any(|candidate| key.eq_ignore_ascii_case(candidate))
                {
                    out.push_str(key);
                    out.push(sep);
                    out.push_str("[REDACTED]");
                    masked = true;
                    break;
                }
            }
        }
        if masked {
            continue;
        }
        // Bare provider keys and AWS access key ids; wrappers around a value
        // must not hide it (`"sk-…"`, `['sk-…']`).
        let bare = lower.trim_matches(|c: char| REDACTION_WRAPPERS.contains(&c));
        if bare.starts_with("sk-") || bare.starts_with("ghp_") || bare.starts_with("gho_") {
            out.push_str("[REDACTED]");
            continue;
        }
        // `bare` comes from the lowercased token, so compare in lowercase.
        if bare.len() == 20 && bare.starts_with("akia") {
            out.push_str("[REDACTED]");
            continue;
        }
        out.push_str(token);
    }
    out
}

// ---- Correlation context --------------------------------------------------

/// Correlation context shared by the stderr drain and the ACP handshake.
///
/// `Clone` shares the same state: the handshake publishes the session id while
/// the stderr drain keeps appending lines.
#[derive(Clone)]
pub(crate) struct DshCorrelation {
    pub(crate) pid: u32,
    pub(crate) session_id: Arc<Mutex<Option<String>>>,
    pub(crate) stderr_lines: Arc<Mutex<Vec<String>>>,
    pub(crate) enabled: bool,
}

impl DshCorrelation {
    pub(crate) fn new(pid: u32) -> Self {
        Self {
            pid,
            session_id: Arc::new(Mutex::new(None)),
            stderr_lines: Arc::new(Mutex::new(Vec::new())),
            enabled: true,
        }
    }

    /// Offline sink used by the transport tests; it never prints.
    #[cfg(test)]
    pub(crate) fn offline() -> Self {
        Self {
            pid: std::process::id(),
            session_id: Arc::new(Mutex::new(None)),
            stderr_lines: Arc::new(Mutex::new(Vec::new())),
            enabled: false,
        }
    }

    pub(crate) fn set_session_id(&self, id: impl Into<String>) {
        *self.session_id.lock().unwrap() = Some(id.into());
    }

    pub(crate) fn record_stderr_line(&self, line: String) {
        self.stderr_lines.lock().unwrap().push(line);
    }

    /// Render captured stderr with the correlation id and secrets masked.
    /// Kept separate from `flush` so tests can assert the exact text.
    pub(crate) fn rendered_lines(&self) -> Vec<String> {
        let session = self
            .session_id
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| "-".to_string());
        let lines = self.stderr_lines.lock().unwrap();
        lines
            .iter()
            .map(|line| {
                format!(
                    "[dsh pid={} session={}] {}",
                    self.pid,
                    session,
                    redact_secrets(line)
                )
            })
            .collect()
    }
}

/// Drain the `dsh` stderr pipe into `correlation` line by line.
///
/// Generic over `AsyncRead` so the drain can be regression-tested with an
/// in-memory duplex stream instead of a real subprocess.
pub(crate) fn spawn_stderr_capture<R>(
    stderr: R,
    correlation: DshCorrelation,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            correlation.record_stderr_line(line);
        }
    })
}

// ---- Patch overlay and child teardown ------------------------------------

/// Validate the optional patch overlay path. An empty value means "unset";
/// a non-empty value must name an existing file, otherwise the typo would
/// silently produce a probe result without the patch (swarm review round 283).
#[allow(dead_code)] // only llm/prompt probes use the patch overlay
pub(crate) fn resolve_patch_path(
    raw: Option<OsString>,
) -> std::result::Result<Option<OsString>, String> {
    match raw {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) => {
            let path = std::path::Path::new(&value);
            if !path.is_file() {
                Err(format!("DSH_ACP_PATCH is not a file: {value:?}"))
            } else {
                Ok(Some(value))
            }
        }
    }
}

/// CLI args for the spawned `dsh`; an optional `--patch` overlay routes the
/// stage-B probe through the NVIDIA free model without editing global config.
#[allow(dead_code)] // only llm/prompt probes pass --patch
pub(crate) fn dsh_cli_args(patch: Option<&std::ffi::OsStr>) -> Vec<OsString> {
    let mut args = vec![OsString::from("--profile"), OsString::from("acp")];
    if let Some(patch) = patch {
        args.push(OsString::from("--patch"));
        args.push(patch.to_os_string());
    }
    args
}

/// Kill then wait: `wait()` reaps the child so no zombie is left behind.
/// Mirrors `shutdown_child` in `acp_probe` / `acp_multi_turn_probe`.
#[allow(dead_code)] // stream_ui owns a different teardown path
pub(crate) async fn reap_child(child: &mut tokio::process::Child) {
    let _ = child.kill().await;
    let _ = child.wait().await; // reap so no zombie is left behind
}

// ---- Output contract: one routing point for stdout vs stderr --------------

/// Result lines go to stdout; diagnostics go to stderr. `emit` is the single
/// routing point for every ACP binary (previously duplicated in three probes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeStream {
    #[allow(dead_code)] // handshake probes emit result lines; others only emit diagnostics
    Stdout,
    Stderr,
}

thread_local! {
    static OUTPUT_CAPTURE: RefCell<Option<Vec<(ProbeStream, String)>>> =
        const { RefCell::new(None) };
}

pub(crate) fn emit(stream: ProbeStream, line: &str) {
    let captured = OUTPUT_CAPTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(lines) = slot.as_mut() {
            lines.push((stream, line.to_string()));
            true
        } else {
            false
        }
    });
    if !captured {
        match stream {
            ProbeStream::Stdout => println!("{line}"),
            ProbeStream::Stderr => eprintln!("{line}"),
        }
    }
}

#[cfg(test)]
#[allow(dead_code)] // only probes with stream-capture tests call these
pub(crate) fn start_capture() {
    OUTPUT_CAPTURE.with(|slot| *slot.borrow_mut() = Some(Vec::new()));
}

#[cfg(test)]
#[allow(dead_code)] // only probes with stream-capture tests call these
pub(crate) fn take_capture() -> Vec<(ProbeStream, String)> {
    OUTPUT_CAPTURE.with(|slot| slot.borrow_mut().take().unwrap_or_default())
}

impl DshCorrelation {
    /// Correlation-tagged, redacted stderr lines share one routing point.
    pub(crate) fn flush(&self) {
        if !self.enabled {
            return;
        }
        for line in self.rendered_lines() {
            emit(ProbeStream::Stderr, &line);
        }
    }
}

/// How the graceful shutdown of the `dsh --profile acp` subprocess ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // only the two handshake probes use graceful shutdown
pub(crate) enum ShutdownOutcome {
    /// The agent exited on its own after the ACP transport was closed (EOF).
    Exited,
    /// The agent ignored EOF and was killed after the grace period elapsed.
    KilledAfterTimeout,
    /// `wait()` itself failed, so the child state is unknown.
    WaitFailed,
}

/// Close stdin, allow a bounded grace period for a clean exit, then kill+wait
/// so no zombie survives. `session/close` is unstable in the pinned ACP
/// version, so EOF on stdin is the stable wire-compatible teardown signal.
#[allow(dead_code)] // only the two handshake probes use graceful shutdown
pub(crate) async fn shutdown_child(
    child: &mut tokio::process::Child,
    grace: Duration,
) -> ShutdownOutcome {
    match tokio::time::timeout(grace, child.wait()).await {
        Ok(Ok(_status)) => ShutdownOutcome::Exited,
        Ok(Err(error)) => {
            emit(ProbeStream::Stderr, &format!("child wait failed: {error}"));
            ShutdownOutcome::WaitFailed
        }
        Err(_elapsed) => {
            emit(
                ProbeStream::Stderr,
                &format!("child did not exit within {grace:?}; killing"),
            );
            let _ = child.kill().await;
            let _ = child.wait().await; // reap so no zombie is left behind
            ShutdownOutcome::KilledAfterTimeout
        }
    }
}

// ---- Prompt resolution ----------------------------------------------------

#[allow(dead_code)] // only the three prompt-driven binaries read this env
pub(crate) const PROMPT_ENV: &str = "DSH_ACP_PROMPT";
#[allow(dead_code)] // only the three prompt-driven binaries cap input length
pub(crate) const MAX_PROMPT_CHARS: usize = 4096;

/// Join every CLI argument with one space and reject blank / control / overlong
/// input. Pure so the boundary contract can be tested without a subprocess.
#[allow(dead_code)] // only the three prompt-driven binaries sanitize CLI/env input
pub(crate) fn sanitize_prompt(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed
        .chars()
        .any(|c| c == '\0' || (c.is_control() && !matches!(c, '\n' | '\t')))
    {
        return None;
    }
    if trimmed.chars().count() > MAX_PROMPT_CHARS {
        return None;
    }
    Some(trimmed.to_owned())
}

/// Resolve CLI → environment → the caller-supplied default. Each probe keeps
/// its own `DEFAULT_PROMPT` and a two-argument local wrapper so existing tests
/// and CLI behaviour stay unchanged.
#[allow(dead_code)] // only the three prompt-driven binaries resolve prompts
pub(crate) fn resolve_prompt_with_default(
    args: &[String],
    env_prompt: Option<&str>,
    default: &str,
) -> String {
    let from_args = args.join(" ");
    if let Some(prompt) = sanitize_prompt(&from_args) {
        return prompt;
    }
    env_prompt
        .and_then(sanitize_prompt)
        .unwrap_or_else(|| default.to_owned())
}

// ---- Child environment hardening -----------------------------------------

/// Remove Node-specific injection vectors from the `dsh` child environment.
///
/// `dsh` is launched through a `#!/usr/bin/env node` shim, so a poisoned
/// `NODE_OPTIONS` / `NODE_PATH` in the ambient environment can execute code
/// before any PATH hardening takes effect. Provider credentials remain in the
/// environment — clearing it wholesale would break real model calls.
pub(crate) fn scrub_child_env(cmd: &mut tokio::process::Command) {
    cmd.env_remove("NODE_OPTIONS");
    cmd.env_remove("NODE_PATH");
}

#[cfg(test)]
mod tests {
    use super::scrub_child_env;
    use std::ffi::OsStr;

    #[test]
    fn scrub_child_env_drops_node_injection_but_keeps_credentials() {
        let mut cmd = tokio::process::Command::new("dsh");
        cmd.env("NODE_OPTIONS", "--require /tmp/payload.js")
            .env("NODE_PATH", "/tmp/modules")
            .env("DEEPSEEK_API_KEY", "placeholder-not-a-real-key");
        scrub_child_env(&mut cmd);
        // `env_remove` records the key with a `None` value; assert that exact
        // state, because a missing entry would inherit the parent's value at
        // spawn time (the regression this helper exists to prevent).
        let value = |name: &str| {
            cmd.as_std()
                .get_envs()
                .find(|(key, _)| key.to_str() == Some(name))
                .and_then(|(_, value)| value)
        };
        let removed = |name: &str| {
            cmd.as_std()
                .get_envs()
                .any(|(key, value)| key.to_str() == Some(name) && value.is_none())
        };
        assert!(removed("NODE_OPTIONS"));
        assert!(removed("NODE_PATH"));
        assert_eq!(
            value("DEEPSEEK_API_KEY"),
            Some(OsStr::new("placeholder-not-a-real-key"))
        );
    }
}

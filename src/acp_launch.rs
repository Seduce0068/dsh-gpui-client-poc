//! Shared, offline-safe helpers for locating `dsh` and building its child PATH.
//!
//! All ACP probes historically carried six identical copies of these helpers.
//! `DSH_BIN` wins, ambient `PATH` is never consulted, and every returned path
//! is absolute. This module is included from each binary with
//! `#[path = "../acp_launch.rs"] mod acp_launch;`; it is not a Cargo target.
//!
//! Redaction and stderr correlation are shared here as well; `flush` stays in
//! each binary because its output sink (`emit` vs `eprintln!`) is bin-specific.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

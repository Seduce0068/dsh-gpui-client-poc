//! Probe: send a real prompt through the ACP client to `dsh --profile acp`.
//! Requires a working model provider configuration in the ambient environment.
//!
//! The `dsh` subprocess stderr is captured through a pipe, redacted, and
//! rendered with a `[dsh pid=… session=…]` correlation id. Both the drain and
//! the redaction policy are regression-tested offline.

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion, StopReason,
    TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

// ---- Secure `dsh` launch -------------------------------------------------
//
// Never resolve `dsh` through the ambient `PATH`: a poisoned entry (`.`, a
// world-writable temp dir, an attacker-controlled bin dir) could substitute a
// fake `dsh` and capture the ACP stdio traffic. Resolve an absolute path from
// an explicit override or a fixed allowlist, then give the child a minimal,
// attacker-independent `PATH`.

/// System dirs safe to expose to `dsh` and its children. `node` for the
/// `#!/usr/bin/env node` shebang lives in `/usr/bin`.
const MINIMAL_PATH_DIRS: &[&str] = &["/usr/local/bin", "/usr/bin", "/bin"];

/// Absolute directories searched for `dsh` when `DSH_BIN` is unset.
/// The ambient `PATH` is intentionally never consulted.
fn trusted_dsh_dirs(home: Option<&Path>) -> Vec<PathBuf> {
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
fn dsh_candidates(dsh_bin: Option<&Path>, cwd: &Path, search_dirs: &[PathBuf]) -> Vec<PathBuf> {
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
fn resolve_dsh_from(
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
fn minimal_child_path(dsh_path: &Path) -> Result<OsString, String> {
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
fn dsh_command_parts() -> Result<(PathBuf, OsString), String> {
    let dsh_bin = std::env::var_os("DSH_BIN").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let cwd = std::env::current_dir().map_err(|error| format!("cannot read cwd: {error}"))?;
    let dsh_path = resolve_dsh_from(dsh_bin.as_deref(), &cwd, &trusted_dsh_dirs(home.as_deref()))?;
    let child_path = minimal_child_path(&dsh_path)?;
    Ok((dsh_path, child_path))
}

/// CLI arguments for the spawned `dsh`. An optional `--patch` overlay keeps
/// stage-B provider experiments out of the user's global profile.
fn dsh_cli_args(patch: Option<&std::ffi::OsStr>) -> Vec<OsString> {
    let mut args = vec![OsString::from("--profile"), OsString::from("acp")];
    if let Some(patch) = patch {
        args.push(OsString::from("--patch"));
        args.push(patch.to_os_string());
    }
    args
}

/// Characters that can wrap a key or a value in a URL / JSON / shell log line.
const REDACTION_WRAPPERS: &[char] = &['"', '\'', '`', '(', ')', '[', ']', '{', '}', ',', '\\'];

/// Reduce the left-hand side of a `key=value` / `key:value` token to a bare
/// key. Wrappers are stripped from both ends first, then the last URL/JSON
/// separator is dropped, so `?token`, `"api_key"`, `{"api_key"` and
/// `['access_token']` all match the same key list. Kept pure so URL / JSON /
/// quoted boundaries are regression-testable without a subprocess.
fn redaction_key(raw: &str) -> &str {
    let unwrapped = raw.trim_matches(|c: char| REDACTION_WRAPPERS.contains(&c));
    unwrapped
        .rfind(['?', '&', '(', '[', '{'])
        .map_or(unwrapped, |index| &unwrapped[index + 1..])
}

/// Validate the optional patch overlay path. An empty value means "unset";
/// a non-empty value must name an existing file, otherwise a typo would
/// silently produce a probe result without the patch. Same contract as
/// `acp_llm_probe::resolve_patch_path`.
fn resolve_patch_path(
    raw: Option<std::ffi::OsString>,
) -> std::result::Result<Option<std::ffi::OsString>, String> {
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

/// Prompt input contract mirrored from `acp_stream_ui::resolve_prompt`:
/// join every CLI argument with one space, reject blank / control / overlong
/// input, then fall back to the environment and finally the fixed prompt.
const DEFAULT_PROMPT: &str = "1+1は？";
const PROMPT_ENV: &str = "DSH_ACP_PROMPT";
const MAX_PROMPT_CHARS: usize = 4096;

fn sanitize_prompt(raw: &str) -> Option<String> {
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

fn resolve_prompt(args: &[String], env_prompt: Option<&str>) -> String {
    let from_args = args.join(" ");
    if let Some(prompt) = sanitize_prompt(&from_args) {
        return prompt;
    }
    env_prompt
        .and_then(sanitize_prompt)
        .unwrap_or_else(|| DEFAULT_PROMPT.to_owned())
}

/// Mask credential-like values in a subprocess log line.
///
/// A pure function: the redaction policy must be testable without spawning
/// `dsh` or touching the ambient environment.
fn redact_secrets(line: &str) -> String {
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

/// Correlation context shared by the stderr drain and the ACP handshake.
///
/// `Clone` shares the same state: the handshake publishes the session id while
/// the stderr drain keeps appending lines.
#[derive(Clone)]
struct DshCorrelation {
    pid: u32,
    session_id: Arc<Mutex<Option<String>>>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    enabled: bool,
}

impl DshCorrelation {
    fn new(pid: u32) -> Self {
        Self {
            pid,
            session_id: Arc::new(Mutex::new(None)),
            stderr_lines: Arc::new(Mutex::new(Vec::new())),
            enabled: true,
        }
    }

    /// Offline sink used by the transport tests; it never prints.
    #[cfg(test)]
    fn offline() -> Self {
        Self {
            pid: std::process::id(),
            session_id: Arc::new(Mutex::new(None)),
            stderr_lines: Arc::new(Mutex::new(Vec::new())),
            enabled: false,
        }
    }

    fn set_session_id(&self, id: impl Into<String>) {
        *self.session_id.lock().unwrap() = Some(id.into());
    }

    fn record_stderr_line(&self, line: String) {
        self.stderr_lines.lock().unwrap().push(line);
    }

    /// Render captured stderr with the correlation id and secrets masked.
    /// Kept separate from `flush` so tests can assert the exact text.
    fn rendered_lines(&self) -> Vec<String> {
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

    fn flush(&self) {
        if !self.enabled {
            return;
        }
        for line in self.rendered_lines() {
            eprintln!("{line}");
        }
    }
}

/// Drain the `dsh` stderr pipe into `correlation` line by line.
///
/// Generic over `AsyncRead` so the drain can be regression-tested with an
/// in-memory duplex stream instead of a real subprocess.
fn spawn_stderr_capture<R>(stderr: R, correlation: DshCorrelation) -> tokio::task::JoinHandle<()>
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

/// Diagnostic echo of the prompt on stderr. CLI/env input is untrusted, so
/// credential-looking text is masked while the `prompt => …` prefix stays.
fn format_prompt_echo(prompt: &str) -> String {
    format!("prompt => {}", redact_secrets(prompt))
}

/// Send one prompt on a fresh ACP session over an arbitrary transport,
/// publishing the negotiated session id to `correlation`.
async fn prompt_over_with_correlation(
    cwd: PathBuf,
    prompt: String,
    transport: impl ConnectTo<Client> + 'static,
    correlation: DshCorrelation,
) -> Result<StopReason, String> {
    Client
        .builder()
        .name("dsh-gpui-prompt-probe")
        .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            correlation.set_session_id(session.session_id.to_string());
            eprintln!("{}", format_prompt_echo(&prompt));
            let response = connection
                .send_request(PromptRequest::new(
                    session.session_id,
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .block_task()
                .await?;
            eprintln!("stop_reason = {:?}", response.stop_reason);
            Ok(response.stop_reason)
        })
        .await
        .map_err(|error| error.to_string())
}

/// Send one prompt on a fresh ACP session over an arbitrary transport.
///
/// The `dsh` subprocess transport and the offline `Channel::duplex()` tests
/// share this path, so the prompt seam can be exercised without a model
/// provider or credentials. Test callers keep this signature unchanged.
#[cfg(test)]
async fn prompt_over(
    cwd: PathBuf,
    prompt: String,
    transport: impl ConnectTo<Client> + 'static,
) -> Result<StopReason, String> {
    prompt_over_with_correlation(cwd, prompt, transport, DshCorrelation::offline()).await
}

/// Kill then wait: `wait()` reaps the child so no zombie is left behind.
/// Mirrors `shutdown_child` in `acp_probe` / `acp_multi_turn_probe`.
async fn reap_child(child: &mut tokio::process::Child) {
    let _ = child.kill().await;
    let _ = child.wait().await; // reap so no zombie is left behind
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env_prompt = std::env::var(PROMPT_ENV).ok();
    let prompt = resolve_prompt(&args, env_prompt.as_deref());

    let (dsh_path, child_path) = dsh_command_parts()?;
    let mut cmd = tokio::process::Command::new(&dsh_path);
    cmd.env("PATH", child_path);
    // Optional stage-B overlay (e.g. the NVIDIA free-model patch) is injected
    // without touching the user's global profile. No secret is passed here.
    let patch = resolve_patch_path(std::env::var_os("DSH_ACP_PATCH"))?;
    cmd.args(dsh_cli_args(patch.as_deref()))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;

    // All post-spawn steps are fallible; keep them in one block so even an
    // early `no pid` / `no stdio` return still reaches the reap below.
    let pipeline = async {
        let correlation = DshCorrelation::new(child.id().ok_or("no pid")?);
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let capture = spawn_stderr_capture(stderr, correlation.clone());
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
        let result =
            prompt_over_with_correlation(cwd, prompt, transport, correlation.clone()).await;
        Ok::<_, String>((capture, correlation, result))
    }
    .await;

    reap_child(&mut child).await;

    let (capture, correlation, result) = pipeline?;
    // Closing the pipe ends the drain; awaiting it guarantees every captured
    // line is present before the correlation report is printed.
    let _ = capture.await;
    correlation.flush();
    let _stop_reason = result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use agent_client_protocol::schema::{
        ContentBlock, InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
        PromptRequest, PromptResponse, SessionId, StopReason,
    };
    use agent_client_protocol::{Agent, Channel};
    use tokio::io::AsyncWriteExt;

    use super::{
        DEFAULT_PROMPT, DshCorrelation, MAX_PROMPT_CHARS, PROMPT_ENV, prompt_over, redact_secrets,
        redaction_key, resolve_patch_path, resolve_prompt, spawn_stderr_capture,
    };

    const SESSION_ID: &str = "offline-prompt-session";

    #[derive(Default)]
    struct Observed {
        /// cwd carried by the single `session/new` request.
        cwd: Mutex<Option<PathBuf>>,
        /// (session id, prompt text) seen by the fake agent.
        prompts: Mutex<Vec<(SessionId, String)>>,
        session_new_count: AtomicUsize,
    }

    /// Per-test temporary directory, removed on drop.
    ///
    /// The path combines the process id, an atomic sequence number and a
    /// nanosecond timestamp, so concurrent tests never share a directory and
    /// a reused pid across runs cannot collide either. `Drop` removes it even
    /// when a test panics, leaving no residue in the system temp directory.
    struct TestTempDir(PathBuf);

    impl TestTempDir {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let seq = NEXT.fetch_add(1, Ordering::SeqCst);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default();
            let path = std::env::temp_dir().join(format!(
                "dsh-gpui-{tag}-{}-{seq}-{nanos:x}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create per-test temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Serve a fake ACP agent on one end of an in-process duplex channel.
    ///
    /// It records the requests that traverse the channel and completes every
    /// prompt with `StopReason::EndTurn`. No subprocess, model provider, or
    /// balance is involved.
    fn spawn_fake_agent(observed: Arc<Observed>) -> agent_client_protocol::Channel {
        let (client_channel, agent_channel) = Channel::duplex();
        let observed_new = Arc::clone(&observed);
        let observed_prompt = Arc::clone(&observed);
        tokio::spawn(async move {
            let builder = Agent
                .builder()
                .name("offline-fake-agent")
                .on_receive_request(
                    async move |request: InitializeRequest, responder, _cx| {
                        responder.respond(InitializeResponse::new(request.protocol_version))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |request: NewSessionRequest, responder, _cx| {
                        *observed_new.cwd.lock().unwrap() = Some(request.cwd);
                        observed_new
                            .session_new_count
                            .fetch_add(1, Ordering::SeqCst);
                        responder.respond(NewSessionResponse::new(SessionId::from(SESSION_ID)))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |request: PromptRequest, responder, _cx| {
                        let text = match request.prompt.first() {
                            Some(ContentBlock::Text(content)) => content.text.clone(),
                            other => format!("{other:?}"),
                        };
                        observed_prompt
                            .prompts
                            .lock()
                            .unwrap()
                            .push((request.session_id, text));
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    },
                    agent_client_protocol::on_receive_request!(),
                );
            let _ = builder.connect_to(agent_channel).await;
        });
        client_channel
    }

    #[tokio::test]
    async fn prompt_reaches_fake_agent_and_returns_stop_reason() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(Arc::clone(&observed));
        let cwd = TestTempDir::new("prompt");
        let prompt = "1+1は？".to_string();

        let stop_reason = prompt_over(cwd.path().to_path_buf(), prompt.clone(), client_channel)
            .await
            .expect("offline prompt should succeed");

        // The agent's canned stop reason must come back through the client.
        assert_eq!(format!("{stop_reason:?}"), "EndTurn");

        // These values are only observable if the real requests traversed the
        // channel, so they assert the handshake and prompt really ran.
        assert_eq!(observed.session_new_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            *observed.cwd.lock().unwrap(),
            Some(cwd.path().to_path_buf())
        );
        let seen = observed.prompts.lock().unwrap().clone();
        assert_eq!(seen, vec![(SessionId::from(SESSION_ID), prompt)]);
    }

    /// Regression: every test gets its own temp dir, so parallel runs cannot
    /// share (or race on) a hardcoded `/tmp` directory, and dropping one must
    /// not delete another's or leave residue behind.
    #[test]
    fn temp_dirs_are_unique_and_removed_on_drop() {
        let first = TestTempDir::new("prompt");
        let second = TestTempDir::new("prompt");
        let first_path = first.path().to_path_buf();
        let second_path = second.path().to_path_buf();

        assert_ne!(
            first_path, second_path,
            "two tests must not receive the same cwd"
        );
        assert!(first_path.is_dir(), "cwd must be a real directory");
        assert!(second_path.is_dir(), "cwd must be a real directory");
        assert!(
            first_path.starts_with(std::env::temp_dir()),
            "cwd must live under the temp directory: {}",
            first_path.display()
        );

        drop(first);
        assert!(!first_path.exists(), "temp dir must be removed on drop");
        assert!(
            second_path.is_dir(),
            "dropping one test's dir must not remove another's"
        );
    }

    #[test]
    fn redaction_masks_credentials_but_keeps_ordinary_text() {
        assert_eq!(redact_secrets("plain message"), "plain message");

        let masked = redact_secrets("Authorization: Bearer sk-abc123");
        assert!(!masked.contains("sk-abc123"), "masked: {masked}");
        assert!(!masked.contains("Bearer sk"), "masked: {masked}");

        let masked = redact_secrets("?token=abcd");
        assert!(masked.contains("token=[REDACTED]"), "masked: {masked}");
        assert!(!masked.contains("abcd"), "masked: {masked}");

        let masked = redact_secrets("key AKIAIOSFODNN7EXAMPLE ok");
        assert!(!masked.contains("AKIA"), "masked: {masked}");
        assert!(masked.contains("ok"), "masked: {masked}");
    }

    #[test]
    fn redaction_key_strips_url_json_and_quote_wrappers() {
        assert_eq!(redaction_key("?token"), "token");
        assert_eq!(redaction_key("&access_token"), "access_token");
        assert_eq!(redaction_key("\"api_key\""), "api_key");
        assert_eq!(redaction_key("{\"api_key\""), "api_key");
        assert_eq!(redaction_key("['refresh_token'"), "refresh_token");
        assert_eq!(redaction_key("api_key"), "api_key");
    }

    #[test]
    fn redaction_masks_url_json_and_quoted_values() {
        let cases = [
            "https://host/path?api_key=sk-leaked-url",
            "\"api_key\":\"sk-leaked-json\"",
            "{'token': 'sk-leaked-quoted'}",
            "api_key=\"sk-leaked-dq\"",
            "token='abcd'",
        ];
        for case in cases {
            let masked = redact_secrets(case);
            assert!(
                masked.contains("[REDACTED]")
                    && !masked.contains("sk-leaked")
                    && !masked.contains("abcd"),
                "unmasked boundary: {case} -> {masked}"
            );
        }
        // The original `?token=` contract is unchanged.
        assert_eq!(redact_secrets("?token=abcd"), "token=[REDACTED]");
    }

    #[test]
    fn prompt_echo_is_redacted() {
        let line = super::format_prompt_echo("api_key=sk-leaked-prompt");
        assert!(line.starts_with("prompt => "), "prefix changed: {line}");
        assert!(
            line.contains("api_key=[REDACTED]") && !line.contains("sk-leaked-prompt"),
            "prompt echo leaked: {line}"
        );
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn prompt_joins_all_cli_args_with_a_space() {
        assert_eq!(
            resolve_prompt(&args(&["explain", "this", "repository"]), None),
            "explain this repository"
        );
        assert_eq!(
            resolve_prompt(&args(&["from cli"]), Some("from env")),
            "from cli"
        );
    }

    #[test]
    fn prompt_falls_back_to_environment_then_default() {
        assert_eq!(PROMPT_ENV, "DSH_ACP_PROMPT");
        assert_eq!(
            resolve_prompt(&[], Some("  describe the tests  ")),
            "describe the tests"
        );
        assert_eq!(resolve_prompt(&[], None), DEFAULT_PROMPT);
        assert_eq!(resolve_prompt(&[], Some("   ")), DEFAULT_PROMPT);
        assert_eq!(
            resolve_prompt(&args(&["   "]), Some("env prompt")),
            "env prompt"
        );
    }

    #[test]
    fn prompt_rejects_control_chars_and_overlong_input() {
        assert_eq!(
            resolve_prompt(&args(&["a\0b"]), Some("env prompt")),
            "env prompt"
        );
        assert_eq!(
            resolve_prompt(&args(&["a\u{1b}b"]), Some("env prompt")),
            "env prompt"
        );
        let long = "x".repeat(MAX_PROMPT_CHARS + 1);
        assert_eq!(resolve_prompt(&[], Some(&long)), DEFAULT_PROMPT);
        assert_eq!(
            resolve_prompt(&args(&["hello\nworld"]), None),
            "hello\nworld"
        );
    }

    #[test]
    fn missing_session_id_uses_placeholder() {
        let correlation = DshCorrelation::new(7);
        correlation.record_stderr_line("booting".to_string());
        assert_eq!(
            correlation.rendered_lines(),
            vec!["[dsh pid=7 session=-] booting"]
        );
    }

    #[tokio::test]
    async fn stderr_pipe_is_drained_and_rendered_with_correlation_id() {
        let correlation = DshCorrelation::new(4242);
        correlation.set_session_id("offline-session");
        let (mut writer, reader) = tokio::io::duplex(1024);
        let drain = spawn_stderr_capture(reader, correlation.clone());
        writer
            .write_all(b"api_key=sk-secret-value\nordinary line\n")
            .await
            .expect("write to in-memory stderr pipe");
        drop(writer);
        drain.await.expect("stderr drain task should finish");

        let rendered = correlation.rendered_lines();
        assert_eq!(rendered.len(), 2, "every stderr line is rendered");
        assert!(
            rendered[0].starts_with("[dsh pid=4242 session=offline-session]"),
            "correlation id must lead the line: {}",
            rendered[0]
        );
        assert!(
            rendered[0].contains("[REDACTED]") && !rendered[0].contains("sk-secret-value"),
            "secret must be masked: {}",
            rendered[0]
        );
        assert!(
            rendered[1].ends_with("ordinary line"),
            "ordinary text is preserved: {}",
            rendered[1]
        );
    }

    #[tokio::test]
    async fn handshake_publishes_session_id_to_correlation() {
        use super::prompt_over_with_correlation;

        let correlation = DshCorrelation::new(99);
        let client_channel = spawn_fake_agent(Arc::new(Observed::default()));
        prompt_over_with_correlation(
            TestTempDir::new("correlation").path().to_path_buf(),
            "1+1は？".to_string(),
            client_channel,
            correlation.clone(),
        )
        .await
        .expect("offline prompt should succeed");

        assert_eq!(
            *correlation.session_id.lock().unwrap(),
            Some(SESSION_ID.to_string()),
            "the negotiated ACP session id must be correlated"
        );
    }

    // ---- Secure `dsh` launch: offline regression tests (no subprocess) ----

    #[test]
    fn dsh_candidates_prefer_dsh_bin_and_stay_absolute() {
        let cwd = PathBuf::from("/work");
        let dirs = vec![PathBuf::from("/trusted/bin")];

        let from_abs = super::dsh_candidates(Some(&PathBuf::from("/opt/dsh")), &cwd, &dirs);
        assert_eq!(from_abs.first(), Some(&PathBuf::from("/opt/dsh")));

        let from_rel = super::dsh_candidates(Some(&PathBuf::from("local/dsh")), &cwd, &dirs);
        assert_eq!(from_rel.first(), Some(&PathBuf::from("/work/local/dsh")));

        let from_search = super::dsh_candidates(None, &cwd, &dirs);
        assert_eq!(from_search, vec![PathBuf::from("/trusted/bin/dsh")]);

        assert!(
            from_abs
                .iter()
                .chain(&from_rel)
                .chain(&from_search)
                .all(|path| path.is_absolute())
        );
    }

    #[test]
    fn trusted_dsh_dirs_are_absolute_and_system_backed() {
        let dirs = super::trusted_dsh_dirs(Some(&PathBuf::from("/home/operator")));
        assert!(
            dirs.iter().all(|dir| dir.is_absolute()),
            "no relative (hijackable) dir: {dirs:?}"
        );
        assert!(dirs.iter().any(|dir| dir == &PathBuf::from("/usr/bin")));
        assert!(
            dirs.iter()
                .any(|dir| dir == &PathBuf::from("/home/operator/.npm-global/bin"))
        );
        assert!(
            dirs.iter()
                .all(|dir| dir != &PathBuf::from(".") && dir != &PathBuf::from("/tmp"))
        );
    }

    #[test]
    fn resolve_dsh_from_picks_existing_file_and_reports_missing() {
        // No explicit cleanup: `TestTempDir::drop` owns removal, so an early
        // failure cannot leave a stale directory behind.
        let tmp = TestTempDir::new("pathpin-resolve");
        let fake = tmp.path().join("dsh");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").expect("write fake dsh");

        let resolved = super::resolve_dsh_from(Some(&fake), &PathBuf::from("/"), &[])
            .expect("resolve fake dsh");
        assert_eq!(resolved, fake);
        assert!(resolved.is_absolute());

        let missing =
            super::resolve_dsh_from(Some(&tmp.path().join("absent")), &PathBuf::from("/"), &[]);
        assert!(missing.is_err(), "missing DSH_BIN must be an error");
    }

    #[test]
    fn minimal_child_path_is_fixed_and_keeps_node_discoverable() {
        let path = super::minimal_child_path(&PathBuf::from("/home/operator/.npm-global/bin/dsh"))
            .expect("build minimal PATH");
        let parts: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert_eq!(
            parts.first(),
            Some(&PathBuf::from("/home/operator/.npm-global/bin"))
        );
        assert!(
            parts.iter().any(|part| part == &PathBuf::from("/usr/bin")),
            "`env node` shebang needs /usr/bin: {parts:?}"
        );
        assert!(
            parts.iter().all(|part| part.is_absolute()),
            "no empty/relative PATH entry: {parts:?}"
        );
        assert!(
            parts
                .iter()
                .all(|part| part != &PathBuf::from("/tmp") && part != &PathBuf::from("."))
        );
    }

    #[test]
    fn patch_path_validation_distinguishes_unset_from_typo() {
        assert_eq!(resolve_patch_path(None).unwrap(), None);
        assert_eq!(
            resolve_patch_path(Some(std::ffi::OsString::new())).unwrap(),
            None
        );
        let tmp = TestTempDir::new("patch");
        let missing = tmp.path().join("absent.yml");
        let error = resolve_patch_path(Some(missing.into_os_string())).unwrap_err();
        assert!(
            error.contains("DSH_ACP_PATCH is not a file"),
            "unexpected error: {error}"
        );
        let existing = tmp.path().join("patch.yml");
        std::fs::write(&existing, "profile: {}\n").expect("write patch file");
        assert_eq!(
            resolve_patch_path(Some(existing.clone().into_os_string())).unwrap(),
            Some(existing.into_os_string())
        );
    }

    #[test]
    fn patch_overlay_is_added_only_when_requested() {
        let base = super::dsh_cli_args(None);
        assert_eq!(
            base,
            vec![OsString::from("--profile"), OsString::from("acp")]
        );
        let with_patch = super::dsh_cli_args(Some(OsStr::new("/repo/fixtures/acp.patch.yml")));
        assert_eq!(
            with_patch,
            vec![
                OsString::from("--profile"),
                OsString::from("acp"),
                OsString::from("--patch"),
                OsString::from("/repo/fixtures/acp.patch.yml"),
            ]
        );
    }
}

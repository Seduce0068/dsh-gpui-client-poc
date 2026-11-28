//! Probe: keep one ACP session alive across multiple prompts.
//!
//! `dsh --profile acp` is spawned once, `initialize` + `session/new` run once,
//! then several `session/prompt` calls reuse the same session id. Verifies the
//! session-lifecycle seam that a persistent native client would build on.
//!
//! Requires a working model provider in the environment; with an exhausted
//! balance the prompts still reach ACP and return an error stop reason.
//!
//! # 出力契約 (log stream contract)
//!
//! 全ての出力は `emit` を通り、行の種別でストリームが決まる。
//! - `ProbeStream::Stdout`（`println!`）: 通常出力＝パイプ・grep 対象の結果。
//!   session id、prompt / stop_reason のトレース、完了サマリが該当する。
//! - `ProbeStream::Stderr`（`eprintln!`）: 診断出力のみ。子プロセスの警告と
//!   shutdown 結果。結果を stderr に混ぜない。

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion, SessionId,
    TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[path = "../acp_launch.rs"]
mod acp_launch;
use acp_launch::*;

// ---- Output contract: one routing point for stdout vs stderr -------------
//
// `emit` が宛先を一箇所で固定する。テストはスレッドローカルの
// `OUTPUT_CAPTURE` に差し替えて、グローバルな stdout/stderr を奪わずに
// 各呼び出し箇所のストリームを検証する（本番は None のまま）。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeStream {
    Stdout,
    Stderr,
}

thread_local! {
    static OUTPUT_CAPTURE: RefCell<Option<Vec<(ProbeStream, String)>>> =
        const { RefCell::new(None) };
}

fn emit(stream: ProbeStream, line: &str) {
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
fn start_capture() {
    OUTPUT_CAPTURE.with(|slot| *slot.borrow_mut() = Some(Vec::new()));
}

#[cfg(test)]
fn take_capture() -> Vec<(ProbeStream, String)> {
    OUTPUT_CAPTURE.with(|slot| slot.borrow_mut().take().unwrap_or_default())
}

// ---- Secure `dsh` launch -------------------------------------------------
//
// Never resolve `dsh` through the ambient `PATH`: a poisoned entry (`.`, a
// world-writable temp dir, an attacker-controlled bin dir) could substitute a
// fake `dsh` and capture the ACP stdio traffic. Resolve an absolute path from
// an explicit override or a fixed allowlist, then give the child a minimal,
// attacker-independent `PATH`.

// ---- dsh stderr capture: redaction + correlation -------------------------
//
// The child's stderr is piped and drained line by line instead of being
// inherited. Every line is masked by `redact_secrets` and rendered with a
// `[dsh pid=… session=…]` correlation id before it reaches the diagnostics
// (stderr) stream, so provider credentials printed by `dsh` never leak into
// the log. This mirrors `acp_prompt_probe.rs`; the offline tests exercise both
// the redaction policy and the drain without spawning `dsh`.

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

/// Mask credential-like values in a subprocess log line.
///
/// Pure function: the redaction policy is testable without spawning `dsh` or
/// reading any secret from the ambient environment.
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

    /// Diagnostics go to stderr through the single `emit` routing point.
    fn flush(&self) {
        if !self.enabled {
            return;
        }
        for line in self.rendered_lines() {
            emit(ProbeStream::Stderr, &line);
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

/// How the graceful shutdown of the `dsh --profile acp` subprocess ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownOutcome {
    /// The agent exited on its own after the ACP transport was closed (EOF).
    Exited,
    /// The agent ignored EOF and was killed after the grace period elapsed.
    KilledAfterTimeout,
    /// `wait()` itself failed, so the child state is unknown.
    WaitFailed,
}

/// Gracefully tear down the `dsh --profile acp` subprocess after the turn loop.
///
/// # Why no `session/close` request (unstable-feature decision)
///
/// In the pinned `agent-client-protocol` 0.11.1, `CloseSessionRequest`
/// (`session/close`) is gated behind the UNSTABLE `unstable_session_close`
/// feature and is documented as "not part of the spec yet, and may be removed
/// or changed at any point" (`agent-client-protocol-schema` `agent.rs` near
/// `CloseSessionRequest`; crate `schema/client_to_agent/requests.rs:52-53`).
/// Enabling it would require editing `Cargo.toml` (outside this task's
/// two-file scope), and `investigation/round-5.md` records that the gate was
/// renamed/removed in 5.3.0, so a `session/close` implementation would need a
/// rewrite on the next ACP upgrade. Decision: do **not** enable any unstable
/// feature (including `unstable_session_model` for a close-time cancel).
///
/// The stable alternative: once `connect_with` returns, the ACP connection and
/// its stdio transport are dropped, closing our write half of the child's
/// stdin. A stdio ACP agent treats that EOF as session teardown. We give the
/// agent a bounded grace period to exit cleanly and only escalate to `kill()`
/// as a last resort, so a hung agent cannot leak a subprocess or block the
/// probe. (`session/cancel` is stable, but all turns have already completed
/// here, so there is no in-flight work to cancel.)
async fn shutdown_child(child: &mut tokio::process::Child, grace: Duration) -> ShutdownOutcome {
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

/// 完了サマリはパイプ可能な結果なので stdout へ。
fn report_completed(prompt_count: usize) {
    emit(
        ProbeStream::Stdout,
        &format!("multi-turn probe completed: {prompt_count} prompts on one session"),
    );
}

/// teardown の成否は診断情報なので stderr へ。
fn report_shutdown(outcome: ShutdownOutcome) {
    emit(ProbeStream::Stderr, &format!("SHUTDOWN = {outcome:?}"));
}

/// Send every prompt on one ACP session over an arbitrary transport,
/// publishing the negotiated session id to `correlation` right after
/// `session/new` so stderr from any prompt turn carries the same id (same
/// shape as `acp_prompt_probe::prompt_over_with_correlation`).
async fn multi_turn_over_with_correlation(
    cwd: PathBuf,
    prompts: Vec<String>,
    transport: impl ConnectTo<Client> + 'static,
    correlation: DshCorrelation,
) -> Result<(SessionId, Vec<String>), String> {
    Client
        .builder()
        .name("cordis-acp-multi-turn-probe")
        .connect_with(
            transport,
            move |connection: ConnectionTo<Agent>| async move {
                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                let session = connection
                    .send_request(NewSessionRequest::new(cwd))
                    .block_task()
                    .await?;
                let session_id = session.session_id.clone();
                correlation.set_session_id(session_id.to_string());
                emit(ProbeStream::Stdout, &format!("session = {session_id}"));

                let mut stop_reasons = Vec::new();
                for prompt in prompts {
                    // The echo is a diagnostic trace, not a safe value: run the
                    // user-supplied prompt through the same redaction policy as
                    // the child's stderr before writing it to stdout. The wire
                    // request still carries the unmodified prompt.
                    emit(
                        ProbeStream::Stdout,
                        &format!("prompt => {}", redact_secrets(&prompt)),
                    );
                    // A model-side failure (e.g. Insufficient Balance) still
                    // proves the prompt reached this session; keep the loop going.
                    let reason = match connection
                        .send_request(PromptRequest::new(
                            session_id.clone(),
                            vec![ContentBlock::Text(TextContent::new(prompt))],
                        ))
                        .block_task()
                        .await
                    {
                        Ok(response) => format!("{:?}", response.stop_reason),
                        // Error text can embed the prompt or a credential from
                        // the provider; mask it before it reaches the trace.
                        Err(error) => format!("error: {}", redact_secrets(&error.to_string())),
                    };
                    emit(ProbeStream::Stdout, &format!("  stop_reason = {reason}"));
                    stop_reasons.push(reason);
                }
                // All prompts were sent to the same session id.
                Ok((session_id, stop_reasons))
            },
        )
        .await
        .map_err(|error| error.to_string())
}

/// Send every prompt on one ACP session over an arbitrary transport.
///
/// The `dsh` subprocess and the offline `Channel::duplex()` tests share this
/// path, so session reuse can be checked without a model provider. Test callers
/// keep this signature unchanged and use an offline correlation sink.
#[cfg(test)]
async fn multi_turn_over(
    cwd: PathBuf,
    prompts: Vec<String>,
    transport: impl ConnectTo<Client> + 'static,
) -> Result<(SessionId, Vec<String>), String> {
    multi_turn_over_with_correlation(cwd, prompts, transport, DshCorrelation::offline()).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let prompts = ["remember: blue", "what color did I say?"];

    let (dsh_path, child_path) = dsh_command_parts()?;
    let mut cmd = tokio::process::Command::new(&dsh_path);
    cmd.env("PATH", child_path);
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;

    // Capture stderr before any fallible work starts. `child.id()` is always
    // Some after a successful spawn; the fallback only keeps the correlation
    // total instead of leaking the child on an impossible None.
    let correlation = DshCorrelation::new(child.id().unwrap_or(0));
    let capture = child
        .stderr
        .take()
        .map(|stderr| spawn_stderr_capture(stderr, correlation.clone()));

    // Everything after spawn lives in one fallible block: an error in stdio
    // acquisition or in the ACP handshake must still reach the teardown below
    // instead of returning early with a live child.
    let result = async {
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

        // The session id is published inside `multi_turn_over_with_correlation`
        // immediately after `session/new`, so stderr from every prompt turn
        // carries the same correlation id instead of `session=-`.
        let (_session_id, stop_reasons) = multi_turn_over_with_correlation(
            cwd,
            prompts.iter().map(|prompt| (*prompt).to_string()).collect(),
            transport,
            correlation.clone(),
        )
        .await?;

        // Return an error instead of panicking: a panic would unwind past the
        // shutdown_child / kill + wait teardown and leak the dsh child.
        if stop_reasons.len() != 2 {
            return Err(Box::<dyn std::error::Error>::from(format!(
                "expected 2 stop reasons, got {}",
                stop_reasons.len()
            )));
        }
        report_completed(stop_reasons.len());
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;

    // All turns are done (or failed) and the ACP connection has been dropped,
    // so the agent has seen EOF on stdin. Always tear down and reap the child,
    // then drain and render the captured stderr before surfacing `result`.
    let outcome = shutdown_child(&mut child, Duration::from_secs(5)).await;
    report_shutdown(outcome);
    if let Some(capture) = capture {
        let _ = capture.await;
    }
    correlation.flush();
    result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use agent_client_protocol::schema::{
        ContentBlock, InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
        PromptRequest, PromptResponse, SessionId, StopReason,
    };
    use agent_client_protocol::{Agent, Channel};
    use tokio::io::AsyncWriteExt;

    use super::{
        DshCorrelation, ProbeStream, ShutdownOutcome, multi_turn_over,
        multi_turn_over_with_correlation, redact_secrets, redaction_key, report_completed,
        report_shutdown, shutdown_child, spawn_stderr_capture, start_capture, take_capture,
    };

    const SESSION_ID: &str = "offline-multi-turn-session";

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

    #[derive(Default)]
    struct Observed {
        /// (session id, prompt text) for each `session/prompt` request.
        prompts: Mutex<Vec<(SessionId, String)>>,
        session_new_count: AtomicUsize,
        turn: AtomicUsize,
    }

    /// Serve a fake ACP agent over an in-process duplex channel.
    ///
    /// It accepts one session and returns a distinct stop reason per turn so
    /// both prompt ordering and stop-reason collection are observable without
    /// a subprocess, model provider, or balance.
    fn spawn_fake_agent(observed: Arc<Observed>) -> agent_client_protocol::Channel {
        let (client_channel, agent_channel) = Channel::duplex();
        let observed_new = Arc::clone(&observed);
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
                    async move |_request: NewSessionRequest, responder, _cx| {
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
                        observed
                            .prompts
                            .lock()
                            .unwrap()
                            .push((request.session_id, text));
                        let turn = observed.turn.fetch_add(1, Ordering::SeqCst);
                        let reason = if turn == 0 {
                            StopReason::EndTurn
                        } else {
                            StopReason::MaxTokens
                        };
                        responder.respond(PromptResponse::new(reason))
                    },
                    agent_client_protocol::on_receive_request!(),
                );
            let _ = builder.connect_to(agent_channel).await;
        });
        client_channel
    }

    #[tokio::test]
    async fn prompts_reuse_one_session_and_keep_their_order() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(Arc::clone(&observed));
        let prompts = vec![
            "remember: blue".to_string(),
            "what color did I say?".to_string(),
        ];

        let (session_id, stop_reasons) = multi_turn_over(
            PathBuf::from("/tmp/offline-multi-turn"),
            prompts.clone(),
            client_channel,
        )
        .await
        .expect("offline multi-turn run should succeed");

        assert_eq!(session_id, SessionId::from(SESSION_ID));
        assert_eq!(observed.session_new_count.load(Ordering::SeqCst), 1);
        assert_eq!(stop_reasons, vec!["EndTurn", "MaxTokens"]);

        let seen = observed.prompts.lock().unwrap().clone();
        let expected: Vec<_> = prompts
            .iter()
            .map(|prompt| (SessionId::from(SESSION_ID), prompt.clone()))
            .collect();
        assert_eq!(seen, expected);
    }

    /// A child that exits on its own must be reported as a clean shutdown;
    /// this guards against regressing back to an unconditional `kill()`.
    #[tokio::test]
    async fn shutdown_child_reports_natural_exit() {
        let mut child = tokio::process::Command::new("true")
            .spawn()
            .expect("spawn a short-lived child");

        let outcome = shutdown_child(&mut child, Duration::from_secs(5)).await;

        assert_eq!(outcome, ShutdownOutcome::Exited);
    }

    /// A hung agent must still be terminated and reaped after the grace
    /// period, so the probe cannot leak a subprocess or hang indefinitely.
    #[tokio::test]
    async fn shutdown_child_kills_a_hung_child_after_grace() {
        let mut child = tokio::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn a long-lived child");

        let outcome = shutdown_child(&mut child, Duration::from_millis(100)).await;

        assert_eq!(outcome, ShutdownOutcome::KilledAfterTimeout);
    }

    // ---- 出力契約の回帰テスト（stdout/stderr の使い分け） ----

    /// session / prompt / stop_reason のトレースは全て結果なので stdout へ。
    #[tokio::test]
    async fn multi_turn_trace_lines_are_results_on_stdout() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(Arc::clone(&observed));
        let prompts = vec!["first".to_string(), "second".to_string()];

        start_capture();
        let (_session_id, stop_reasons) = multi_turn_over(
            PathBuf::from("/tmp/offline-multi-turn"),
            prompts.clone(),
            client_channel,
        )
        .await
        .expect("offline multi-turn run should succeed");
        let lines = take_capture();

        assert_eq!(stop_reasons, vec!["EndTurn", "MaxTokens"]);
        assert!(!lines.is_empty(), "the trace must be observable");
        assert!(
            lines
                .iter()
                .all(|(stream, _)| *stream == ProbeStream::Stdout),
            "trace lines must never go to stderr: {lines:?}"
        );
        let texts: Vec<_> = lines.iter().map(|(_, text)| text.as_str()).collect();
        assert!(
            texts.iter().any(|text| text.starts_with("session = ")),
            "session id must be a stdout result: {texts:?}"
        );
        let first_prompt = format!("prompt => {}", prompts[0]);
        assert!(
            texts.contains(&first_prompt.as_str()),
            "prompt echo must be a stdout result: {texts:?}"
        );
        assert!(
            texts.contains(&"  stop_reason = EndTurn"),
            "stop reason must be a stdout result: {texts:?}"
        );
    }

    /// The user-supplied prompt is echoed only after redaction, while the ACP
    /// request still carries the original text and the `prompt => …` /
    /// `  stop_reason = …` trace contract is unchanged. The error branch uses
    /// the same redaction policy before emitting `error: …`.
    #[tokio::test]
    async fn prompt_echo_is_redacted_but_request_keeps_original_text() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(Arc::clone(&observed));
        let raw_prompt = "api_key=sk-leaked-prompt remember this".to_string();

        start_capture();
        let (_session_id, stop_reasons) = multi_turn_over(
            PathBuf::from("/tmp/offline-multi-turn"),
            vec![raw_prompt.clone()],
            client_channel,
        )
        .await
        .expect("offline multi-turn run should succeed");
        let lines = take_capture();

        assert_eq!(stop_reasons, vec!["EndTurn"]);
        let texts: Vec<_> = lines.iter().map(|(_, text)| text.as_str()).collect();
        assert!(
            texts.contains(&"prompt => api_key=[REDACTED] remember this"),
            "echo must be redacted but keep prefix and ordinary words: {texts:?}"
        );
        assert!(
            texts.iter().all(|text| !text.contains("sk-leaked-prompt")),
            "raw credential must never reach stdout: {texts:?}"
        );
        assert!(
            texts.contains(&"  stop_reason = EndTurn"),
            "stop_reason trace contract must be unchanged: {texts:?}"
        );
        assert!(
            lines
                .iter()
                .all(|(stream, _)| *stream == ProbeStream::Stdout),
            "trace lines must stay on stdout: {lines:?}"
        );

        let seen = observed.prompts.lock().unwrap().clone();
        assert_eq!(
            seen,
            vec![(SessionId::from(SESSION_ID), raw_prompt)],
            "the ACP request must carry the unmodified prompt"
        );

        // Same policy on the error branch of the stop_reason trace.
        let masked = redact_secrets("error: api_key=sk-leaked-error");
        assert!(
            masked.contains("[REDACTED]") && !masked.contains("sk-leaked-error"),
            "error text must be masked: {masked}"
        );
    }

    /// 完了サマリは結果（stdout）、shutdown は診断（stderr）。
    #[test]
    fn summary_is_stdout_and_shutdown_is_stderr() {
        start_capture();
        report_completed(2);
        report_shutdown(ShutdownOutcome::Exited);
        let lines = take_capture();

        assert_eq!(
            lines,
            vec![
                (
                    ProbeStream::Stdout,
                    "multi-turn probe completed: 2 prompts on one session".to_string()
                ),
                (ProbeStream::Stderr, "SHUTDOWN = Exited".to_string()),
            ]
        );
    }

    /// kill に至る警告も診断なので stderr に留まる。
    #[tokio::test]
    async fn shutdown_child_warning_stays_on_stderr() {
        start_capture();
        let mut child = tokio::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn a long-lived child");

        let outcome = shutdown_child(&mut child, Duration::from_millis(100)).await;
        let lines = take_capture();

        assert_eq!(outcome, ShutdownOutcome::KilledAfterTimeout);
        assert!(!lines.is_empty(), "kill path must emit a diagnostic");
        assert!(
            lines
                .iter()
                .all(|(stream, _)| *stream == ProbeStream::Stderr),
            "shutdown diagnostics must never pollute stdout: {lines:?}"
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

    /// Regression: each test gets its own temp dir, and dropping one must not
    /// delete another test's dir or leave its own behind.
    #[test]
    fn temp_dirs_are_unique_and_removed_on_drop() {
        let first = TestTempDir::new("isolation");
        let second = TestTempDir::new("isolation");
        let first_path = first.path().to_path_buf();
        let second_path = second.path().to_path_buf();

        assert_ne!(
            first_path, second_path,
            "parallel tests must not share a temp dir"
        );
        assert!(first_path.is_dir(), "temp dir must be a real directory");
        assert!(second_path.is_dir(), "temp dir must be a real directory");

        drop(first);
        assert!(!first_path.exists(), "temp dir must be removed on drop");
        assert!(
            second_path.is_dir(),
            "dropping one test's dir must not remove another's"
        );
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

    // ---- dsh stderr capture: redaction + correlation (offline) ------------

    /// The redaction policy masks credentials but keeps ordinary diagnostics.
    #[test]
    fn redaction_masks_credentials_but_keeps_ordinary_text() {
        assert_eq!(redact_secrets("plain message"), "plain message");

        let masked = redact_secrets("api_key=sk-secret-value ordinary");
        assert!(masked.contains("api_key=[REDACTED]"), "masked: {masked}");
        assert!(!masked.contains("sk-secret-value"), "masked: {masked}");
        assert!(masked.contains("ordinary"), "masked: {masked}");

        let masked = redact_secrets("Authorization: Bearer sk-abc123");
        assert!(!masked.contains("sk-abc123"), "masked: {masked}");
    }

    /// URL / JSON / quoted boundaries must reduce to the same bare key list.
    #[test]
    fn redaction_key_strips_url_json_and_quote_wrappers() {
        assert_eq!(redaction_key("?token"), "token");
        assert_eq!(redaction_key("&access_token"), "access_token");
        assert_eq!(redaction_key("\"api_key\""), "api_key");
        assert_eq!(redaction_key("{\"api_key\""), "api_key");
        assert_eq!(redaction_key("['refresh_token'"), "refresh_token");
        assert_eq!(redaction_key("api_key"), "api_key");
    }

    /// Credentials wrapped in URL queries, JSON quotes or shell quotes must
    /// never survive into a diagnostic line.
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

    /// The offline sink used by transport tests never emits diagnostics.
    #[test]
    fn offline_correlation_never_emits() {
        let correlation = DshCorrelation::offline();
        correlation.set_session_id("offline-session");
        correlation.record_stderr_line("api_key=sk-would-leak".to_string());
        start_capture();
        correlation.flush();
        assert!(
            take_capture().is_empty(),
            "offline correlation must not print"
        );
    }

    /// The session id must be published as soon as `session/new` answers, not
    /// after all prompts, so stderr from the first turn is already correlated.
    #[tokio::test]
    async fn handshake_publishes_session_id_to_correlation() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(Arc::clone(&observed));
        let correlation = DshCorrelation::offline();

        multi_turn_over_with_correlation(
            PathBuf::from("/tmp/offline-multi-turn"),
            vec!["first".to_string()],
            client_channel,
            correlation.clone(),
        )
        .await
        .expect("offline multi-turn run should succeed");

        assert_eq!(
            *correlation.session_id.lock().unwrap(),
            Some(SESSION_ID.to_string()),
            "the negotiated ACP session id must be correlated"
        );
    }

    /// A piped stderr is drained line by line and rendered with the pid and
    /// the session id published by the handshake, with secrets masked.
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
}

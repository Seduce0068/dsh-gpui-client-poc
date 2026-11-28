//! Probe: drive a cordis-rs agent loop through a real ACP backend.
//!
//! `dsh --profile acp` is spawned as a subprocess and wrapped in an LLM
//! provider via `from_async_text`. This verifies the adapter seam; a full
//! session-persistent ACP client is future work.
//!
//! Requires a working model provider configuration in the environment.
//!
//! Failures are classified into balance / rate-limit / auth-expired /
//! disconnect before being reported, so the operator sees a class-specific
//! recovery step instead of one generic string (`errors-not-classified`, G7).
//! The ACP transport itself has no retry/backoff; the separate
//! `cordis_retry_ui` demo exercises cordis agent-loop retry with a local stub
//! and is NOT this ACP path.
//!
//! The `dsh` subprocess stderr is captured through a pipe, redacted, and
//! rendered with a `[dsh pid=… session=…]` correlation id.
//!
//! # 出力契約 (log stream contract)
//!
//! 全ての出力は `emit` を通り、行の種別でストリームが決まる。
//! - `ProbeStream::Stdout`（`println!`）: 通常出力＝パイプ・grep 対象の結果。
//!   agent loop の step 数と回答本文が該当する。
//! - `ProbeStream::Stderr`（`eprintln!`）: 診断出力のみ。失敗の class /
//!   recovery hint、および相関 id 付き・redact 済みの `dsh` stderr。
//!   `rendered_lines()` の書式と redaction は変更しない契約であり、
//!   その全行が stderr へ出ることを回帰テストで固定する。

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion,
    SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use cordis_rs::Context;
use cordis_rs::core_services::{
    AgentLoop, LlmRequest, from_async_text, provide_llm, provide_tools,
};
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

impl DshCorrelation {
    fn flush(&self) {
        if !self.enabled {
            return;
        }
        // 相関 id 付きの redact 済み行は診断出力なので stderr 固定。
        for line in self.rendered_lines() {
            emit(ProbeStream::Stderr, &line);
        }
    }
}

/// Operator-facing class of an ACP / model-provider failure.
///
/// ACP (`error.to_string()`) and the cordis LLM provider both surface failures
/// as opaque strings, so classification is text based. It is a pure function
/// so the recovery guidance can be regression-tested without a live backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcpFailureKind {
    Balance,
    RateLimited,
    AuthExpired,
    Disconnected,
    Unknown,
}

impl AcpFailureKind {
    fn classify(message: &str) -> Self {
        let text = message.to_ascii_lowercase();
        const BALANCE: &[&str] = &[
            "insufficient balance",
            "insufficient credit",
            "insufficient funds",
            "insufficient_quota",
            "payment required",
            "billing",
            "402",
            "残高不足",
        ];
        const RATE_LIMIT: &[&str] = &["429", "rate limit", "too many requests", "throttl", "quota"];
        const AUTH: &[&str] = &[
            "auth",
            "unauthor",
            "forbidden",
            "credential",
            "token",
            "expired",
            "401",
            "403",
        ];
        const DISCONNECT: &[&str] = &[
            "disconnect",
            "connection",
            "broken pipe",
            "eof",
            "no stdin",
            "no stdout",
            "spawn",
            "transport",
            "timed out",
            "timeout",
            "reset by peer",
        ];
        // Specific causes first: a balance message may also mention "request"
        // or "limit", and an auth failure may follow a reconnect attempt.
        if BALANCE.iter().any(|needle| text.contains(needle)) {
            Self::Balance
        } else if RATE_LIMIT.iter().any(|needle| text.contains(needle)) {
            Self::RateLimited
        } else if AUTH.iter().any(|needle| text.contains(needle)) {
            Self::AuthExpired
        } else if DISCONNECT.iter().any(|needle| text.contains(needle)) {
            Self::Disconnected
        } else {
            Self::Unknown
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Balance => "balance/402",
            Self::RateLimited => "rate-limit/429",
            Self::AuthExpired => "auth-expired",
            Self::Disconnected => "disconnected",
            Self::Unknown => "unknown",
        }
    }

    /// Next step for the operator. Each class has a distinct hint: the old
    /// behaviour rendered all four as the same generic string.
    fn recovery(self) -> &'static str {
        match self {
            Self::Balance => {
                "top up the model account or switch provider, then rerun (not an ACP transport fault)"
            }
            Self::RateLimited => {
                "wait for the rate-limit window or lower request rate; the ACP client has no retry/backoff"
            }
            Self::AuthExpired => "refresh the credentials/API key in the `acp` profile, then rerun",
            Self::Disconnected => {
                "check `dsh --profile acp` is on PATH and its stdio pipe is alive, then rerun"
            }
            Self::Unknown => "inspect the raw error below; no automatic recovery hint",
        }
    }
}

/// Render a failure with its class and next step. Pure so tests can assert
/// that every class carries a distinct, actionable recovery line.
///
/// Backend errors are raw ACP / model text, so the message is redacted before
/// it is embedded; masking only the message preserves the two-line layout.
fn format_failure(message: &str) -> String {
    let kind = AcpFailureKind::classify(message);
    // Redact per line so a multi-line backend error keeps its original layout.
    let safe_message = message
        .split_inclusive('\n')
        .map(|line| {
            if let Some(body) = line.strip_suffix('\n') {
                format!("{}\n", redact_secrets(body))
            } else {
                redact_secrets(line)
            }
        })
        .collect::<String>();
    format!(
        "[{}] {safe_message}\n  recovery: {}",
        kind.label(),
        kind.recovery()
    )
}

/// 失敗の class / recovery hint は診断情報なので stderr へ。
/// 書式・ストリーム・failure exit status は不変（生エラーのみ redact 済み）。
fn report_failure(error: &str) {
    emit(
        ProbeStream::Stderr,
        &format!("acp_llm_probe: {}", format_failure(error)),
    );
}

/// Payload carried by `main`'s `Err`. Rust's `Termination` prints that value
/// to stderr as well, so raw ACP/model tokens must be masked here too.
fn failure_error_payload(error: &str) -> String {
    redact_secrets(error)
}

/// step 数と回答本文はパイプ可能な結果なので stdout へ。
fn report_success(steps: usize, body: &str) {
    emit(ProbeStream::Stdout, &format!("steps: {steps}"));
    emit(ProbeStream::Stdout, &format!("acp body: {body}"));
}

/// Run the ACP handshake and a single prompt over an arbitrary transport,
/// publishing the negotiated session id to `correlation`.
///
/// `PromptResponse::stop_reason` is a completion signal, not the answer body.
/// The text is delivered through `session/update` notifications, so we collect
/// `AgentMessageChunk` text and stop only when the turn's response arrives.
async fn acp_prompt_over_with_correlation(
    prompt: String,
    transport: impl ConnectTo<Client> + 'static,
    correlation: DshCorrelation,
) -> std::result::Result<String, String> {
    let chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let capture = Arc::clone(&chunks);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));

    Client
        .builder()
        .name("cordis-acp-llm-probe")
        .on_receive_notification(
            move |notification: SessionNotification, _cx| {
                let capture = Arc::clone(&capture);
                async move {
                    if let SessionUpdate::AgentMessageChunk(chunk) = &notification.update
                        && let ContentBlock::Text(text) = &chunk.content
                    {
                        capture.lock().unwrap().push(text.text.clone());
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
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
            let _response = connection
                .send_request(PromptRequest::new(
                    session.session_id,
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .block_task()
                .await?;
            let text = chunks.lock().unwrap().join("");
            Ok(text)
        })
        .await
        .map_err(|error| error.to_string())
}

/// Run the ACP handshake and a single prompt over an arbitrary transport,
/// returning the agent's message text.
///
/// The `dsh` subprocess transport and the offline `Channel::duplex()` tests
/// share this path, so the adapter seam can be exercised without a live
/// provider. Test callers keep this signature unchanged.
#[cfg(test)]
async fn acp_prompt_over(
    prompt: String,
    transport: impl ConnectTo<Client> + 'static,
) -> std::result::Result<String, String> {
    acp_prompt_over_with_correlation(prompt, transport, DshCorrelation::offline()).await
}

/// Ask the ACP agent one prompt in a fresh session and return its message body.
async fn acp_prompt(prompt: String) -> std::result::Result<String, String> {
    let (dsh_path, child_path) = dsh_command_parts()?;
    let mut cmd = tokio::process::Command::new(&dsh_path);
    cmd.env("PATH", child_path);
    let patch = resolve_patch_path(std::env::var_os("DSH_ACP_PATCH"))?;
    cmd.args(dsh_cli_args(patch.as_deref()))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|error| error.to_string())?;

    // Everything after spawn lives in one fallible block, so even `no pid` /
    // `no stdio` early returns still fall through to the reap below.
    let pipeline = async {
        let correlation = DshCorrelation::new(child.id().ok_or("no pid")?);
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let capture = spawn_stderr_capture(stderr, correlation.clone());
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
        let body = acp_prompt_over_with_correlation(prompt, transport, correlation.clone()).await;
        Ok::<_, String>((capture, correlation, body))
    }
    .await;

    reap_child(&mut child).await;

    let (capture, correlation, body) = pipeline?;
    // Closing the pipe ends the drain; awaiting it guarantees every captured
    // line is present before the correlation report is printed.
    let _ = capture.await;
    correlation.flush();
    body
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::root("acp-llm-probe");
    let (tools, _) = provide_tools(&ctx).await?;
    let llm = provide_llm(&ctx).await?;

    let provider = from_async_text("acp", "dsh-acp", |request: LlmRequest| async move {
        let last = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.clone())
            .unwrap_or_default();
        acp_prompt(last).await
    });
    let _p = llm.register(provider);
    llm.set_default("acp");

    // A single-step run: the agent asks once, the ACP backend answers with a
    // stop reason. No tool calls are registered here, so the loop ends.
    let loop_ = AgentLoop::new(llm, tools).with_call_parser(Arc::new(|_text: &str| Vec::new()));
    let run = match loop_.run("1+1は？").await {
        Ok(run) => run,
        Err(error) => {
            // Provider/ACP failures reach here as opaque strings. Print the
            // classified recovery hint, then return a redacted payload: Rust's
            // Termination prints the `Err` value to stderr too. The failure
            // exit status is unchanged.
            let raw = error.to_string();
            report_failure(&raw);
            return Err(failure_error_payload(&raw).into());
        }
    };
    report_success(run.steps.len(), &run.final_text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use agent_client_protocol::schema::{
        ContentBlock, ContentChunk, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptRequest, PromptResponse, SessionId, SessionNotification,
        SessionUpdate, StopReason, TextContent,
    };
    use agent_client_protocol::{Agent, Channel, Client, ConnectionTo};
    use tokio::io::AsyncWriteExt;

    use super::{
        AcpFailureKind, DshCorrelation, ProbeStream, acp_prompt_over,
        acp_prompt_over_with_correlation, failure_error_payload, format_failure, redact_secrets,
        redaction_key, report_failure, report_success, spawn_stderr_capture, start_capture,
        take_capture,
    };

    const SESSION_ID: &str = "probe-session";

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

    /// Serve a fake ACP agent over one end of an in-process duplex channel.
    ///
    /// It streams the supplied text chunks for every prompt and then completes
    /// the turn with `StopReason::EndTurn`. No model provider or subprocess is
    /// involved.
    fn spawn_fake_agent(chunks: &'static [&'static str]) -> agent_client_protocol::Channel {
        let (client_channel, agent_channel) = Channel::duplex();
        tokio::spawn(async move {
            let builder = Agent
                .builder()
                .name("fake-acp-agent")
                .on_receive_request(
                    async move |request: InitializeRequest, responder, _cx| {
                        responder.respond(InitializeResponse::new(request.protocol_version))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |_request: NewSessionRequest, responder, _cx| {
                        responder.respond(NewSessionResponse::new(SessionId::from(SESSION_ID)))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |_request: PromptRequest, responder, cx: ConnectionTo<Client>| {
                        for chunk in chunks {
                            cx.send_notification(SessionNotification::new(
                                SessionId::from(SESSION_ID),
                                SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                    ContentBlock::Text(TextContent::new(*chunk)),
                                )),
                            ))?;
                        }
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    },
                    agent_client_protocol::on_receive_request!(),
                );
            // `connect_to` serves until the channel closes; dropping the test's
            // runtime aborts this task.
            let _ = builder.connect_to(agent_channel).await;
        });
        client_channel
    }

    #[tokio::test]
    async fn body_is_streamed_text_not_stop_reason() {
        let client_channel = spawn_fake_agent(&["2+2", "=4"]);

        let body = acp_prompt_over("1+1は？".to_string(), client_channel)
            .await
            .expect("offline prompt should succeed");

        assert_eq!(body, "2+2=4", "body must be the streamed agent text");
        assert!(
            !body.contains("EndTurn"),
            "stop reason must never leak into the body"
        );
    }

    #[tokio::test]
    async fn body_is_empty_when_agent_streams_no_text() {
        let client_channel = spawn_fake_agent(&[]);

        let body = acp_prompt_over("1+1は？".to_string(), client_channel)
            .await
            .expect("offline prompt should succeed");

        assert_eq!(
            body, "",
            "a stop reason alone must not be returned as the body"
        );
    }

    #[test]
    fn failures_are_classified_into_four_distinct_kinds() {
        let cases = [
            // The balance error actually seen against `dsh --profile acp`.
            ("Error: Insufficient Balance", AcpFailureKind::Balance),
            ("HTTP 429 rate limited", AcpFailureKind::RateLimited),
            (
                "AuthRequired: token expired (401)",
                AcpFailureKind::AuthExpired,
            ),
            (
                "ACP transport disconnected: broken pipe",
                AcpFailureKind::Disconnected,
            ),
            ("some brand new failure", AcpFailureKind::Unknown),
        ];
        for (message, expected) in cases {
            assert_eq!(
                AcpFailureKind::classify(message),
                expected,
                "unexpected class for {message}"
            );
        }
    }

    #[test]
    fn every_failure_kind_has_a_distinct_actionable_hint() {
        let hints: Vec<_> = [
            AcpFailureKind::Balance,
            AcpFailureKind::RateLimited,
            AcpFailureKind::AuthExpired,
            AcpFailureKind::Disconnected,
            AcpFailureKind::Unknown,
        ]
        .into_iter()
        .map(|kind| (kind, kind.recovery()))
        .collect();
        let unique: std::collections::BTreeSet<_> = hints.iter().map(|(_, hint)| *hint).collect();
        assert_eq!(unique.len(), hints.len(), "hints must be class-specific");
        for (kind, hint) in hints {
            assert!(!hint.is_empty(), "missing hint for {kind:?}");
        }
    }

    #[test]
    fn formatted_failure_carries_class_and_next_step() {
        let report = format_failure("Insufficient Balance");
        assert!(report.contains("[balance/402]"), "report: {report}");
        assert!(report.contains("Insufficient Balance"), "report: {report}");
        assert!(
            report.contains("recovery: top up"),
            "balance errors must get a top-up step: {report}"
        );
        assert!(
            !format_failure("HTTP 429 rate limited").contains("top up"),
            "429 must not reuse the balance hint"
        );
    }

    /// Regression for parallel-test isolation: this probe has no fixed `/tmp`
    /// path (it builds `session/new` from `current_dir()`), but concurrent
    /// offline runs must still keep their streamed bodies separate.
    #[tokio::test]
    async fn concurrent_offline_prompts_stay_isolated() {
        let first = spawn_fake_agent(&["2+2", "=4"]);
        let second = spawn_fake_agent(&["3+3", "=6"]);

        let (first_body, second_body) = tokio::join!(
            acp_prompt_over("1+1は？".to_string(), first),
            acp_prompt_over("2+2は？".to_string(), second),
        );

        assert_eq!(
            first_body.expect("first offline prompt should succeed"),
            "2+2=4"
        );
        assert_eq!(
            second_body.expect("second offline prompt should succeed"),
            "3+3=6"
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
    fn failure_report_masks_raw_model_error_before_output() {
        let raw = "auth failure: api_key=sk-leaked-failure";
        let formatted = format_failure(raw);
        assert!(
            formatted.contains("api_key=[REDACTED]") && !formatted.contains("sk-leaked-failure"),
            "raw error leaked into failure text: {formatted}"
        );

        start_capture();
        report_failure(raw);
        let lines = take_capture();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, ProbeStream::Stderr);
        assert!(
            !lines[0].1.contains("sk-leaked-failure") && lines[0].1.contains("[REDACTED]"),
            "failure output leaked: {}",
            lines[0].1
        );
    }

    #[test]
    fn failure_exit_payload_carries_no_raw_token() {
        let payload = failure_error_payload("AuthRequired: token=sk-leaked-exit");
        assert!(
            !payload.contains("sk-leaked-exit") && payload.contains("token=[REDACTED]"),
            "raw token leaked into Err payload: {payload}"
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
        let correlation = DshCorrelation::new(99);
        let client_channel = spawn_fake_agent(&[]);
        acp_prompt_over_with_correlation(
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

    // ---- 出力契約の回帰テスト（stdout/stderr の使い分け） ----

    /// 相関 id 付き・redact 済みの `dsh` stderr は全行 stderr へ出る。
    /// `rendered_lines()` の書式と redaction 契約は変更しない。
    #[test]
    fn correlated_dsh_stderr_stays_on_stderr_and_is_redacted() {
        let correlation = DshCorrelation::new(555);
        correlation.set_session_id("session-x");
        correlation.record_stderr_line("api_key=sk-hidden".to_string());
        correlation.record_stderr_line("ordinary line".to_string());

        start_capture();
        correlation.flush();
        let lines = take_capture();

        assert_eq!(lines.len(), 2, "every captured line is rendered once");
        assert!(
            lines
                .iter()
                .all(|(stream, _)| *stream == ProbeStream::Stderr),
            "correlated diagnostics must never pollute stdout: {lines:?}"
        );
        assert_eq!(
            lines[0],
            (
                ProbeStream::Stderr,
                "[dsh pid=555 session=session-x] api_key=[REDACTED]".to_string()
            )
        );
        assert_eq!(
            lines[1],
            (
                ProbeStream::Stderr,
                "[dsh pid=555 session=session-x] ordinary line".to_string()
            )
        );
        assert!(!lines[0].1.contains("sk-hidden"), "secret leaked");
    }

    /// 成功結果は stdout、失敗の recovery hint は stderr（従来の文言を維持）。
    #[test]
    fn results_go_to_stdout_and_failure_to_stderr() {
        start_capture();
        report_success(3, "the answer");
        report_failure("Insufficient Balance");
        let lines = take_capture();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], (ProbeStream::Stdout, "steps: 3".to_string()));
        assert_eq!(
            lines[1],
            (ProbeStream::Stdout, "acp body: the answer".to_string())
        );
        assert_eq!(lines[2].0, ProbeStream::Stderr);
        assert!(
            lines[2].1.starts_with("acp_llm_probe: "),
            "failure prefix changed: {}",
            lines[2].1
        );
        assert!(
            lines[2].1.contains("[balance/402]") && lines[2].1.contains("recovery: top up"),
            "classification contract changed: {}",
            lines[2].1
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

    #[test]
    fn patch_path_validation_distinguishes_unset_from_typo() {
        assert_eq!(super::resolve_patch_path(None).unwrap(), None);
        assert_eq!(
            super::resolve_patch_path(Some(std::ffi::OsString::new())).unwrap(),
            None
        );
        assert!(
            super::resolve_patch_path(Some(std::ffi::OsString::from(
                "/definitely/missing/patch.yml"
            )))
            .is_err()
        );
        let existing = std::env::temp_dir().join(format!("acp-patch-{}.yml", std::process::id()));
        std::fs::write(
            &existing,
            "profile: {}
",
        )
        .unwrap();
        assert_eq!(
            super::resolve_patch_path(Some(existing.clone().into_os_string())).unwrap(),
            Some(existing.clone().into_os_string())
        );
        let _ = std::fs::remove_file(existing);
    }

    #[test]
    fn patch_overlay_is_added_only_when_requested() {
        use std::ffi::{OsStr, OsString};
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

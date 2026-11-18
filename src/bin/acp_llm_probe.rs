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
                let key = key.trim_start_matches(['?', '&', '(', '[']);
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
        // Bare provider keys and AWS access key ids.
        if lower.starts_with("sk-") || lower.starts_with("ghp_") || lower.starts_with("gho_") {
            out.push_str("[REDACTED]");
            continue;
        }
        if token.len() == 20 && token.as_bytes().starts_with(b"AKIA") {
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
fn format_failure(message: &str) -> String {
    let kind = AcpFailureKind::classify(message);
    format!(
        "[{}] {message}\n  recovery: {}",
        kind.label(),
        kind.recovery()
    )
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
    let mut cmd = tokio::process::Command::new("dsh");
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|error| error.to_string())?;
    let correlation = DshCorrelation::new(child.id().ok_or("no pid")?);
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let capture = spawn_stderr_capture(stderr, correlation.clone());
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

    let body = acp_prompt_over_with_correlation(prompt, transport, correlation.clone()).await;
    let _ = child.kill().await;
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
            // classified recovery hint before exiting; keep returning the
            // original error so the probe's failure exit status is unchanged.
            eprintln!("acp_llm_probe: {}", format_failure(&error.to_string()));
            return Err(error.into());
        }
    };
    println!("steps: {}", run.steps.len());
    println!("acp body: {}", run.final_text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::{
        ContentBlock, ContentChunk, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptRequest, PromptResponse, SessionId, SessionNotification,
        SessionUpdate, StopReason, TextContent,
    };
    use agent_client_protocol::{Agent, Channel, Client, ConnectionTo};
    use tokio::io::AsyncWriteExt;

    use super::{
        AcpFailureKind, DshCorrelation, acp_prompt_over, acp_prompt_over_with_correlation,
        format_failure, redact_secrets, spawn_stderr_capture,
    };

    const SESSION_ID: &str = "probe-session";

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
}

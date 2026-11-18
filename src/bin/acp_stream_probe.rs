//! Probe: capture ACP `session/update` chunks from `dsh --profile acp`.
//!
//! Registers a notification listener that prints each agent message chunk as
//! it arrives. With a working model provider the chunks stream live; with an
//! exhausted balance the turn fails before producing chunks. Either way the
//! notification wiring is exercised.
//!
//! The `dsh` subprocess stderr is captured through a pipe, redacted, and
//! rendered with a `[dsh pid=… session=…]` correlation id. Both the drain and
//! the redaction policy are regression-tested offline.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion,
    SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
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

/// Run one prompt over an arbitrary transport, collect the agent message
/// chunks delivered through `session/update`, and publish the negotiated
/// session id to `correlation`.
async fn stream_prompt_over_with_correlation(
    cwd: PathBuf,
    prompt: String,
    transport: impl ConnectTo<Client> + 'static,
    correlation: DshCorrelation,
) -> Result<(Vec<String>, String), String> {
    let chunks: Arc<AsyncMutex<Vec<String>>> = Arc::new(AsyncMutex::new(Vec::new()));
    let capture = Arc::clone(&chunks);

    let outcome = Client
        .builder()
        .name("cordis-acp-stream-probe")
        .on_receive_notification(
            move |notification: SessionNotification, _cx| {
                let capture = Arc::clone(&capture);
                async move {
                    if let SessionUpdate::AgentMessageChunk(chunk) = &notification.update
                        && let ContentBlock::Text(text) = &chunk.content
                    {
                        println!("chunk: {}", text.text);
                        capture.lock().await.push(text.text.clone());
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
            match connection
                .send_request(PromptRequest::new(
                    session.session_id,
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .block_task()
                .await
            {
                Ok(response) => Ok(format!("stop_reason = {:?}", response.stop_reason)),
                Err(error) => Ok(format!("prompt error (model-side): {error}")),
            }
        })
        .await
        .map_err(|error| error.to_string())?;

    let collected = chunks.lock().await.clone();
    Ok((collected, outcome))
}

/// Run one prompt over an arbitrary transport and collect the agent message
/// chunks delivered through `session/update` notifications.
///
/// The `dsh` subprocess transport and the offline `Channel::duplex()` tests
/// share this path, so the notification wiring can be checked without a model
/// provider or balance. The second return value is the outcome line printed by
/// the probe, so a model-side failure stays observable. Test callers keep this
/// signature unchanged.
#[cfg(test)]
async fn stream_prompt_over(
    cwd: PathBuf,
    prompt: String,
    transport: impl ConnectTo<Client> + 'static,
) -> Result<(Vec<String>, String), String> {
    stream_prompt_over_with_correlation(cwd, prompt, transport, DshCorrelation::offline()).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1+1은?".to_string());

    let mut cmd = tokio::process::Command::new("dsh");
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let correlation = DshCorrelation::new(child.id().ok_or("no pid")?);
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let capture = spawn_stderr_capture(stderr, correlation.clone());
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

    let result =
        stream_prompt_over_with_correlation(cwd, prompt, transport, correlation.clone()).await;
    let _ = child.kill().await;
    // Closing the pipe ends the drain; awaiting it guarantees every captured
    // line is present before the correlation report is printed.
    let _ = capture.await;
    correlation.flush();
    let (collected, outcome) = result?;
    println!("{outcome}");
    println!("captured {} chunk(s)", collected.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use agent_client_protocol::schema::{
        ContentBlock, ContentChunk, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptRequest, PromptResponse, SessionId, SessionNotification,
        SessionUpdate, StopReason, TextContent,
    };
    use agent_client_protocol::{Agent, Channel, Client, ConnectionTo};
    use tokio::io::AsyncWriteExt;

    use super::{
        DshCorrelation, redact_secrets, spawn_stderr_capture, stream_prompt_over,
        stream_prompt_over_with_correlation,
    };

    const SESSION_ID: &str = "offline-stream-session";

    #[derive(Default)]
    struct Observed {
        cwd: Mutex<Option<PathBuf>>,
        /// (session id, prompt text) seen by the fake agent.
        prompt: Mutex<Option<(SessionId, String)>>,
    }

    /// Per-test temporary cwd, removed on drop.
    ///
    /// A shared fixed `/tmp/...` path can collide between parallel tests, so
    /// the path is made unique with the process id plus a call counter.
    struct TestCwd(PathBuf);

    impl TestCwd {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let seq = NEXT.fetch_add(1, Ordering::SeqCst);
            let path =
                std::env::temp_dir().join(format!("dsh-gpui-{tag}-{}-{seq}", std::process::id()));
            std::fs::create_dir_all(&path).expect("create per-test temp cwd");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestCwd {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Serve a fake ACP agent over an in-process duplex channel.
    ///
    /// It streams the supplied text chunks as `session/update` notifications,
    /// records the prompt request, then completes with `StopReason::EndTurn`.
    /// No subprocess, model provider, or balance is involved.
    fn spawn_fake_agent(
        chunks: &'static [&'static str],
        observed: Arc<Observed>,
    ) -> agent_client_protocol::Channel {
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
                        responder.respond(NewSessionResponse::new(SessionId::from(SESSION_ID)))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |request: PromptRequest, responder, cx: ConnectionTo<Client>| {
                        let text = match request.prompt.first() {
                            Some(ContentBlock::Text(content)) => content.text.clone(),
                            other => format!("{other:?}"),
                        };
                        *observed_prompt.prompt.lock().unwrap() = Some((request.session_id, text));
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
            let _ = builder.connect_to(agent_channel).await;
        });
        client_channel
    }

    #[tokio::test]
    async fn agent_message_chunks_are_captured_in_order() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(&["2+2", "=4"], Arc::clone(&observed));
        let cwd = TestCwd::new("stream");
        let prompt = "1+1은?".to_string();

        let (collected, outcome) =
            stream_prompt_over(cwd.path().to_path_buf(), prompt.clone(), client_channel)
                .await
                .expect("offline stream run should succeed");

        assert_eq!(collected, vec!["2+2".to_string(), "=4".to_string()]);
        assert_eq!(outcome, "stop_reason = EndTurn");

        // These values are only observable if the real requests traversed the
        // channel, so they assert the handshake and prompt really ran.
        assert_eq!(
            *observed.cwd.lock().unwrap(),
            Some(cwd.path().to_path_buf())
        );
        assert_eq!(
            *observed.prompt.lock().unwrap(),
            Some((SessionId::from(SESSION_ID), prompt))
        );
    }

    #[tokio::test]
    async fn no_chunks_are_captured_when_agent_streams_no_text() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(&[], Arc::clone(&observed));
        let cwd = TestCwd::new("stream-empty");

        let (collected, outcome) = stream_prompt_over(
            cwd.path().to_path_buf(),
            "1+1은?".to_string(),
            client_channel,
        )
        .await
        .expect("offline stream run should succeed");

        assert!(collected.is_empty());
        assert_eq!(outcome, "stop_reason = EndTurn");
        assert_eq!(
            *observed.cwd.lock().unwrap(),
            Some(cwd.path().to_path_buf())
        );
    }

    /// Regression: every test gets its own temp cwd, so parallel runs cannot
    /// share (or race on) a hardcoded `/tmp` directory.
    #[test]
    fn per_test_cwds_are_unique_and_real() {
        let first = TestCwd::new("stream");
        let second = TestCwd::new("stream");

        assert_ne!(
            first.path(),
            second.path(),
            "two tests must not receive the same cwd"
        );
        assert!(first.path().is_dir(), "cwd must be a real directory");
        assert!(second.path().is_dir(), "cwd must be a real directory");
        assert!(
            first.path().starts_with(std::env::temp_dir()),
            "cwd must live under the temp directory: {}",
            first.path().display()
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
        let client_channel = spawn_fake_agent(&[], Arc::new(Observed::default()));
        let cwd = TestCwd::new("correlation");
        stream_prompt_over_with_correlation(
            cwd.path().to_path_buf(),
            "1+1은?".to_string(),
            client_channel,
            correlation.clone(),
        )
        .await
        .expect("offline stream run should succeed");

        assert_eq!(
            *correlation.session_id.lock().unwrap(),
            Some(SESSION_ID.to_string()),
            "the negotiated ACP session id must be correlated"
        );
    }
}

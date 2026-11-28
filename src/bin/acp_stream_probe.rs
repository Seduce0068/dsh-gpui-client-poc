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
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion,
    SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[path = "../acp_launch.rs"]
mod acp_launch;
use acp_launch::*;

// ---- Secure `dsh` launch -------------------------------------------------
//
// Never resolve `dsh` through the ambient `PATH`: a poisoned entry (`.`, a
// world-writable temp dir, an attacker-controlled bin dir) could substitute a
// fake `dsh` and capture the ACP stdio traffic. Resolve an absolute path from
// an explicit override or a fixed allowlist, then give the child a minimal,
// attacker-independent `PATH`.

/// Render a raw model-side prompt error for stdout, masking any credential
/// text the ACP backend echoed back. Pure so the regression is testable
/// without a live model provider.
fn model_error_outcome(error: &str) -> String {
    format!("prompt error (model-side): {}", redact_secrets(error))
}

/// Prompt input contract shared with the other probes through `acp_launch`:
/// join every CLI argument with one space, reject blank / control / overlong
/// input, then fall back to the environment and finally this fixed prompt.
/// `PROMPT_ENV`, `MAX_PROMPT_CHARS`, `sanitize_prompt` and the resolver are
/// provided by the `acp_launch::*` glob import above.
const DEFAULT_PROMPT: &str = "1+1은?";

/// Two-argument shim so existing callers and tests keep their signature while
/// the validation logic stays in one shared implementation.
fn resolve_prompt(args: &[String], env_prompt: Option<&str>) -> String {
    resolve_prompt_with_default(args, env_prompt, DEFAULT_PROMPT)
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
                Err(error) => Ok(model_error_outcome(&error.to_string())),
            }
        })
        .await
        .map_err(|error| redact_secrets(&error.to_string()))?;

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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env_prompt = std::env::var(PROMPT_ENV).ok();
    let prompt = resolve_prompt(&args, env_prompt.as_deref());

    let (dsh_path, child_path) = dsh_command_parts()
        .map_err(|error| Box::<dyn std::error::Error>::from(redact_secrets(&error)))?;
    let mut cmd = tokio::process::Command::new(&dsh_path);
    cmd.env("PATH", child_path);
    scrub_child_env(&mut cmd);
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|error| Box::<dyn std::error::Error>::from(redact_secrets(&error.to_string())))?;

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
            stream_prompt_over_with_correlation(cwd, prompt, transport, correlation.clone()).await;
        Ok::<_, String>((capture, correlation, result))
    }
    .await;

    reap_child(&mut child).await;

    let (capture, correlation, result) = pipeline
        .map_err(|error| Box::<dyn std::error::Error>::from(redact_secrets(&error.to_string())))?;
    // Closing the pipe ends the drain; awaiting it guarantees every captured
    // line is present before the correlation report is printed.
    let _ = capture.await;
    correlation.flush();
    let (collected, outcome) = result
        .map_err(|error| Box::<dyn std::error::Error>::from(redact_secrets(&error.to_string())))?;
    // Final stdout guard: never print a raw ACP/model outcome line.
    println!("{}", redact_secrets(&outcome));
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
        DEFAULT_PROMPT, DshCorrelation, MAX_PROMPT_CHARS, PROMPT_ENV, model_error_outcome,
        redact_secrets, redaction_key, resolve_prompt, spawn_stderr_capture, stream_prompt_over,
        stream_prompt_over_with_correlation,
    };

    const SESSION_ID: &str = "offline-stream-session";

    #[derive(Default)]
    struct Observed {
        cwd: Mutex<Option<PathBuf>>,
        /// (session id, prompt text) seen by the fake agent.
        prompt: Mutex<Option<(SessionId, String)>>,
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
        let cwd = TestTempDir::new("stream");
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
        let cwd = TestTempDir::new("stream-empty");

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

    /// Regression: every test gets its own temp dir, so parallel runs cannot
    /// share (or race on) a hardcoded `/tmp` directory, and dropping one must
    /// not delete another's or leave residue behind.
    #[test]
    fn temp_dirs_are_unique_and_removed_on_drop() {
        let first = TestTempDir::new("stream");
        let second = TestTempDir::new("stream");
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
    fn model_error_outcome_is_redacted_before_stdout() {
        let line = model_error_outcome("auth failure: api_key=sk-leaked-model");
        assert!(
            line.starts_with("prompt error (model-side): "),
            "prefix contract changed: {line}"
        );
        assert!(
            line.contains("api_key=[REDACTED]") && !line.contains("sk-leaked-model"),
            "model error leaked: {line}"
        );
        assert_eq!(
            redact_secrets("prompt error (model-side): api_key=sk-leaked-print"),
            "prompt error (model-side): api_key=[REDACTED]"
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
        let correlation = DshCorrelation::new(99);
        let client_channel = spawn_fake_agent(&[], Arc::new(Observed::default()));
        let cwd = TestTempDir::new("correlation");
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake)
                .expect("metadata fake dsh")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).expect("chmod fake dsh");
        }

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
}

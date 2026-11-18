//! Probe: capture ACP `session/update` chunks from `dsh --profile acp`.
//!
//! Registers a notification listener that prints each agent message chunk as
//! it arrives. With a working model provider the chunks stream live; with an
//! exhausted balance the turn fails before producing chunks. Either way the
//! notification wiring is exercised.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion,
    SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Run one prompt over an arbitrary transport and collect the agent message
/// chunks delivered through `session/update` notifications.
///
/// The `dsh` subprocess transport and the offline `Channel::duplex()` tests
/// share this path, so the notification wiring can be checked without a model
/// provider or balance. The second return value is the outcome line printed by
/// the probe, so a model-side failure stays observable.
async fn stream_prompt_over(
    cwd: PathBuf,
    prompt: String,
    transport: impl ConnectTo<Client> + 'static,
) -> Result<(Vec<String>, String), String> {
    let chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1+1은?".to_string());

    let mut cmd = tokio::process::Command::new("dsh");
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

    let (collected, outcome) = stream_prompt_over(cwd, prompt, transport).await?;
    println!("{outcome}");
    println!("captured {} chunk(s)", collected.len());
    let _ = child.kill().await;
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

    use super::stream_prompt_over;

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
}

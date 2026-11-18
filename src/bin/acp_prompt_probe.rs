//! Probe: send a real prompt through the ACP client to `dsh --profile acp`.
//! Requires a working model provider configuration in the ambient environment.

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion, StopReason,
    TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use std::path::PathBuf;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Send one prompt on a fresh ACP session over an arbitrary transport.
///
/// The `dsh` subprocess transport and the offline `Channel::duplex()` tests
/// share this path, so the prompt seam can be exercised without a model
/// provider or credentials.
async fn prompt_over(
    cwd: PathBuf,
    prompt: String,
    transport: impl ConnectTo<Client> + 'static,
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
            eprintln!("prompt => {}", prompt);
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1+1は？".to_string());

    let mut cmd = tokio::process::Command::new("dsh");
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;

    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
    let _stop_reason = prompt_over(cwd, prompt, transport).await?;

    let _ = child.kill().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use agent_client_protocol::schema::{
        ContentBlock, InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
        PromptRequest, PromptResponse, SessionId, StopReason,
    };
    use agent_client_protocol::{Agent, Channel};

    use super::prompt_over;

    const SESSION_ID: &str = "offline-prompt-session";

    #[derive(Default)]
    struct Observed {
        /// cwd carried by the single `session/new` request.
        cwd: Mutex<Option<PathBuf>>,
        /// (session id, prompt text) seen by the fake agent.
        prompts: Mutex<Vec<(SessionId, String)>>,
        session_new_count: AtomicUsize,
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
        let cwd = TestCwd::new("prompt");
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

    /// Regression: every test gets its own temp cwd, so parallel runs cannot
    /// share (or race on) a hardcoded `/tmp` directory.
    #[test]
    fn per_test_cwds_are_unique_and_real() {
        let first = TestCwd::new("prompt");
        let second = TestCwd::new("prompt");

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

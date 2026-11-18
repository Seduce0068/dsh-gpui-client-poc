//! Probe: keep one ACP session alive across multiple prompts.
//!
//! `dsh --profile acp` is spawned once, `initialize` + `session/new` run once,
//! then several `session/prompt` calls reuse the same session id. Verifies the
//! session-lifecycle seam that a persistent native client would build on.
//!
//! Requires a working model provider in the environment; with an exhausted
//! balance the prompts still reach ACP and return an error stop reason.

use std::path::PathBuf;
use std::time::Duration;

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion, SessionId,
    TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

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
            eprintln!("child wait failed: {error}");
            ShutdownOutcome::WaitFailed
        }
        Err(_elapsed) => {
            eprintln!("child did not exit within {grace:?}; killing");
            let _ = child.kill().await;
            let _ = child.wait().await; // reap so no zombie is left behind
            ShutdownOutcome::KilledAfterTimeout
        }
    }
}

/// Send every prompt on one ACP session over an arbitrary transport.
///
/// The `dsh` subprocess and the offline `Channel::duplex()` tests share this
/// path, so session reuse can be checked without a model provider.
async fn multi_turn_over(
    cwd: PathBuf,
    prompts: Vec<String>,
    transport: impl ConnectTo<Client> + 'static,
) -> Result<(SessionId, Vec<String>), String> {
    Client
        .builder()
        .name("cordis-acp-multi-turn-probe")
        .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            let session_id = session.session_id.clone();
            println!("session = {session_id}");

            let mut stop_reasons = Vec::new();
            for prompt in prompts {
                println!("prompt => {prompt}");
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
                    Err(error) => format!("error: {error}"),
                };
                println!("  stop_reason = {reason}");
                stop_reasons.push(reason);
            }
            // All prompts were sent to the same session id.
            Ok((session_id, stop_reasons))
        })
        .await
        .map_err(|error| error.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let prompts = ["remember: blue", "what color did I say?"];

    let mut cmd = tokio::process::Command::new("dsh");
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

    let (session_id, stop_reasons) = multi_turn_over(
        cwd,
        prompts.iter().map(|prompt| (*prompt).to_string()).collect(),
        transport,
    )
    .await?;

    assert_eq!(stop_reasons.len(), 2);
    println!(
        "multi-turn probe completed: {} prompts on one session",
        stop_reasons.len()
    );
    let _ = session_id;

    // All turns are done and the ACP connection has been dropped, so the
    // agent has seen EOF on stdin. Wait for a clean exit before killing.
    let outcome = shutdown_child(&mut child, Duration::from_secs(5)).await;
    eprintln!("SHUTDOWN = {outcome:?}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use agent_client_protocol::schema::{
        ContentBlock, InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
        PromptRequest, PromptResponse, SessionId, StopReason,
    };
    use agent_client_protocol::{Agent, Channel};

    use super::{ShutdownOutcome, multi_turn_over, shutdown_child};

    const SESSION_ID: &str = "offline-multi-turn-session";

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
}

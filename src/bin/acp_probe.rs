//! Probe: verify a Rust ACP client can initialize and open a session on
//! `dsh --profile acp`. No prompt is sent, so no model credentials are used.

use agent_client_protocol::schema::{
    InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse, ProtocolVersion,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use std::path::PathBuf;
use std::time::Duration;
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

/// Gracefully tear down the `dsh --profile acp` subprocess.
///
/// # Why no `session/close` request (unstable-feature decision)
///
/// In the pinned `agent-client-protocol` 0.11.1, `CloseSessionRequest`
/// (`session/close`) only exists behind the `unstable_session_close` feature.
/// The schema marks it **UNSTABLE**: "not part of the spec yet, and may be
/// removed or changed at any point" (see `agent-client-protocol-schema`
/// `agent.rs` near `CloseSessionRequest`, and the crate's
/// `schema/client_to_agent/requests.rs:52-53`). Enabling it would mean editing
/// `Cargo.toml` (outside this task's two-file scope), and the project's own
/// dependency triage (`investigation/round-5.md`) records that the gate was
/// renamed/removed in 5.3.0, so a `session/close`-based implementation would
/// have to be rewritten on the next ACP upgrade. Decision: do **not** enable
/// any unstable feature.
///
/// The stable, wire-compatible alternative used here: when `connect_with`
/// returns, the ACP connection and its stdio transport are dropped, which
/// closes our write half of the child's stdin. A stdio ACP agent observes that
/// EOF as session teardown (cancel in-flight work, release session resources).
/// We then allow a bounded grace period for a clean exit and only escalate to
/// `kill()` as a last resort, so a hung agent can neither leak a subprocess
/// nor block this probe forever. (`session/cancel` is stable, but this probe
/// sends no prompts, so there is no in-flight turn to cancel.)
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

/// Run the ACP handshake (`initialize` + `session/new`) over any transport.
///
/// The `dsh` subprocess transport and the offline `Channel::duplex()` tests
/// share this path, so the seam the probe checks can be exercised without a
/// model provider or credentials.
async fn initialize_and_open_session(
    cwd: PathBuf,
    transport: impl ConnectTo<Client> + 'static,
) -> Result<(InitializeResponse, NewSessionResponse), String> {
    Client
        .builder()
        .name("dsh-gpui-probe")
        .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            Ok((init, session))
        })
        .await
        .map_err(|error| error.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    eprintln!("cwd = {}", cwd.display());

    let mut cmd = tokio::process::Command::new("dsh");
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;

    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
    let (init, session) = initialize_and_open_session(cwd, transport).await?;
    eprintln!("INIT OK: agent_info={:?}", init.agent_info);
    eprintln!("SESSION OK: id={:?}", session.session_id);

    // The ACP connection (and with it the stdio transport / our stdin write
    // half) was dropped when `initialize_and_open_session` returned, so the
    // agent has already seen EOF. Wait for a clean exit before killing.
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
        Implementation, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptRequest, PromptResponse, ProtocolVersion, SessionId, StopReason,
    };
    use agent_client_protocol::{Agent, Channel};

    use super::{ShutdownOutcome, initialize_and_open_session, shutdown_child};

    const SESSION_ID: &str = "offline-probe-session";
    const AGENT_NAME: &str = "offline-fake-agent";

    #[derive(Default)]
    struct Observed {
        protocol_version: Mutex<Option<ProtocolVersion>>,
        cwd: Mutex<Option<PathBuf>>,
        prompt_count: AtomicUsize,
    }

    /// Serve a fake ACP agent on one end of an in-process duplex channel.
    ///
    /// It records what the client sent and replies with a fixed session id.
    /// No subprocess, model provider, or balance is involved. A prompt
    /// handler is registered only to detect (and fail) an unexpected prompt.
    fn spawn_fake_agent(observed: Arc<Observed>) -> agent_client_protocol::Channel {
        let (client_channel, agent_channel) = Channel::duplex();
        let observed_init = Arc::clone(&observed);
        let observed_new = Arc::clone(&observed);
        tokio::spawn(async move {
            let builder = Agent
                .builder()
                .name(AGENT_NAME)
                .on_receive_request(
                    async move |request: InitializeRequest, responder, _cx| {
                        let version = request.protocol_version;
                        *observed_init.protocol_version.lock().unwrap() = Some(version.clone());
                        responder.respond(
                            InitializeResponse::new(version)
                                .agent_info(Implementation::new(AGENT_NAME, "0.0.0")),
                        )
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
                    async move |_request: PromptRequest, responder, _cx| {
                        observed.prompt_count.fetch_add(1, Ordering::SeqCst);
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    },
                    agent_client_protocol::on_receive_request!(),
                );
            let _ = builder.connect_to(agent_channel).await;
        });
        client_channel
    }

    #[tokio::test]
    async fn initialize_and_new_session_work_over_duplex_channel() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(Arc::clone(&observed));
        let cwd = PathBuf::from("/tmp/offline-acp-probe");

        let (init, session) = initialize_and_open_session(cwd.clone(), client_channel)
            .await
            .expect("offline handshake should succeed");

        assert_eq!(init.protocol_version, ProtocolVersion::V1);
        assert_eq!(
            init.agent_info.as_ref().map(|info| info.name.as_str()),
            Some(AGENT_NAME)
        );
        assert_eq!(session.session_id, SessionId::from(SESSION_ID));

        // The fake agent only observes these values if the real requests
        // traversed the channel, so these assert the handshake really ran.
        assert_eq!(
            *observed.protocol_version.lock().unwrap(),
            Some(ProtocolVersion::V1)
        );
        assert_eq!(*observed.cwd.lock().unwrap(), Some(cwd));

        // This probe must stay free of model calls.
        assert_eq!(observed.prompt_count.load(Ordering::SeqCst), 0);
    }

    /// Returning from `connect_with` drops the ACP transport and with it our
    /// stdin write half. A child blocked on stdin must then see EOF and exit
    /// on its own. This is the stable session-close mechanism `shutdown_child`
    /// relies on instead of the unstable `session/close` request, so a
    /// regression that keeps the pipe open must fail this test.
    #[tokio::test]
    async fn dropping_acp_transport_delivers_stdin_eof_to_child() {
        use agent_client_protocol::{ByteStreams, Client};
        use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("spawn a child that blocks on stdin");
        let stdin = child.stdin.take().expect("child has a piped stdin");
        let transport = ByteStreams::new(stdin.compat_write(), tokio::io::empty().compat());

        // The exact result is irrelevant; what matters is that returning from
        // `connect_with` drops the transport and closes the child's stdin.
        let _ = Client
            .builder()
            .name("acp-probe-eof-test")
            .connect_with(transport, |_cx| async { Ok(()) })
            .await;

        let outcome = shutdown_child(&mut child, Duration::from_secs(2)).await;
        assert_eq!(outcome, ShutdownOutcome::Exited);
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

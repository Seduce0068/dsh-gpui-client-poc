//! Probe: verify a Rust ACP client can initialize and open a session on
//! `dsh --profile acp`. No prompt is sent, so no model credentials are used.

use agent_client_protocol::schema::{
    InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse, ProtocolVersion,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use std::path::PathBuf;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

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

    let _ = child.kill().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use agent_client_protocol::schema::{
        Implementation, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptRequest, PromptResponse, ProtocolVersion, SessionId, StopReason,
    };
    use agent_client_protocol::{Agent, Channel};

    use super::initialize_and_open_session;

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
}

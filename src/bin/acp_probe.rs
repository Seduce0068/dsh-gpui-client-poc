//! Probe: verify a Rust ACP client can initialize and open a session on
//! `dsh --profile acp`. No prompt is sent, so no model credentials are used.

use agent_client_protocol::schema::{InitializeRequest, NewSessionRequest, ProtocolVersion};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};
use std::path::PathBuf;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

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

    Client
        .builder()
        .name("dsh-gpui-probe")
        .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            eprintln!("INIT OK: agent_info={:?}", init.agent_info);

            let session = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            eprintln!("SESSION OK: id={:?}", session.session_id);
            Ok(())
        })
        .await?;

    let _ = child.kill().await;
    Ok(())
}

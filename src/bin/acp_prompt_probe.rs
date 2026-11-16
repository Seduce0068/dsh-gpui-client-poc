//! Probe: send a real prompt through the ACP client to `dsh --profile acp`.
//! Requires a working model provider configuration in the ambient environment.

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};
use std::path::PathBuf;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

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
            Ok(())
        })
        .await?;

    let _ = child.kill().await;
    Ok(())
}

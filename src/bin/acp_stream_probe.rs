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
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1+1은?".to_string());

    let chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let capture = chunks.clone();

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
        .name("cordis-acp-stream-probe")
        .on_receive_notification(
            move |notification: SessionNotification, _cx| {
                let capture = capture.clone();
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
                Ok(response) => println!("stop_reason = {:?}", response.stop_reason),
                Err(error) => println!("prompt error (model-side): {error}"),
            }
            Ok(())
        })
        .await?;

    let collected = chunks.lock().await;
    println!("captured {} chunk(s)", collected.len());
    let _ = child.kill().await;
    Ok(())
}

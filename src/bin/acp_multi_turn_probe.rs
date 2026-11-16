//! Probe: keep one ACP session alive across multiple prompts.
//!
//! `dsh --profile acp` is spawned once, `initialize` + `session/new` run once,
//! then several `session/prompt` calls reuse the same session id. Verifies the
//! session-lifecycle seam that a persistent native client would build on.
//!
//! Requires a working model provider in the environment; with an exhausted
//! balance the prompts still reach ACP and return an error stop reason.

use std::path::PathBuf;

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

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

    let results = Client
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
            // Both prompts were sent to the same session id.
            Ok((session_id, stop_reasons))
        })
        .await?;

    assert_eq!(results.1.len(), 2);
    println!(
        "multi-turn probe completed: {} prompts on one session",
        results.1.len()
    );
    let _ = results.0;
    let _ = child.kill().await;
    Ok(())
}

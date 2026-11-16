//! Probe: drive a cordis-rs agent loop through a real ACP backend.
//!
//! `dsh --profile acp` is spawned as a subprocess and wrapped in an LLM
//! provider via `from_async_text`. This verifies the adapter seam; a full
//! session-persistent ACP client is future work.
//!
//! Requires a working model provider configuration in the environment.

use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};
use cordis_rs::Context;
use cordis_rs::core_services::{
    AgentLoop, LlmRequest, from_async_text, provide_llm, provide_tools,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Ask the ACP agent one prompt in a fresh session and return its stop reason.
async fn acp_prompt(prompt: String) -> std::result::Result<String, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let mut cmd = tokio::process::Command::new("dsh");
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|error| error.to_string())?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

    let stop_reason = Client
        .builder()
        .name("cordis-acp-llm-probe")
        .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            let response = connection
                .send_request(PromptRequest::new(
                    session.session_id,
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .block_task()
                .await?;
            Ok(format!("{:?}", response.stop_reason))
        })
        .await
        .map_err(|error| error.to_string())?;
    let _ = child.kill().await;
    Ok(stop_reason)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::root("acp-llm-probe");
    let (tools, _) = provide_tools(&ctx).await?;
    let llm = provide_llm(&ctx).await?;

    let provider = from_async_text("acp", "dsh-acp", |request: LlmRequest| async move {
        let last = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.clone())
            .unwrap_or_default();
        acp_prompt(last).await
    });
    let _p = llm.register(provider);
    llm.set_default("acp");

    // A single-step run: the agent asks once, the ACP backend answers with a
    // stop reason. No tool calls are registered here, so the loop ends.
    let loop_ = AgentLoop::new(llm, tools).with_call_parser(Arc::new(|_text: &str| Vec::new()));
    let run = loop_.run("1+1は？").await?;
    println!("steps: {}", run.steps.len());
    println!("acp stop reason: {}", run.final_text);
    Ok(())
}

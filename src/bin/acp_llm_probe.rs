//! Probe: drive a cordis-rs agent loop through a real ACP backend.
//!
//! `dsh --profile acp` is spawned as a subprocess and wrapped in an LLM
//! provider via `from_async_text`. This verifies the adapter seam; a full
//! session-persistent ACP client is future work.
//!
//! Requires a working model provider configuration in the environment.

use std::sync::Arc;

use agent_client_protocol::schema::{InitializeRequest, ProtocolVersion};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use cordis_rs::Context;
use cordis_rs::core_services::{
    AgentLoop, LlmRequest, from_async_text, provide_llm, provide_tools,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Run the ACP handshake and a single prompt over an arbitrary transport,
/// returning the agent's message text.
///
/// `PromptResponse::stop_reason` is a completion signal, not the answer body.
/// The text is delivered through `session/update` notifications, so we use the
/// session API's `read_to_string`, which collects `AgentMessageChunk` text and
/// stops only when the turn's stop reason arrives.
async fn acp_prompt_over(
    prompt: String,
    transport: impl ConnectTo<Client> + 'static,
) -> std::result::Result<String, String> {
    Client
        .builder()
        .name("cordis-acp-llm-probe")
        .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            connection
                .build_session_cwd()?
                .block_task()
                .run_until(async |mut session| {
                    session.send_prompt(prompt)?;
                    session.read_to_string().await
                })
                .await
        })
        .await
        .map_err(|error| error.to_string())
}

/// Ask the ACP agent one prompt in a fresh session and return its message body.
async fn acp_prompt(prompt: String) -> std::result::Result<String, String> {
    let mut cmd = tokio::process::Command::new("dsh");
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|error| error.to_string())?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

    let body = acp_prompt_over(prompt, transport).await;
    let _ = child.kill().await;
    body
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
    println!("acp body: {}", run.final_text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::{
        ContentBlock, ContentChunk, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptRequest, PromptResponse, SessionId, SessionNotification,
        SessionUpdate, StopReason, TextContent,
    };
    use agent_client_protocol::{Agent, Channel, Client, ConnectionTo};

    use super::acp_prompt_over;

    const SESSION_ID: &str = "probe-session";

    /// Serve a fake ACP agent over one end of an in-process duplex channel.
    ///
    /// It streams the supplied text chunks for every prompt and then completes
    /// the turn with `StopReason::EndTurn`. No model provider or subprocess is
    /// involved.
    fn spawn_fake_agent(chunks: &'static [&'static str]) -> agent_client_protocol::Channel {
        let (client_channel, agent_channel) = Channel::duplex();
        tokio::spawn(async move {
            let builder = Agent
                .builder()
                .name("fake-acp-agent")
                .on_receive_request(
                    async move |request: InitializeRequest, responder, _cx| {
                        responder.respond(InitializeResponse::new(request.protocol_version))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |_request: NewSessionRequest, responder, _cx| {
                        responder.respond(NewSessionResponse::new(SessionId::from(SESSION_ID)))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |_request: PromptRequest, responder, cx: ConnectionTo<Client>| {
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
            // `connect_to` serves until the channel closes; dropping the test's
            // runtime aborts this task.
            let _ = builder.connect_to(agent_channel).await;
        });
        client_channel
    }

    #[tokio::test]
    async fn body_is_streamed_text_not_stop_reason() {
        let client_channel = spawn_fake_agent(&["2+2", "=4"]);

        let body = acp_prompt_over("1+1は？".to_string(), client_channel)
            .await
            .expect("offline prompt should succeed");

        assert_eq!(body, "2+2=4", "body must be the streamed agent text");
        assert!(
            !body.contains("EndTurn"),
            "stop reason must never leak into the body"
        );
    }

    #[tokio::test]
    async fn body_is_empty_when_agent_streams_no_text() {
        let client_channel = spawn_fake_agent(&[]);

        let body = acp_prompt_over("1+1は？".to_string(), client_channel)
            .await
            .expect("offline prompt should succeed");

        assert_eq!(
            body, "",
            "a stop reason alone must not be returned as the body"
        );
    }
}

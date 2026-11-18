//! Probe: drive a cordis-rs agent loop through a real ACP backend.
//!
//! `dsh --profile acp` is spawned as a subprocess and wrapped in an LLM
//! provider via `from_async_text`. This verifies the adapter seam; a full
//! session-persistent ACP client is future work.
//!
//! Requires a working model provider configuration in the environment.
//!
//! Failures are classified into balance / rate-limit / auth-expired /
//! disconnect before being reported, so the operator sees a class-specific
//! recovery step instead of one generic string (`errors-not-classified`, G7).
//! The ACP transport itself has no retry/backoff; the separate
//! `cordis_retry_ui` demo exercises cordis agent-loop retry with a local stub
//! and is NOT this ACP path.

use std::sync::Arc;

use agent_client_protocol::schema::{InitializeRequest, ProtocolVersion};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use cordis_rs::Context;
use cordis_rs::core_services::{
    AgentLoop, LlmRequest, from_async_text, provide_llm, provide_tools,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Operator-facing class of an ACP / model-provider failure.
///
/// ACP (`error.to_string()`) and the cordis LLM provider both surface failures
/// as opaque strings, so classification is text based. It is a pure function
/// so the recovery guidance can be regression-tested without a live backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcpFailureKind {
    Balance,
    RateLimited,
    AuthExpired,
    Disconnected,
    Unknown,
}

impl AcpFailureKind {
    fn classify(message: &str) -> Self {
        let text = message.to_ascii_lowercase();
        const BALANCE: &[&str] = &[
            "insufficient balance",
            "insufficient credit",
            "insufficient funds",
            "insufficient_quota",
            "payment required",
            "billing",
            "402",
            "残高不足",
        ];
        const RATE_LIMIT: &[&str] = &["429", "rate limit", "too many requests", "throttl", "quota"];
        const AUTH: &[&str] = &[
            "auth",
            "unauthor",
            "forbidden",
            "credential",
            "token",
            "expired",
            "401",
            "403",
        ];
        const DISCONNECT: &[&str] = &[
            "disconnect",
            "connection",
            "broken pipe",
            "eof",
            "no stdin",
            "no stdout",
            "spawn",
            "transport",
            "timed out",
            "timeout",
            "reset by peer",
        ];
        // Specific causes first: a balance message may also mention "request"
        // or "limit", and an auth failure may follow a reconnect attempt.
        if BALANCE.iter().any(|needle| text.contains(needle)) {
            Self::Balance
        } else if RATE_LIMIT.iter().any(|needle| text.contains(needle)) {
            Self::RateLimited
        } else if AUTH.iter().any(|needle| text.contains(needle)) {
            Self::AuthExpired
        } else if DISCONNECT.iter().any(|needle| text.contains(needle)) {
            Self::Disconnected
        } else {
            Self::Unknown
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Balance => "balance/402",
            Self::RateLimited => "rate-limit/429",
            Self::AuthExpired => "auth-expired",
            Self::Disconnected => "disconnected",
            Self::Unknown => "unknown",
        }
    }

    /// Next step for the operator. Each class has a distinct hint: the old
    /// behaviour rendered all four as the same generic string.
    fn recovery(self) -> &'static str {
        match self {
            Self::Balance => {
                "top up the model account or switch provider, then rerun (not an ACP transport fault)"
            }
            Self::RateLimited => {
                "wait for the rate-limit window or lower request rate; the ACP client has no retry/backoff"
            }
            Self::AuthExpired => "refresh the credentials/API key in the `acp` profile, then rerun",
            Self::Disconnected => {
                "check `dsh --profile acp` is on PATH and its stdio pipe is alive, then rerun"
            }
            Self::Unknown => "inspect the raw error below; no automatic recovery hint",
        }
    }
}

/// Render a failure with its class and next step. Pure so tests can assert
/// that every class carries a distinct, actionable recovery line.
fn format_failure(message: &str) -> String {
    let kind = AcpFailureKind::classify(message);
    format!(
        "[{}] {message}\n  recovery: {}",
        kind.label(),
        kind.recovery()
    )
}

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
    let run = match loop_.run("1+1は？").await {
        Ok(run) => run,
        Err(error) => {
            // Provider/ACP failures reach here as opaque strings. Print the
            // classified recovery hint before exiting; keep returning the
            // original error so the probe's failure exit status is unchanged.
            eprintln!("acp_llm_probe: {}", format_failure(&error.to_string()));
            return Err(error.into());
        }
    };
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

    use super::{AcpFailureKind, acp_prompt_over, format_failure};

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

    #[test]
    fn failures_are_classified_into_four_distinct_kinds() {
        let cases = [
            // The balance error actually seen against `dsh --profile acp`.
            ("Error: Insufficient Balance", AcpFailureKind::Balance),
            ("HTTP 429 rate limited", AcpFailureKind::RateLimited),
            (
                "AuthRequired: token expired (401)",
                AcpFailureKind::AuthExpired,
            ),
            (
                "ACP transport disconnected: broken pipe",
                AcpFailureKind::Disconnected,
            ),
            ("some brand new failure", AcpFailureKind::Unknown),
        ];
        for (message, expected) in cases {
            assert_eq!(
                AcpFailureKind::classify(message),
                expected,
                "unexpected class for {message}"
            );
        }
    }

    #[test]
    fn every_failure_kind_has_a_distinct_actionable_hint() {
        let hints: Vec<_> = [
            AcpFailureKind::Balance,
            AcpFailureKind::RateLimited,
            AcpFailureKind::AuthExpired,
            AcpFailureKind::Disconnected,
            AcpFailureKind::Unknown,
        ]
        .into_iter()
        .map(|kind| (kind, kind.recovery()))
        .collect();
        let unique: std::collections::BTreeSet<_> = hints.iter().map(|(_, hint)| *hint).collect();
        assert_eq!(unique.len(), hints.len(), "hints must be class-specific");
        for (kind, hint) in hints {
            assert!(!hint.is_empty(), "missing hint for {kind:?}");
        }
    }

    #[test]
    fn formatted_failure_carries_class_and_next_step() {
        let report = format_failure("Insufficient Balance");
        assert!(report.contains("[balance/402]"), "report: {report}");
        assert!(report.contains("Insufficient Balance"), "report: {report}");
        assert!(
            report.contains("recovery: top up"),
            "balance errors must get a top-up step: {report}"
        );
        assert!(
            !format_failure("HTTP 429 rate limited").contains("top up"),
            "429 must not reuse the balance hint"
        );
    }

    /// Regression for parallel-test isolation: this probe has no fixed `/tmp`
    /// path (it uses `build_session_cwd()` -> `current_dir()`), but concurrent
    /// offline runs must still keep their streamed bodies separate.
    #[tokio::test]
    async fn concurrent_offline_prompts_stay_isolated() {
        let first = spawn_fake_agent(&["2+2", "=4"]);
        let second = spawn_fake_agent(&["3+3", "=6"]);

        let (first_body, second_body) = tokio::join!(
            acp_prompt_over("1+1は？".to_string(), first),
            acp_prompt_over("2+2は？".to_string(), second),
        );

        assert_eq!(
            first_body.expect("first offline prompt should succeed"),
            "2+2=4"
        );
        assert_eq!(
            second_body.expect("second offline prompt should succeed"),
            "3+3=6"
        );
    }
}

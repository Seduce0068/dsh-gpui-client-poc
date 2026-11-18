//! GPUI window that renders ACP `session/update` chunks as they arrive.
//!
//! The ACP client runs on a background thread and appends rendered lines from
//! each `session/update` to a shared buffer. A GPUI task polls the buffer and
//! updates the view, so updates appear live. `AgentMessageChunk(Text)` is still
//! appended verbatim (no marker or separator); other DSH update variants get a
//! labelled line or an explicit ignore reason. With an exhausted model balance
//! no chunks arrive, but the wiring is exercised and the connection status is
//! visible.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion,
    SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[derive(Default)]
struct StreamBuffer {
    chunks: Vec<String>,
    status: String,
}

/// Turn one `session/update` into a line for the shared transcript.
///
/// `AgentMessageChunk(Text)` deliberately keeps the legacy behaviour: the bare
/// text is returned and concatenated without a separator, so the agent's answer
/// is unchanged. Every other variant gets a leading/trailing newline so its
/// marker cannot be spliced into the middle of a word. Unknown variants (for
/// example `usage_update`, gated behind the `unstable_session_usage` feature of
/// `agent-client-protocol` and therefore undecodable in this build) get an
/// explicit ignore reason instead of being silently dropped.
fn render_update_line(update: &SessionUpdate) -> String {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
            ContentBlock::Text(text) => text.text.clone(),
            other => ignored_content("AgentMessageChunk", other),
        },
        SessionUpdate::AgentThoughtChunk(chunk) => match &chunk.content {
            ContentBlock::Text(text) => {
                let thought = &text.text;
                format!("\n[thought] {thought}\n")
            }
            other => ignored_content("AgentThoughtChunk", other),
        },
        SessionUpdate::ToolCall(call) => {
            let id = &call.tool_call_id;
            let title = &call.title;
            let status = call.status;
            format!("\n[tool_call id={id}] {title} (status={status:?})\n")
        }
        SessionUpdate::ToolCallUpdate(update) => {
            let id = &update.tool_call_id;
            let status = match update.fields.status {
                Some(status) => format!("{status:?}"),
                None => "(unchanged)".to_owned(),
            };
            let title = update.fields.title.as_deref().unwrap_or("(unchanged)");
            format!("\n[tool_call_update id={id}] status={status} title={title}\n")
        }
        SessionUpdate::ConfigOptionUpdate(update) => {
            let names = update
                .config_options
                .iter()
                .map(|option| option.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let names = if names.is_empty() { "(none)" } else { &names };
            format!("\n[config_option_update] {names}\n")
        }
        other => {
            let kind = update_kind(other);
            format!(
                "\n[ignored sessionUpdate={kind}] not rendered in this build (unsupported or unstable variant)\n"
            )
        }
    }
}

/// Extract the wire-level `sessionUpdate` tag without depending on a variant
/// being available in this crate's feature set.
fn update_kind(update: &SessionUpdate) -> String {
    tag_from_json(serde_json::to_value(update).ok().as_ref(), "sessionUpdate")
}

/// Extract the wire-level `type` tag of a content block.
fn content_kind(content: &ContentBlock) -> String {
    tag_from_json(serde_json::to_value(content).ok().as_ref(), "type")
}

fn tag_from_json(value: Option<&serde_json::Value>, key: &str) -> String {
    value
        .and_then(|json| json.get(key))
        .and_then(|tag| tag.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| "<unknown>".to_owned())
}

fn ignored_content(update: &str, content: &ContentBlock) -> String {
    let kind = content_kind(content);
    format!("\n[ignored {update} content={kind}] only Text content is rendered in this build\n")
}

struct StreamPanel {
    buffer: Arc<Mutex<StreamBuffer>>,
}

impl Render for StreamPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let (status, text) = {
            let buffer = self.buffer.lock().expect("stream buffer poisoned");
            (buffer.status.clone(), buffer.chunks.join(""))
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_3()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — ACP live stream"))
            .child(div().text_sm().child(SharedString::from(status)))
            .child(
                div()
                    .flex_1()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x24242c))
                    .child(SharedString::from(if text.is_empty() {
                        "(waiting for first chunk…)".to_string()
                    } else {
                        text
                    })),
            )
    }
}

fn spawn_acp(buffer: Arc<Mutex<StreamBuffer>>) {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                buffer.lock().unwrap().status = format!("runtime error: {error}");
                return;
            }
        };
        runtime.block_on(async move {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            let prompt = "Explain this repository in one sentence.".to_string();

            let mut cmd = tokio::process::Command::new("dsh");
            cmd.args(["--profile", "acp"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped());
            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(error) => {
                    buffer.lock().unwrap().status = format!("spawn error: {error}");
                    return;
                }
            };
            let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
                buffer.lock().unwrap().status = "no stdio".to_string();
                return;
            };
            let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

            let notified = buffer.clone();
            let result = Client
                .builder()
                .name("cordis-acp-stream-ui")
                .on_receive_notification(
                    move |notification: SessionNotification, _cx| {
                        let notified = notified.clone();
                        async move {
                            let line = render_update_line(&notification.update);
                            if !line.is_empty() {
                                notified.lock().unwrap().chunks.push(line);
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
                    let response = connection
                        .send_request(PromptRequest::new(
                            session.session_id,
                            vec![ContentBlock::Text(TextContent::new(prompt))],
                        ))
                        .block_task()
                        .await?;
                    Ok(format!("{:?}", response.stop_reason))
                })
                .await;

            {
                let mut guard = buffer.lock().unwrap();
                guard.status = match result {
                    Ok(reason) => format!("turn finished: {reason}"),
                    Err(error) => format!("turn error: {error}"),
                };
            }
            let _ = child.kill().await;
        });
    });
}

fn main() {
    let buffer = Arc::new(Mutex::new(StreamBuffer {
        status: "connecting to dsh --profile acp…".to_string(),
        chunks: Vec::new(),
    }));
    spawn_acp(buffer.clone());

    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| StreamPanel {
            buffer: buffer.clone(),
        });
        let poll_target = panel.clone();
        let poll_buffer = buffer.clone();
        let timer = cx.background_executor().clone();
        cx.spawn(async move |app: &mut gpui::AsyncApp| {
            loop {
                timer.timer(Duration::from_millis(100)).await;
                let _ = app.update(|cx| {
                    poll_target.update(cx, |panel, cx| {
                        panel.refresh_from(&poll_buffer, cx);
                    });
                });
            }
        })
        .detach();
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("acp_stream_ui: GPUI window created");
    });
}

impl StreamPanel {
    fn refresh_from(&mut self, _buffer: &Arc<Mutex<StreamBuffer>>, cx: &mut GpuiContext<Self>) {
        // The Render impl reads the shared buffer each frame, so a notify is
        // sufficient to repaint with the latest chunks.
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::{
        ConfigOptionUpdate, ContentChunk, TextContent, ToolCall, ToolCallStatus, ToolCallUpdate,
        ToolCallUpdateFields,
    };

    fn text_chunk(text: &str) -> ContentChunk {
        ContentChunk::new(ContentBlock::Text(TextContent::new(text.to_string())))
    }

    #[test]
    fn agent_message_chunk_text_is_verbatim() {
        let update = SessionUpdate::AgentMessageChunk(text_chunk("Hello "));
        assert_eq!(render_update_line(&update), "Hello ");
    }

    /// The legacy `chunks.join("")` rendering must stay byte-for-byte identical
    /// for ordinary agent message chunks. The panel still renders one plain-text
    /// `SharedString` child, so its DOM/aria shape is left untouched.
    #[test]
    fn agent_message_chunk_join_is_unchanged() {
        let mut chunks = Vec::new();
        for update in [
            SessionUpdate::AgentMessageChunk(text_chunk("Hello ")),
            SessionUpdate::AgentMessageChunk(text_chunk("world")),
        ] {
            chunks.push(render_update_line(&update));
        }
        assert_eq!(chunks.join(""), "Hello world");
    }

    #[test]
    fn agent_thought_chunk_is_rendered() {
        let update = SessionUpdate::AgentThoughtChunk(text_chunk("need to check the tool"));
        let line = render_update_line(&update);
        assert!(line.contains("[thought] need to check the tool"), "{line}");
        assert!(line.starts_with('\n') && line.ends_with('\n'), "{line}");
    }

    #[test]
    fn tool_call_is_rendered() {
        let call = ToolCall::new("call-1", "read file").status(ToolCallStatus::InProgress);
        let line = render_update_line(&SessionUpdate::ToolCall(call));
        assert!(line.contains("[tool_call id=call-1] read file"), "{line}");
        assert!(line.contains("InProgress"), "{line}");
    }

    #[test]
    fn tool_call_update_is_rendered() {
        let fields = ToolCallUpdateFields::new()
            .status(Some(ToolCallStatus::Completed))
            .title(Some("read file".to_owned()));
        let update = SessionUpdate::ToolCallUpdate(ToolCallUpdate::new("call-1", fields));
        let line = render_update_line(&update);
        assert!(line.contains("[tool_call_update id=call-1]"), "{line}");
        assert!(line.contains("status=Completed"), "{line}");
        assert!(line.contains("title=read file"), "{line}");
    }

    #[test]
    fn config_option_update_is_rendered() {
        let update = SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(Vec::new()));
        let line = render_update_line(&update);
        assert!(line.contains("[config_option_update] (none)"), "{line}");
    }

    /// `usage_update` is gated behind `unstable_session_usage` in
    /// agent-client-protocol 0.11.1 and that feature is not enabled here, so
    /// there is no typed variant to match. If it ever arrives it goes through
    /// the wildcard arm; `UserMessageChunk` exercises that same arm using a
    /// variant that does exist in this build.
    #[test]
    fn unknown_update_variant_has_explicit_ignore_reason() {
        let update = SessionUpdate::UserMessageChunk(text_chunk("echo"));
        let line = render_update_line(&update);
        assert!(line.contains("sessionUpdate=user_message_chunk"), "{line}");
        assert!(line.contains("not rendered in this build"), "{line}");
    }

    #[test]
    fn non_text_content_has_explicit_ignore_reason() {
        let content: ContentBlock = serde_json::from_value(serde_json::json!({
            "type": "image",
            "data": "AAAA",
            "mimeType": "image/png",
        }))
        .expect("image content block should decode");
        let update = SessionUpdate::AgentMessageChunk(ContentChunk::new(content));
        let line = render_update_line(&update);
        assert!(
            line.contains("ignored AgentMessageChunk content=image"),
            "{line}"
        );
    }
}

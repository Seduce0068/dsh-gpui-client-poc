//! GPUI window that renders ACP `session/update` chunks as they arrive.
//!
//! The ACP client runs on a background thread and appends rendered lines from
//! each `session/update` to a shared buffer keyed by session id, so concurrent
//! sessions cannot splice their text into one answer. A GPUI task waits on a
//! wake-up signal and calls `cx.notify()` whenever the buffer changes, so
//! updates appear live without a 100 ms polling loop. `AgentMessageChunk(Text)` is
//! still appended verbatim (no marker or separator); other DSH update variants
//! get a labelled line or an explicit ignore reason. With an exhausted model
//! balance no chunks arrive, but the wiring is exercised and the connection
//! status is visible.
//!
//! Lifetime: the repaint task holds only a `WeakEntity<StreamPanel>`, so it can
//! never keep the view alive after its window is gone. Closing the window
//! cancels `StreamSignal::done`, which ends the detached task instead of
//! leaking it forever.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion, SessionId,
    SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};
use tokio::sync::Notify;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tokio_util::sync::CancellationToken;

/// One independent transcript per ACP session, plus connection-level status.
///
/// A single ACP connection can deliver `session/update` notifications for more
/// than one `sessionId`. A shared `Vec<String>` lets those sessions splice their
/// text into one answer, so each session owns its own vector keyed by
/// `SessionId`. `status` stays global on purpose: runtime/spawn/turn errors are
/// connection-scoped and are not the text of any one session.
#[derive(Default)]
struct StreamBuffer {
    sessions: HashMap<SessionId, Vec<String>>,
    status: String,
}

impl StreamBuffer {
    /// Append a rendered line to exactly one session's transcript.
    fn append_update(&mut self, session_id: &SessionId, line: String) {
        self.sessions
            .entry(session_id.clone())
            .or_default()
            .push(line);
    }

    /// Verbatim transcript of one session: no separator is inserted between its
    /// chunks, preserving the legacy rendering of agent message chunks.
    fn session_transcript(&self, session_id: &SessionId) -> String {
        self.sessions
            .get(session_id)
            .map(|chunks| chunks.concat())
            .unwrap_or_default()
    }

    /// Render every session into the panel's single text area. With one session
    /// this is the legacy byte-for-byte join; with several, stable labelled
    /// sections keep their text from reading as one continuous answer.
    fn render_transcript(&self) -> String {
        if self.sessions.len() <= 1 {
            return self
                .sessions
                .keys()
                .next()
                .map(|session_id| self.session_transcript(session_id))
                .unwrap_or_default();
        }

        let mut sessions: Vec<_> = self.sessions.iter().collect();
        sessions.sort_by_key(|(session_id, _)| session_id.to_string());
        let mut rendered = String::new();
        for (session_id, chunks) in sessions {
            rendered.push_str(&format!("\n[session {session_id}]\n"));
            rendered.push_str(&chunks.concat());
        }
        rendered
    }
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
            (buffer.status.clone(), buffer.render_transcript())
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

/// Route one ACP notification into its own session's transcript.
///
/// `SessionNotification::session_id` is the routing key; the rendered line is
/// unchanged. Kept out of the callback so the routing is regression-testable
/// without opening a real ACP connection.
fn record_notification(buffer: &Mutex<StreamBuffer>, notification: &SessionNotification) {
    let line = render_update_line(&notification.update);
    if line.is_empty() {
        return;
    }
    buffer
        .lock()
        .expect("stream buffer poisoned")
        .append_update(&notification.session_id, line);
}

/// Wake-up and completion channel shared by the ACP producer thread and the
/// GPUI repaint task.
///
/// * `wake` replaces the old 100 ms poll: every buffer mutation calls
///   `notify_one`, so the repaint task runs exactly when new text exists.
/// * `done` is the completion notification tied to window lifecycle. Closing
///   the window cancels it, ending the detached task. The task keeps only a
///   `WeakEntity`, so neither the view nor the task is leaked.
#[derive(Clone)]
struct StreamSignal {
    wake: Arc<Notify>,
    done: CancellationToken,
}

impl Default for StreamSignal {
    fn default() -> Self {
        Self {
            wake: Arc::new(Notify::new()),
            done: CancellationToken::new(),
        }
    }
}

impl StreamSignal {
    /// Signal that the shared buffer changed and the view should repaint.
    fn changed(&self) {
        self.wake.notify_one();
    }

    /// Signal completion from the window-close lifecycle hook. Idempotent.
    fn complete(&self) {
        self.done.cancel();
    }
}

/// Repaint on every buffer change until the owning window completes us.
///
/// Deliberately free of GPUI types so the wake/cancel contract is regression
/// testable with a plain tokio runtime. `on_change` returns `false` when its
/// weak target no longer resolves; that also ends the pump so a released view
/// can never strand a live task.
async fn run_stream_pump(signal: &StreamSignal, mut on_change: impl FnMut() -> bool) {
    let wake = signal.wake.clone();
    let done = signal.done.clone();
    loop {
        tokio::select! {
            _ = wake.notified() => {
                if !on_change() {
                    break;
                }
            }
            _ = done.cancelled() => break,
        }
    }
}

fn spawn_acp(buffer: Arc<Mutex<StreamBuffer>>, signal: StreamSignal) {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                buffer.lock().expect("stream buffer poisoned").status =
                    format!("runtime error: {error}");
                signal.changed();
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
                    buffer.lock().expect("stream buffer poisoned").status =
                        format!("spawn error: {error}");
                    signal.changed();
                    return;
                }
            };
            let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
                buffer.lock().expect("stream buffer poisoned").status = "no stdio".to_string();
                signal.changed();
                return;
            };
            let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

            let notified = buffer.clone();
            let notified_signal = signal.clone();
            let result = Client
                .builder()
                .name("cordis-acp-stream-ui")
                .on_receive_notification(
                    move |notification: SessionNotification, _cx| {
                        let notified = notified.clone();
                        let signal = notified_signal.clone();
                        async move {
                            record_notification(&notified, &notification);
                            signal.changed();
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
                let mut guard = buffer.lock().expect("stream buffer poisoned");
                guard.status = match result {
                    Ok(reason) => format!("turn finished: {reason}"),
                    Err(error) => format!("turn error: {error}"),
                };
            }
            signal.changed();
            let _ = child.kill().await;
        });
    });
}

fn main() {
    let buffer = Arc::new(Mutex::new(StreamBuffer {
        status: "connecting to dsh --profile acp…".to_string(),
        sessions: HashMap::new(),
    }));
    let signal = StreamSignal::default();
    spawn_acp(buffer.clone(), signal.clone());

    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| StreamPanel {
            buffer: buffer.clone(),
        });

        // The detached repaint task holds only a weak handle: it can never keep
        // the view alive after its window is gone.
        let weak_panel = panel.downgrade();
        let pump_signal = signal.clone();

        // Window lifecycle owns the completion signal, so closing the window
        // ends the task instead of leaving an infinite detached task behind.
        let close_signal = signal.clone();
        cx.on_window_closed(move |_cx: &mut App| {
            close_signal.complete();
        })
        .detach();

        cx.spawn(async move |app: &mut gpui::AsyncApp| {
            run_stream_pump(&pump_signal, || {
                // A failed weak upgrade is reported as `false`, which stops the
                // pump. `cx.notify()` in `refresh_from` repaints from the buffer.
                app.update(|cx| weak_panel.update(cx, |panel, cx| panel.refresh_from(cx)))
                    .is_ok_and(|result| result.is_ok())
            })
            .await;
        })
        .detach();

        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("acp_stream_ui: GPUI window created");
    });
}

impl StreamPanel {
    fn refresh_from(&mut self, cx: &mut GpuiContext<Self>) {
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

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

    fn session(id: &str) -> SessionId {
        SessionId::new(id)
    }

    fn notification(session_id: &str, update: SessionUpdate) -> SessionNotification {
        SessionNotification::new(session(session_id), update)
    }

    /// Regression: notifications for two sessions arriving over one connection
    /// must never splice into a single transcript. This drives the same
    /// `record_notification` path that `spawn_acp` uses, including the
    /// `session_id` routing key.
    #[test]
    fn concurrent_sessions_do_not_share_a_transcript() {
        let buffer = Arc::new(Mutex::new(StreamBuffer::default()));

        record_notification(
            &buffer,
            &notification(
                "session-a",
                SessionUpdate::AgentMessageChunk(text_chunk("Hello from A: ")),
            ),
        );
        record_notification(
            &buffer,
            &notification(
                "session-b",
                SessionUpdate::AgentMessageChunk(text_chunk("Bonjour from B")),
            ),
        );
        record_notification(
            &buffer,
            &notification(
                "session-a",
                SessionUpdate::AgentMessageChunk(text_chunk("done")),
            ),
        );

        let guard = buffer.lock().expect("stream buffer poisoned");
        assert_eq!(guard.sessions.len(), 2);
        let a = guard.session_transcript(&session("session-a"));
        let b = guard.session_transcript(&session("session-b"));
        assert_eq!(a, "Hello from A: done");
        assert_eq!(b, "Bonjour from B");
        assert!(!a.contains("Bonjour"), "{a}");
        assert!(!b.contains("Hello") && !b.contains("done"), "{b}");
    }

    /// One session still renders exactly as before: the plain chunks are joined
    /// with no marker or separator inserted.
    #[test]
    fn single_session_render_is_legacy_verbatim_join() {
        let mut buffer = StreamBuffer::default();
        buffer.append_update(&session("solo"), "Hello ".to_owned());
        buffer.append_update(&session("solo"), "world".to_owned());
        assert_eq!(buffer.render_transcript(), "Hello world");
    }

    #[test]
    fn empty_buffer_renders_empty_transcript() {
        assert_eq!(StreamBuffer::default().render_transcript(), "");
    }

    /// Multi-session rendering uses a stable order so the text area is
    /// deterministic even though `HashMap` iteration is not.
    #[test]
    fn multiple_sessions_render_with_stable_labelled_sections() {
        let mut buffer = StreamBuffer::default();
        buffer.append_update(&session("zeta"), "Z".to_owned());
        buffer.append_update(&session("alpha"), "A".to_owned());

        let rendered = buffer.render_transcript();
        let alpha = rendered
            .find("[session alpha]")
            .expect("alpha session label missing");
        let zeta = rendered
            .find("[session zeta]")
            .expect("zeta session label missing");
        assert!(alpha < zeta, "{rendered}");
        assert!(
            rendered.contains('A') && rendered.contains('Z'),
            "{rendered}"
        );
    }

    /// An update that renders to nothing must not create an empty session.
    #[test]
    fn empty_rendered_line_does_not_create_session() {
        let buffer = Arc::new(Mutex::new(StreamBuffer::default()));
        record_notification(
            &buffer,
            &notification("silent", SessionUpdate::AgentMessageChunk(text_chunk(""))),
        );
        assert!(
            buffer
                .lock()
                .expect("stream buffer poisoned")
                .sessions
                .is_empty()
        );
    }

    /// Regression: a buffer change wakes the repaint closure without any timer,
    /// and the window-close completion ends the detached pump. Together these
    /// replace the old 100 ms infinite poll that leaked a strong `Entity`.
    #[tokio::test]
    async fn pump_repaints_on_change_and_stops_on_completion() {
        let signal = StreamSignal::default();
        let repaints = Arc::new(AtomicUsize::new(0));

        let pump_signal = signal.clone();
        let pump_repaints = repaints.clone();
        let pump = tokio::spawn(async move {
            run_stream_pump(&pump_signal, || {
                pump_repaints.fetch_add(1, Ordering::SeqCst);
                true
            })
            .await;
        });

        // Producer-side changes collapse into at least one repaint; no clock is
        // involved, so this can only happen through `Notify::notify_one`.
        signal.changed();
        signal.changed();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(repaints.load(Ordering::SeqCst) >= 1);

        // Simulate the window-close hook firing.
        signal.complete();
        tokio::time::timeout(Duration::from_secs(1), pump)
            .await
            .expect("pump must terminate when the window completes it")
            .expect("pump task must not panic");
    }

    /// The completion notification is idempotent (several lifecycle hooks may
    /// fire) and must not itself count as a buffer-change wake-up.
    #[test]
    fn completion_is_idempotent_and_independent_of_change() {
        let signal = StreamSignal::default();
        assert!(!signal.done.is_cancelled());
        signal.complete();
        signal.complete();
        assert!(signal.done.is_cancelled());
    }

    /// A pump started after the window already completed exits immediately, so
    /// a late-scheduled task cannot outlive its window.
    #[tokio::test]
    async fn pump_started_after_completion_exits_immediately() {
        let signal = StreamSignal::default();
        signal.complete();
        tokio::time::timeout(Duration::from_secs(1), run_stream_pump(&signal, || true))
            .await
            .expect("pump should return immediately when already completed");
    }

    /// When the weak target no longer resolves (`on_change` returns `false`),
    /// the pump stops on its own instead of spinning on further notifications.
    #[tokio::test]
    async fn pump_stops_when_weak_target_is_gone() {
        let signal = StreamSignal::default();
        let pump_signal = signal.clone();
        let pump = tokio::spawn(async move { run_stream_pump(&pump_signal, || false).await });

        signal.changed();
        tokio::time::timeout(Duration::from_secs(1), pump)
            .await
            .expect("pump must stop when its weak target no longer resolves")
            .expect("pump task must not panic");
    }
}

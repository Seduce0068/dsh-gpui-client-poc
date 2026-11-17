//! GPUI window that renders ACP `session/update` chunks as they arrive.
//!
//! The ACP client runs on a background thread and appends `AgentMessageChunk`
//! text to a shared buffer. A GPUI task polls the buffer and updates the view,
//! so chunks appear live. With an exhausted model balance no chunks arrive,
//! but the wiring is exercised and the connection status is visible.

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
                            if let SessionUpdate::AgentMessageChunk(chunk) = &notification.update
                                && let ContentBlock::Text(text) = &chunk.content
                            {
                                notified.lock().unwrap().chunks.push(text.text.clone());
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

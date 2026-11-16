//! GPUI view of a cordis-rs agent-loop run.
//!
//! Mounts the LLM / tools / fs / shell / jobs seams, registers the tool-fs and
//! tool-bash ports, runs a scripted agent, and renders each step's model text
//! and tool results in a GPUI window. No model credentials are needed.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cordis_rs::Context;
use cordis_rs::core_services::{
    AgentRun, LlmProvider, StreamChunk, ToolCall, provide_fs, provide_jobs, provide_llm,
    provide_shell, provide_tools,
};
use cordis_rs::plugin_ports::{ToolBashConfig, ToolFsConfig, mount_tool_bash, mount_tool_fs};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

fn scripted_provider() -> LlmProvider {
    let step = Arc::new(AtomicUsize::new(0));
    LlmProvider::new(
        "scripted",
        "scripted-1",
        Arc::new(move |_request| {
            let index = step.fetch_add(1, Ordering::SeqCst);
            let text = match index {
                0 => "WRITE",
                1 => "READ",
                _ => "DONE",
            };
            Box::pin(futures::stream::iter(vec![
                StreamChunk::Text(text.to_string()),
                StreamChunk::Done,
            ])) as Pin<Box<dyn futures::Stream<Item = StreamChunk> + Send + Unpin>>
        }),
    )
}

fn parse_calls(text: &str) -> Vec<ToolCall> {
    match text {
        "WRITE" => vec![ToolCall::new(
            "write",
            serde_json::json!({
                "file_path": "/tmp/cordis-agent-ui.txt",
                "content": "agent ui wrote this\n",
            }),
        )],
        "READ" => vec![ToolCall::new(
            "read",
            serde_json::json!({ "file_path": "/tmp/cordis-agent-ui.txt" }),
        )],
        _ => Vec::new(),
    }
}

struct AgentPanel {
    lines: Vec<SharedString>,
}

impl AgentPanel {
    fn from_run(run: &AgentRun) -> Self {
        let mut lines = vec![SharedString::from("cordis-rs × GPUI — agent-loop")];
        for (index, step) in run.steps.iter().enumerate() {
            lines.push(SharedString::from(format!(
                "step {}: {} ({} tool result(s))",
                index + 1,
                step.response.text,
                step.tool_results.len()
            )));
            for result in &step.tool_results {
                match result {
                    Ok(output) => lines.push(SharedString::from(format!(
                        "  {}",
                        output.rendered.first().cloned().unwrap_or_default()
                    ))),
                    Err(error) => lines.push(SharedString::from(format!("  error: {error}"))),
                }
            }
        }
        lines.push(SharedString::from(format!("final: {}", run.final_text)));
        Self { lines }
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let mut pane = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_2()
            .text_color(rgb(0xe8e8ef));
        for line in &self.lines {
            pane = pane.child(div().text_sm().child(line.clone()));
        }
        pane
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let run = runtime.block_on(async {
        let ctx = Context::root("gpui-agent-ui");
        let (tools, _) = provide_tools(&ctx).await.map_err(|e| e.to_string())?;
        let fs = provide_fs(&ctx).await.map_err(|e| e.to_string())?;
        let shell = provide_shell(&ctx).await.map_err(|e| e.to_string())?;
        let _jobs = provide_jobs(&ctx).await.map_err(|e| e.to_string())?;
        let llm = provide_llm(&ctx).await.map_err(|e| e.to_string())?;

        let _fs_tools = mount_tool_fs(&ctx, fs, ToolFsConfig::default())
            .await
            .map_err(|e| e.to_string())?;
        let _bash_tools = mount_tool_bash(&ctx, shell, ToolBashConfig::default())
            .await
            .map_err(|e| e.to_string())?;
        let _provider = llm.register(scripted_provider());
        llm.set_default("scripted");

        let loop_ = cordis_rs::core_services::AgentLoop::new(llm, tools)
            .with_max_steps(4)
            .with_call_parser(Arc::new(parse_calls));
        loop_
            .run("create and verify a file")
            .await
            .map_err(|e| e.to_string())
    })?;

    eprintln!(
        "cordis_agent_ui: {} steps, final = {}",
        run.steps.len(),
        run.final_text
    );

    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| AgentPanel::from_run(&run));
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_agent_ui: GPUI window created");
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scripted_agent_runs_and_renders_steps() {
        let provider = scripted_provider();
        assert_eq!(provider.name(), "scripted");
        assert_eq!(parse_calls("WRITE")[0].name, "write");
        assert_eq!(parse_calls("READ")[0].name, "read");
        assert!(parse_calls("DONE").is_empty());

        // Build a run without a GPUI window and verify the view data.
        let ctx = Context::root("gpui-agent-test");
        let (tools, _) = provide_tools(&ctx).await.unwrap();
        let fs = provide_fs(&ctx).await.unwrap();
        let shell = provide_shell(&ctx).await.unwrap();
        let _jobs = provide_jobs(&ctx).await.unwrap();
        let llm = provide_llm(&ctx).await.unwrap();
        let _fs_tools = mount_tool_fs(&ctx, fs, ToolFsConfig::default())
            .await
            .unwrap();
        let _bash_tools = mount_tool_bash(&ctx, shell, ToolBashConfig::default())
            .await
            .unwrap();
        let _provider = llm.register(scripted_provider());
        llm.set_default("scripted");

        let loop_ = cordis_rs::core_services::AgentLoop::new(llm, tools)
            .with_max_steps(4)
            .with_call_parser(Arc::new(parse_calls));
        let run = loop_.run("test").await.unwrap();
        assert_eq!(run.steps.len(), 3);
        assert_eq!(run.final_text, "DONE");
        let panel = AgentPanel::from_run(&run);
        assert!(
            panel
                .lines
                .iter()
                .any(|line| line.to_string().contains("step 1"))
        );
        assert!(
            panel
                .lines
                .iter()
                .any(|line| line.to_string().contains("final: DONE"))
        );
    }
}

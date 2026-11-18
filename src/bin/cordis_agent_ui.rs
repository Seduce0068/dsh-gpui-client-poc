//! GPUI view of a cordis-rs agent-loop run.
//!
//! Mounts the LLM / tools / fs / shell / jobs seams, registers the tool-fs and
//! tool-bash ports, runs a scripted agent, and renders each step's model text
//! and tool results in a GPUI window. No model credentials are needed.
//!
//! The loop also opts into the shared `sessions`, `systemPrompt`, `goals` and
//! `compaction` services so the scripted run exercises the same core seams as
//! the other cordis demos.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cordis_rs::Context;
use cordis_rs::core_services::{
    AgentLoop, AgentRun, CompactionEngine, GoalStore, LlmProvider, LlmService, PromptSection,
    Session, StreamChunk, SystemPrompt, ToolCall, ToolRegistry, provide_compaction, provide_fs,
    provide_goals, provide_jobs, provide_llm, provide_sessions, provide_shell,
    provide_system_prompt, provide_token_meter, provide_tools,
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

/// Handles retained next to the loop so the tests can check the provided
/// services, the session log and the rendered DOM text of the same run.
/// Only the regression tests read them, so release builds allow them unused.
#[cfg_attr(not(test), allow(dead_code))]
struct AgentUiRig {
    ctx: Context,
    session: Session,
    llm: LlmService,
    tools: ToolRegistry,
    agent: Arc<AgentLoop>,
}

/// Single source of truth for the loop's `with_*` wiring, shared by main and
/// the regression tests.
fn wire_agent_loop(
    llm: LlmService,
    tools: ToolRegistry,
    session: Session,
    prompt: SystemPrompt,
    goals: GoalStore,
    compaction: CompactionEngine,
) -> Arc<AgentLoop> {
    Arc::new(
        AgentLoop::new(llm, tools)
            .with_max_steps(4)
            .with_call_parser(Arc::new(parse_calls))
            .with_session(session)
            .with_system_prompt(prompt)
            .with_goals(goals)
            .with_compaction(compaction),
    )
}

impl AgentUiRig {
    async fn build() -> Result<Self, String> {
        let ctx = Context::root("gpui-agent-ui");
        let (tools, _) = provide_tools(&ctx).await.map_err(|e| e.to_string())?;
        let fs = provide_fs(&ctx).await.map_err(|e| e.to_string())?;
        let shell = provide_shell(&ctx).await.map_err(|e| e.to_string())?;
        let _jobs = provide_jobs(&ctx).await.map_err(|e| e.to_string())?;
        let llm = provide_llm(&ctx).await.map_err(|e| e.to_string())?;
        let (sessions, _) = provide_sessions(&ctx).await.map_err(|e| e.to_string())?;
        let prompt = provide_system_prompt(&ctx)
            .await
            .map_err(|e| e.to_string())?;
        let goals = provide_goals(&ctx).await.map_err(|e| e.to_string())?;
        let meter = provide_token_meter(&ctx).await.map_err(|e| e.to_string())?;
        let compaction = provide_compaction(&ctx, meter)
            .await
            .map_err(|e| e.to_string())?;

        let _fs_tools = mount_tool_fs(&ctx, fs, ToolFsConfig::default())
            .await
            .map_err(|e| e.to_string())?;
        let _bash_tools = mount_tool_bash(&ctx, shell, ToolBashConfig::default())
            .await
            .map_err(|e| e.to_string())?;

        let session = sessions
            .create(Some("gpui-agent-session".to_string()), None)
            .map_err(|e| e.to_string())?;
        prompt
            .section(PromptSection::new(
                "gpui-agent-goal",
                prompt.get_section_order("DEPLOYMENT_PERSONA_PREFIX"),
                "Goal: {{goal}} ({{goal_status}})",
            ))
            .map_err(|e| e.to_string())?;
        goals
            .set("create and verify a file", 10)
            .map_err(|e| e.to_string())?;

        let _provider = llm.register(scripted_provider());
        llm.set_default("scripted");

        let agent = wire_agent_loop(
            llm.clone(),
            tools.clone(),
            session.clone(),
            prompt,
            goals,
            compaction,
        );
        Ok(Self {
            ctx,
            session,
            llm,
            tools,
            agent,
        })
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
        let rig = AgentUiRig::build().await?;
        rig.agent
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
    use cordis_rs::core_services::{CompactionConfig, LlmRequest, SessionStore, TokenMeter};

    #[tokio::test]
    async fn scripted_agent_runs_and_renders_steps() {
        let provider = scripted_provider();
        assert_eq!(provider.name(), "scripted");
        assert_eq!(parse_calls("WRITE")[0].name, "write");
        assert_eq!(parse_calls("READ")[0].name, "read");
        assert!(parse_calls("DONE").is_empty());

        // Same rig as main, but without a GPUI window, to verify the view data.
        let rig = AgentUiRig::build().await.unwrap();
        let run = rig.agent.run("test").await.unwrap();
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

    /// Regression: the four services are on the context and the scripted run
    /// logs its turn and tool steps to the provided session.
    #[tokio::test]
    async fn shared_services_are_provided_and_turn_is_logged() {
        let rig = AgentUiRig::build().await.unwrap();
        assert!(rig.ctx.get::<SessionStore>("sessions").is_some());
        assert!(rig.ctx.get::<SystemPrompt>("systemPrompt").is_some());
        assert!(rig.ctx.get::<GoalStore>("goals").is_some());
        assert!(rig.ctx.get::<CompactionEngine>("compaction").is_some());
        assert!(rig.ctx.get::<TokenMeter>("tokenMeter").is_some());

        let run = rig.agent.run("test").await.unwrap();
        assert_eq!(run.steps.len(), 3);
        assert_eq!(run.final_text, "DONE");
        let events: Vec<String> = rig
            .session
            .events()
            .into_iter()
            .map(|event| event.event_type)
            .collect();
        assert_eq!(events.first().map(String::as_str), Some("turn/start"));
        assert_eq!(events.last().map(String::as_str), Some("turn/end"));
        assert!(events.iter().any(|event| event == "user/message"));
        assert_eq!(
            events.iter().filter(|event| *event == "step/start").count(),
            3
        );
        assert_eq!(
            events.iter().filter(|event| *event == "step/end").count(),
            3
        );
        assert!(events.iter().any(|event| event == "tool/call"));
        assert!(events.iter().any(|event| event == "tool/result"));
        assert!(!events.iter().any(|event| event == "compaction/checkpoint"));
    }

    /// Regression: `with_system_prompt` plus `with_goals` must assemble the
    /// live goal into a system message ahead of the user prompt.
    #[tokio::test]
    async fn goal_is_interpolated_into_the_system_message() {
        let rig = AgentUiRig::build().await.unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::<LlmRequest>::new()));
        let capture = seen.clone();
        let provider = LlmProvider::new(
            "capture",
            "capture-1",
            Arc::new(move |request| {
                capture.lock().unwrap().push(request);
                Box::pin(futures::stream::iter(vec![
                    StreamChunk::Text("ok".to_string()),
                    StreamChunk::Done,
                ]))
                    as Pin<Box<dyn futures::Stream<Item = StreamChunk> + Send + Unpin>>
            }),
        );
        let _disposer = rig.llm.register(provider);
        assert!(rig.llm.set_default("capture"));

        let run = rig.agent.run("test").await.unwrap();
        assert_eq!(run.final_text, "ok");
        let requests = seen.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages[0].role, "system");
        assert!(
            requests[0].messages[0]
                .content
                .contains("create and verify a file")
        );
        assert!(requests[0].messages[0].content.contains("active"));
        assert_eq!(requests[0].messages[1].role, "user");
    }

    /// Regression: the production `wire_agent_loop` chain must forward the
    /// compaction engine; a tiny budget forces a checkpoint to be appended.
    #[tokio::test]
    async fn wired_compaction_appends_a_checkpoint_under_pressure() {
        let rig = AgentUiRig::build().await.unwrap();
        let engine = CompactionEngine::new(
            CompactionConfig {
                pressure_threshold: 0.5,
                head_chars: 2,
                tail_chars: 2,
            },
            TokenMeter::new(1),
        )
        .unwrap();
        let agent = wire_agent_loop(
            rig.llm.clone(),
            rig.tools.clone(),
            rig.session.clone(),
            rig.ctx
                .get::<SystemPrompt>("systemPrompt")
                .unwrap()
                .as_ref()
                .clone(),
            rig.ctx.get::<GoalStore>("goals").unwrap().as_ref().clone(),
            engine,
        );
        agent.run("test").await.unwrap();
        assert!(
            rig.session
                .events()
                .iter()
                .any(|event| event.event_type == "compaction/checkpoint")
        );
    }
}

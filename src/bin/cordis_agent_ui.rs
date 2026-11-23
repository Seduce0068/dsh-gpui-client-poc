//! GPUI view of a cordis-rs agent-loop run.
//!
//! Mounts the LLM / tools / fs / shell / jobs seams, registers the read-only
//! tool-fs and tool-bash ports, runs a scripted agent, and renders each step's
//! model text and tool results in a GPUI window. No model credentials are needed.
//!
//! The loop also opts into the shared `sessions`, `systemPrompt`, `goals` and
//! `compaction` services so the scripted run exercises the same core seams as
//! the other cordis demos.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cordis_rs::Context;
use cordis_rs::core_services::{
    AgentLoop, AgentRun, ApprovalDecision, ApprovalService, CompactionEngine, FileSystem,
    GoalStore, LlmProvider, LlmService, PromptSection, Session, StreamChunk, SystemPrompt,
    ToolCall, ToolPipeline, ToolRegistry, provide_approval, provide_compaction, provide_goals,
    provide_jobs, provide_llm, provide_sessions, provide_shell, provide_system_prompt,
    provide_token_meter, provide_tools,
};
use cordis_rs::plugin_ports::{ToolBashConfig, ToolFsConfig, mount_tool_bash, mount_tool_fs};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

/// Host-side allowed-root policy: every fs tool call is confined to the
/// process's current directory. Path canonicalization (including `..`
/// normalization and symlink resolution) is owned by the external crate's
/// `FileSystem::scoped`; this host only chooses the root.
fn scoped_fs_root() -> std::io::Result<PathBuf> {
    std::env::current_dir()
}

/// Host-side tool policy: register only `read`. `write` and `edit` are disabled
/// here via the external crate's config fields, without changing its API.
fn read_only_tool_fs_config() -> ToolFsConfig {
    ToolFsConfig {
        read: true,
        write: false,
        edit: false,
        ..ToolFsConfig::default()
    }
}

fn scripted_provider() -> LlmProvider {
    let step = Arc::new(AtomicUsize::new(0));
    LlmProvider::new(
        "scripted",
        "scripted-1",
        Arc::new(move |_request| {
            let index = step.fetch_add(1, Ordering::SeqCst);
            let text = match index {
                0 => "READ_MANIFEST",
                1 => "READ_FIXTURE",
                _ => "DONE",
            };
            Box::pin(futures::stream::iter(vec![
                StreamChunk::Text(text.to_string()),
                StreamChunk::Done,
            ])) as Pin<Box<dyn futures::Stream<Item = StreamChunk> + Send + Unpin>>
        }),
    )
}

/// The scripted run only ever emits `read` calls: both paths live under the
/// scoped root (the current directory), and no write/edit tool is mounted.
fn parse_calls(text: &str) -> Vec<ToolCall> {
    match text {
        "READ_MANIFEST" => vec![ToolCall::new(
            "read",
            serde_json::json!({ "file_path": "Cargo.toml" }),
        )],
        "READ_FIXTURE" => vec![ToolCall::new(
            "read",
            serde_json::json!({ "file_path": "fixtures/text-turn.cordis.yml" }),
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
    pipeline: ToolPipeline,
    approval: ApprovalService,
    approval_calls: Arc<AtomicUsize>,
    agent: Arc<AgentLoop>,
}

/// Single source of truth for the loop's `with_*` wiring, shared by main and
/// the regression tests. The pipeline is the one returned by `provide_tools`
/// with the approval gate installed by the host.
fn wire_agent_loop(
    llm: LlmService,
    tools: ToolRegistry,
    pipeline: ToolPipeline,
    session: Session,
    prompt: SystemPrompt,
    goals: GoalStore,
    compaction: CompactionEngine,
) -> Arc<AgentLoop> {
    Arc::new(
        AgentLoop::new(llm, tools)
            .with_max_steps(4)
            .with_call_parser(Arc::new(parse_calls))
            .with_pipeline(pipeline)
            .with_session(session)
            .with_system_prompt(prompt)
            .with_goals(goals)
            .with_compaction(compaction),
    )
}

impl AgentUiRig {
    async fn build() -> Result<Self, String> {
        let ctx = Context::root("gpui-agent-ui");
        let (tools, pipeline) = provide_tools(&ctx).await.map_err(|e| e.to_string())?;
        let approval = provide_approval(&ctx).await.map_err(|e| e.to_string())?;
        // The gate fails closed: this read-only host authorizes only `read`
        // calls under the scoped root and denies every other action.
        //
        // This answerer is registered once at build time and never calls
        // `add_answerer` / `set_policy` from inside the callback. That keeps
        // the host outside the read-guard reentrancy hazard in older
        // cordis-rs releases (fixed upstream in the local fork by 94c277b).
        let approval_calls = Arc::new(AtomicUsize::new(0));
        let seen_calls = approval_calls.clone();
        let _answerer = approval.add_answerer(
            "cordis-agent-ui-read-only",
            Arc::new(move |request| {
                seen_calls.fetch_add(1, Ordering::SeqCst);
                if request.action == "tool:read" {
                    Some(ApprovalDecision::Allow)
                } else {
                    Some(ApprovalDecision::Deny)
                }
            }),
        );
        pipeline.with_approval(approval.clone());

        let fs = FileSystem::scoped(scoped_fs_root().map_err(|e| e.to_string())?);
        // Pre-provide the scoped filesystem so `mount_tool_fs` reuses it
        // instead of installing its own unrooted `FileSystem::local`.
        ctx.provide("fs", fs.clone())
            .await
            .map_err(|e| e.to_string())?;
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

        let _fs_tools = mount_tool_fs(&ctx, fs, read_only_tool_fs_config())
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
            .set("read and verify a file", 10)
            .map_err(|e| e.to_string())?;

        let _provider = llm.register(scripted_provider());
        llm.set_default("scripted");

        let agent = wire_agent_loop(
            llm.clone(),
            tools.clone(),
            pipeline.clone(),
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
            pipeline,
            approval,
            approval_calls,
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
            .run("read and verify a file")
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
    use cordis_rs::core_services::{
        APPROVAL_DENIED_MARKER, CompactionConfig, LlmRequest, PATH_ESCAPE_MARKER, SessionStore,
        TokenMeter, ToolDefinition, ToolError, ToolExecContext, ToolOutput,
    };
    use cordis_rs::events::BoxFuture;

    #[tokio::test]
    async fn scripted_agent_runs_and_renders_steps() {
        let provider = scripted_provider();
        assert_eq!(provider.name(), "scripted");
        assert_eq!(parse_calls("READ_MANIFEST")[0].name, "read");
        assert_eq!(parse_calls("READ_FIXTURE")[0].name, "read");
        assert!(parse_calls("DONE").is_empty());
        // The read-only script never emits a write/edit call.
        assert!(parse_calls("WRITE").is_empty());

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

    /// Regression: the `ToolPipeline` returned by `provide_tools` is retained,
    /// gated by approval, and actually used by `wire_agent_loop`. Both scripted
    /// `read` calls must reach the host answerer; a non-allowed action is
    /// denied before its tool body runs. The DOM rendering contract is intact.
    #[tokio::test]
    async fn scripted_tool_calls_pass_through_the_approval_pipeline() {
        let rig = AgentUiRig::build().await.unwrap();
        assert!(
            rig.approval
                .answerer_names()
                .iter()
                .any(|name| name == "cordis-agent-ui-read-only")
        );

        // Probe tool: if a denied call ever reached the body, this counter moves.
        let body_ran = Arc::new(AtomicUsize::new(0));
        let seen = body_ran.clone();
        let _probe = rig.tools.register(ToolDefinition::new(
            "bash",
            "probe",
            Arc::new(move |_args, _exec| {
                seen.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(ToolOutput::text("unexpected")) })
                    as BoxFuture<'static, Result<ToolOutput, ToolError>>
            }),
        ));

        let run = rig.agent.run("test").await.unwrap();
        assert_eq!(run.steps.len(), 3);
        assert_eq!(run.final_text, "DONE");
        // Two scripted `read` calls, each answered by the host answerer.
        assert_eq!(rig.approval_calls.load(Ordering::SeqCst), 2);
        let results: Vec<_> = run
            .steps
            .iter()
            .flat_map(|step| step.tool_results.iter())
            .collect();
        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .all(|result| result.as_ref().is_ok_and(|output| !output.is_error()))
        );

        // The same retained pipeline denies `tool:bash` before invoking it.
        let denied = rig
            .pipeline
            .run(
                &rig.tools,
                "bash",
                serde_json::json!({}),
                ToolExecContext::anonymous(),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(&denied, ToolError::Execution(msg) if msg.contains(APPROVAL_DENIED_MARKER)),
            "unexpected error: {denied}"
        );
        assert_eq!(body_ran.load(Ordering::SeqCst), 0);
        assert_eq!(rig.approval_calls.load(Ordering::SeqCst), 3);

        // DOM contract: header + three steps + two results + final line.
        let panel = AgentPanel::from_run(&run);
        assert_eq!(panel.lines.len(), 7);
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
        assert!(rig.ctx.get::<ApprovalService>("approval").is_some());
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
                .contains("read and verify a file")
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
            rig.pipeline.clone(),
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

    /// Regression: the host config mounts only `read` and confines it to the
    /// current directory (the manifest lives there). `write`/`edit` are not
    /// registered, and an absolute path outside the root is rejected by the
    /// external crate's scoped filesystem.
    #[tokio::test]
    async fn tool_fs_is_read_only_and_scoped_to_current_directory() {
        let rig = AgentUiRig::build().await.unwrap();
        let names = rig.tools.names();
        assert_eq!(names.iter().filter(|name| *name == "read").count(), 1);
        assert!(!names.iter().any(|name| name == "write"));
        assert!(!names.iter().any(|name| name == "edit"));
        assert!(rig.ctx.get::<FileSystem>("fs").is_some());

        let exec = ToolExecContext::anonymous();
        let ok = rig
            .tools
            .invoke(
                "read",
                serde_json::json!({ "file_path": "Cargo.toml" }),
                exec.clone(),
            )
            .await
            .unwrap();
        assert!(!ok.is_error());
        assert!(
            ok.rendered
                .first()
                .is_some_and(|line| line.contains("<path>Cargo.toml</path>"))
        );

        // `/etc/passwd` is outside the scoped root and is an existing path,
        // so canonicalization cannot accidentally leave it un-normalized.
        let escaped = rig
            .tools
            .invoke(
                "read",
                serde_json::json!({ "file_path": "/etc/passwd" }),
                exec,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(&escaped, ToolError::Execution(msg) if msg.contains(PATH_ESCAPE_MARKER)),
            "unexpected error: {escaped}"
        );
    }

    /// Regression: path policy is fixed by `FileSystem::scoped` without
    /// touching the external crate API. Redundant `..` components normalize
    /// back under the root; a parent traversal and a symlink that resolve to
    /// an existing file outside the root are both rejected.
    #[cfg(unix)]
    #[tokio::test]
    async fn scoped_root_normalizes_paths_and_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "dsh-toolfs-policy-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        let root = base.join("root");
        let subdir = root.join("subdir");
        let outside = base.join("outside.txt");
        std::fs::create_dir_all(&subdir).unwrap();
        std::fs::write(root.join("inside.txt"), "inside\n").unwrap();
        std::fs::write(&outside, "outside\n").unwrap();
        symlink(&outside, root.join("leak.txt")).unwrap();

        let fs = FileSystem::scoped(&root);
        let canonical_root = std::fs::canonicalize(&root).unwrap();

        // Redundant components and an absolute in-root path stay allowed.
        assert_eq!(fs.read_text("subdir/../inside.txt").unwrap(), "inside\n");
        assert_eq!(
            fs.read_text(canonical_root.join("inside.txt").to_str().unwrap())
                .unwrap(),
            "inside\n"
        );

        // Existing targets outside the root are rejected after canonicalization.
        let escape = fs.read_text("../outside.txt").unwrap_err();
        assert!(
            escape.to_string().contains("path escapes filesystem root"),
            "unexpected error: {escape}"
        );
        let symlink_escape = fs.read_text("leak.txt").unwrap_err();
        assert!(
            symlink_escape
                .to_string()
                .contains("path escapes filesystem root"),
            "unexpected error: {symlink_escape}"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn tool_error_output_is_marked_as_error() {
        // The UI hides failing outputs only when is_error() is true; a false
        // negative would show an error as a normal result (swarm review
        // R16-DUAL04).
        let failure = ToolError::Execution("boom".to_string());
        let output = ToolOutput::from_error(&failure);
        assert!(output.is_error());
        let ok = ToolOutput::text("fine");
        assert!(!ok.is_error());
    }
}

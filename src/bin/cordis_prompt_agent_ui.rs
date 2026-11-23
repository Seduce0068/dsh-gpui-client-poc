//! GPUI prompt input wired to a real cordis-rs agent-loop run.
//!
//! Type a prompt and press Enter: the agent runs asynchronously through
//! `cx.spawn`, and its reply is rendered back into the window. Uses a keyless
//! echo provider so no model credentials are required.
//!
//! The loop opts into the shared `sessions`, `systemPrompt`, `goals` and
//! `compaction` services, so turn events, goal interpolation and pressure
//! compaction run through the same core seams as the other cordis demos.

use std::pin::Pin;
use std::sync::Arc;

use cordis_rs::Context;
use cordis_rs::core_services::{
    AgentLoop, CompactionEngine, GoalStore, LlmProvider, LlmService, PromptSection, Session,
    StreamChunk, SystemPrompt, ToolCall, ToolRegistry, provide_compaction, provide_goals,
    provide_llm, provide_sessions, provide_system_prompt, provide_token_meter, provide_tools,
};
use gpui::{
    App, Application, AsyncApp, Context as GpuiContext, KeystrokeEvent, Render, SharedString,
    Window, div, prelude::*, rgb,
};

fn echo_provider() -> LlmProvider {
    LlmProvider::new(
        "echo",
        "echo-1",
        Arc::new(move |request| {
            let last = request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == "user")
                .map(|message| message.content.clone())
                .unwrap_or_default();
            Box::pin(futures::stream::iter(vec![
                StreamChunk::Text(last),
                StreamChunk::Done,
            ])) as Pin<Box<dyn futures::Stream<Item = StreamChunk> + Send + Unpin>>
        }),
    )
}

/// Handles kept alongside the loop so tests can inspect the provided services
/// and the session log the loop writes to. The handles are only read by the
/// regression tests, so release builds allow them to be unused.
#[cfg_attr(not(test), allow(dead_code))]
struct PromptAgentRig {
    ctx: Context,
    session: Session,
    llm: LlmService,
    tools: ToolRegistry,
    agent: Arc<AgentLoop>,
}

/// The single place where the loop opts into the four shared services.
/// Keeping the chain in one function lets the tests exercise exactly the same
/// wiring as the binary.
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
            .with_call_parser(Arc::new(|_text: &str| Vec::<ToolCall>::new()))
            .with_session(session)
            .with_system_prompt(prompt)
            .with_goals(goals)
            .with_compaction(compaction),
    )
}

impl PromptAgentRig {
    async fn build() -> Result<Self, String> {
        let ctx = Context::root("gpui-prompt-agent");
        let (tools, _) = provide_tools(&ctx)
            .await
            .map_err(|error| error.to_string())?;
        let llm = provide_llm(&ctx).await.map_err(|error| error.to_string())?;
        let (sessions, _) = provide_sessions(&ctx)
            .await
            .map_err(|error| error.to_string())?;
        let prompt = provide_system_prompt(&ctx)
            .await
            .map_err(|error| error.to_string())?;
        let goals = provide_goals(&ctx)
            .await
            .map_err(|error| error.to_string())?;
        let meter = provide_token_meter(&ctx)
            .await
            .map_err(|error| error.to_string())?;
        let compaction = provide_compaction(&ctx, meter)
            .await
            .map_err(|error| error.to_string())?;
        let session = sessions
            .create(Some("gpui-prompt-session".to_string()), None)
            .map_err(|error| error.to_string())?;

        // Interpolation is only observable when a section and a live goal
        // exist; register both so `with_goals` has something to inject.
        prompt
            .section(PromptSection::new(
                "gpui-prompt-goal",
                prompt.get_section_order("DEPLOYMENT_PERSONA_PREFIX"),
                "Goal: {{goal}} ({{goal_status}})",
            ))
            .map_err(|error| error.to_string())?;
        goals
            .set("Answer the user's prompt", 10)
            .map_err(|error| error.to_string())?;

        let _provider = llm.register(echo_provider());
        llm.set_default("echo");

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

struct PromptAgentPanel {
    draft: String,
    prompts: Vec<SharedString>,
    replies: Vec<SharedString>,
    busy: bool,
    error: Option<SharedString>,
}

impl PromptAgentPanel {
    fn new() -> Self {
        Self {
            draft: String::new(),
            prompts: Vec::new(),
            replies: Vec::new(),
            busy: false,
            error: None,
        }
    }

    fn push_char(&mut self, ch: char) {
        self.draft.push(ch);
    }

    fn backspace(&mut self) {
        self.draft.pop();
    }

    fn take_prompt(&mut self) -> Option<String> {
        if self.busy || self.draft.trim().is_empty() {
            return None;
        }
        // Send the trimmed text: blank-check and submission must agree
        // (swarm review round 283).
        let prompt = self.draft.trim().to_owned();
        self.draft.clear();
        self.prompts.push(SharedString::from(prompt.clone()));
        self.replies.push(SharedString::from("(running…)"));
        // Clear the previous attempt's error banner when a new prompt starts,
        // so a success after a failure does not keep showing stale red text
        // (swarm review R5-VU01).
        self.error = None;
        self.busy = true;
        Some(prompt)
    }

    fn finish(&mut self, reply: String) {
        if let Some(slot) = self.replies.last_mut() {
            *slot = SharedString::from(reply);
        }
        // A completed reply clears a previous error banner; `fail` sets its
        // own error after calling finish, so its banner survives (swarm review
        // round 283).
        self.error = None;
        self.busy = false;
    }

    fn fail(&mut self, error: String) {
        self.finish(format!("error: {error}"));
        self.error = Some(SharedString::from(error));
    }
}

impl Render for PromptAgentPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let mut pane = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_2()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — agent prompt"))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x80808c))
                    .child("type a prompt, Enter to send — reply comes from the agent loop"),
            );

        for (index, prompt) in self.prompts.iter().enumerate() {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(rgb(0x8ec5ff))
                    .child(SharedString::from(format!("you: {prompt}"))),
            );
            if let Some(reply) = self.replies.get(index) {
                pane = pane.child(
                    div()
                        .text_sm()
                        .text_color(rgb(0x9ce0b0))
                        .child(reply.clone()),
                );
            }
        }

        if let Some(error) = &self.error {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xff8c8c))
                    .child(error.clone()),
            );
        }

        pane.child(
            div()
                .mt_auto()
                .p_3()
                .rounded_md()
                .bg(rgb(0x24242c))
                .text_color(rgb(0xe8e8ef))
                .child(SharedString::from(format!(
                    "> {}{}",
                    self.draft,
                    if self.busy { "  [busy]" } else { "" }
                ))),
        )
    }
}

async fn build_agent() -> Result<Arc<AgentLoop>, String> {
    Ok(PromptAgentRig::build().await?.agent)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let agent = runtime.block_on(build_agent())?;

    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| PromptAgentPanel::new());
        cx.observe_keystrokes({
            let panel = panel.clone();
            let agent = agent.clone();
            move |event: &KeystrokeEvent, _window, app: &mut App| {
                let key = &event.keystroke.key;
                if key == "enter" {
                    let prompt = panel.update(app, |panel, _cx| panel.take_prompt());
                    if let Some(prompt) = prompt {
                        let agent = agent.clone();
                        let panel = panel.clone();
                        let result = std::sync::Arc::clone(&agent);
                        app.spawn(async move |app: &mut AsyncApp| {
                            let run = result.run(prompt).await;
                            let _ = panel.update(app, |panel, _cx| match run {
                                Ok(run) => panel.finish(run.final_text),
                                Err(error) => panel.fail(error.to_string()),
                            });
                        })
                        .detach();
                    }
                } else if key == "backspace" {
                    panel.update(app, |panel, _cx| panel.backspace());
                } else if let Some(ch) = event.keystroke.key_char.as_deref() {
                    for c in ch.chars() {
                        panel.update(app, |panel, _cx| panel.push_char(c));
                    }
                }
            }
        })
        .detach();
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_prompt_agent_ui: GPUI window created");
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cordis_rs::core_services::{CompactionConfig, LlmRequest, SessionStore, TokenMeter};

    #[tokio::test]
    async fn agent_replies_to_a_prompt() {
        let agent = build_agent().await.unwrap();
        let run = agent.run("hello agent").await.unwrap();
        assert_eq!(run.final_text, "hello agent");
        assert_eq!(run.steps.len(), 1);
    }

    #[test]
    fn success_after_failure_clears_error_banner() {
        let mut panel = PromptAgentPanel::new();
        for ch in "hi".chars() {
            panel.push_char(ch);
        }
        assert!(panel.take_prompt().is_some());
        panel.fail("boom".to_string());
        assert!(panel.error.is_some());
        panel.finish("recovered".to_string());
        assert!(panel.error.is_none());
        assert_eq!(panel.replies.last().unwrap().to_string(), "recovered");
    }

    #[test]
    fn take_prompt_sends_trimmed_text() {
        let mut panel = PromptAgentPanel::new();
        for ch in "  hi  ".chars() {
            panel.push_char(ch);
        }
        assert_eq!(panel.take_prompt().unwrap(), "hi");
    }

    #[test]
    fn prompt_state_machine() {
        let mut panel = PromptAgentPanel::new();
        for ch in "hi".chars() {
            panel.push_char(ch);
        }
        let prompt = panel.take_prompt().unwrap();
        assert_eq!(prompt, "hi");
        assert!(panel.busy);
        assert!(panel.take_prompt().is_none()); // busy blocks another submit
        panel.finish("hi".to_string());
        assert!(!panel.busy);
        assert_eq!(panel.replies.last().unwrap().to_string(), "hi");
    }

    /// Regression: the four services must be registered on the context, and
    /// `with_session` must record the full turn lifecycle on that session.
    #[tokio::test]
    async fn shared_services_are_provided_and_turn_is_logged() {
        let rig = PromptAgentRig::build().await.unwrap();
        assert!(rig.ctx.get::<SessionStore>("sessions").is_some());
        assert!(rig.ctx.get::<SystemPrompt>("systemPrompt").is_some());
        assert!(rig.ctx.get::<GoalStore>("goals").is_some());
        assert!(rig.ctx.get::<CompactionEngine>("compaction").is_some());
        assert!(rig.ctx.get::<TokenMeter>("tokenMeter").is_some());

        let run = rig.agent.run("hello agent").await.unwrap();
        assert_eq!(run.final_text, "hello agent");
        let events: Vec<String> = rig
            .session
            .events()
            .into_iter()
            .map(|event| event.event_type)
            .collect();
        assert_eq!(events.first().map(String::as_str), Some("turn/start"));
        assert!(events.iter().any(|event| event == "user/message"));
        assert!(events.iter().any(|event| event == "step/start"));
        assert!(events.iter().any(|event| event == "assistant/message"));
        assert!(events.iter().any(|event| event == "step/end"));
        assert_eq!(events.last().map(String::as_str), Some("turn/end"));
        // Default pressure is far below the threshold; no spurious compaction.
        assert!(!events.iter().any(|event| event == "compaction/checkpoint"));
    }

    /// Regression: `with_system_prompt` plus `with_goals` must put the live
    /// goal into the system message while leaving the user prompt last, which
    /// is what the echo provider (and the DOM text) relies on.
    #[tokio::test]
    async fn goal_is_interpolated_into_the_system_message() {
        let rig = PromptAgentRig::build().await.unwrap();
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

        let run = rig.agent.run("start").await.unwrap();
        assert_eq!(run.final_text, "ok");
        assert_eq!(run.steps.len(), 1);
        let requests = seen.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages[0].role, "system");
        assert!(
            requests[0].messages[0]
                .content
                .contains("Answer the user's prompt")
        );
        assert!(requests[0].messages[0].content.contains("active"));
        assert_eq!(requests[0].messages[1].role, "user");
        assert_eq!(requests[0].messages[1].content, "start");
    }

    /// Regression: the exact `wire_agent_loop` chain used by main must pass a
    /// compaction engine through to the loop. A tiny budget forces pressure so
    /// the checkpoint event can only appear if `with_compaction` is connected.
    #[tokio::test]
    async fn wired_compaction_appends_a_checkpoint_under_pressure() {
        let rig = PromptAgentRig::build().await.unwrap();
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
        agent
            .run("a long prompt that exceeds the tiny budget")
            .await
            .unwrap();
        assert!(
            rig.session
                .events()
                .iter()
                .any(|event| event.event_type == "compaction/checkpoint")
        );
    }
}

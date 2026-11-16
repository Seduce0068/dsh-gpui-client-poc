//! GPUI prompt input wired to a real cordis-rs agent-loop run.
//!
//! Type a prompt and press Enter: the agent runs asynchronously through
//! `cx.spawn`, and its reply is rendered back into the window. Uses a keyless
//! echo provider so no model credentials are required.

use std::pin::Pin;
use std::sync::Arc;

use cordis_rs::Context;
use cordis_rs::core_services::{
    AgentLoop, LlmProvider, StreamChunk, ToolCall, provide_llm, provide_tools,
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
        let prompt = self.draft.clone();
        self.draft.clear();
        self.prompts.push(SharedString::from(prompt.clone()));
        self.replies.push(SharedString::from("(running…)"));
        self.busy = true;
        Some(prompt)
    }

    fn finish(&mut self, reply: String) {
        if let Some(slot) = self.replies.last_mut() {
            *slot = SharedString::from(reply);
        }
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
    let ctx = Context::root("gpui-prompt-agent");
    let (tools, _) = provide_tools(&ctx)
        .await
        .map_err(|error| error.to_string())?;
    let llm = provide_llm(&ctx).await.map_err(|error| error.to_string())?;
    let _provider = llm.register(echo_provider());
    llm.set_default("echo");
    Ok(Arc::new(AgentLoop::new(llm, tools).with_call_parser(
        Arc::new(|_text: &str| Vec::<ToolCall>::new()),
    )))
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

    #[tokio::test]
    async fn agent_replies_to_a_prompt() {
        let agent = build_agent().await.unwrap();
        let run = agent.run("hello agent").await.unwrap();
        assert_eq!(run.final_text, "hello agent");
        assert_eq!(run.steps.len(), 1);
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
}

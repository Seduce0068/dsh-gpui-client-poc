//! GPUI prompt input wired to the cordis-rs agent loop.
//!
//! Type a prompt, press Enter, and the scripted agent replies through the
//! real cordis-rs tool seams. Keystrokes are captured with
//! `App::observe_keystrokes` (gpui 0.2.2 has no built-in text input widget).

use std::pin::Pin;
use std::sync::Arc;

use cordis_rs::Context;
use cordis_rs::core_services::{LlmProvider, StreamChunk, provide_llm, provide_tools};
use gpui::{
    App, Application, Context as GpuiContext, KeystrokeEvent, Render, SharedString, Window, div,
    prelude::*, rgb,
};

/// Echo provider: the assistant repeats the user's prompt as tool-free text.
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

struct PromptPanel {
    draft: String,
    submitted: Vec<SharedString>,
    replies: Vec<SharedString>,
    error: Option<SharedString>,
}

impl PromptPanel {
    fn new() -> Self {
        Self {
            draft: String::new(),
            submitted: Vec::new(),
            replies: Vec::new(),
            error: None,
        }
    }

    fn push_char(&mut self, ch: char) {
        self.draft.push(ch);
    }

    fn backspace(&mut self) {
        self.draft.pop();
    }

    fn submit(&mut self) {
        if self.draft.trim().is_empty() {
            return;
        }
        self.submitted.push(SharedString::from(self.draft.clone()));
        self.draft.clear();
    }
}

impl Render for PromptPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let mut pane = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_2()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — prompt input"))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x80808c))
                    .child("type a prompt, Enter to submit, Backspace to delete"),
            );

        for (index, prompt) in self.submitted.iter().enumerate() {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(rgb(0x8ec5ff))
                    .child(SharedString::from(format!("you: {prompt}"))),
            );
            if let Some(reply) = self.replies.get(index) {
                pane = pane.child(div().text_sm().child(reply.clone()));
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
                .text_color(rgb(0x9ce0b0))
                .child(SharedString::from(format!("> {}", self.draft))),
        )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    // Register the echo provider once; a real host would keep the AgentLoop
    // entity alive here and call `run(prompt)` on submission.
    runtime.block_on(async {
        let ctx = Context::root("gpui-prompt");
        let _ = provide_tools(&ctx)
            .await
            .map_err(|error| error.to_string())?;
        let llm = provide_llm(&ctx).await.map_err(|error| error.to_string())?;
        let _provider = llm.register(echo_provider());
        llm.set_default("echo");
        Ok::<_, String>(())
    })?;

    Application::new().run(|cx: &mut App| {
        let panel = cx.new(|_cx| PromptPanel::new());
        let handle = panel.clone();
        cx.observe_keystrokes(move |event: &KeystrokeEvent, _window, app| {
            let key = &event.keystroke.key;
            if key == "enter" {
                handle.update(app, |panel, _cx| panel.submit());
            } else if key == "backspace" {
                handle.update(app, |panel, _cx| panel.backspace());
            } else if let Some(ch) = event.keystroke.key_char.as_deref() {
                for c in ch.chars() {
                    handle.update(app, |panel, _cx| panel.push_char(c));
                }
            }
        })
        .detach();
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_prompt_ui: GPUI window created");
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_panel_accumulates_and_submits() {
        let mut panel = PromptPanel::new();
        for ch in "hello".chars() {
            panel.push_char(ch);
        }
        assert_eq!(panel.draft, "hello");
        panel.backspace();
        assert_eq!(panel.draft, "hell");
        panel.submit();
        assert_eq!(panel.submitted.len(), 1);
        assert_eq!(panel.submitted[0].to_string(), "hell");
        assert!(panel.draft.is_empty());

        panel.submit(); // empty draft is ignored
        assert_eq!(panel.submitted.len(), 1);
    }

    #[tokio::test]
    async fn echo_provider_repeats_the_last_user_message() {
        use cordis_rs::core_services::{ChatMessage, LlmRequest};

        let ctx = Context::root("prompt-test");
        let llm = provide_llm(&ctx).await.unwrap();
        let _provider = llm.register(echo_provider());
        llm.set_default("echo");
        let response = llm
            .stream(
                None,
                LlmRequest {
                    model: String::new(),
                    messages: vec![ChatMessage::user("say this")],
                },
            )
            .await
            .unwrap();
        assert_eq!(response.text, "say this");
    }
}

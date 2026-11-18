//! GPUI prompt input wired to the cordis-rs agent loop.
//!
//! Type a prompt, press Enter, and the scripted agent replies through the
//! real cordis-rs tool seams. Keystrokes are captured with
//! `App::observe_keystrokes` (gpui 0.2.2 has no built-in text input widget).
//!
//! IME 変換中は Enter を送信に使わない。gpui 0.2.2 の `Keystroke` には IME
//! 状態フラグが無いため、`Keystroke::is_ime_in_progress()` で変換フェーズを
//! 追跡し、変換確定の Enter を送信から分離する。

use std::pin::Pin;
use std::sync::Arc;

use cordis_rs::Context;
use cordis_rs::core_services::{LlmProvider, StreamChunk, provide_llm, provide_tools};
use gpui::{
    App, Application, Context as GpuiContext, Keystroke, KeystrokeEvent, Render, SharedString,
    Window, div, prelude::*, rgb,
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

/// IME 変換フェーズ。`observe_keystrokes` から見える情報だけで追跡する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImePhase {
    Idle,
    Composing,
}

/// キーストロークが下書きに与える意味。
#[derive(Clone, Debug, PartialEq, Eq)]
enum PromptKeyAction {
    /// 通常の Enter: 下書きを送信する。
    Submit,
    /// IME 変換確定の Enter: 送信せず、変換フェーズを終了するだけ。
    ImeConfirm,
    /// 文字列を下書きへ挿入する。
    Insert(String),
    /// 下書きの末尾を削除する。
    Backspace,
    /// 下書きに影響しない (変換中の preedit / ナビゲーション等)。
    Ignore,
}

/// キーストロークと現在の IME フェーズから、取るべき操作と次のフェーズを決める。
///
/// gpui 0.2.2 の `Keystroke` は IME 状態フラグを持たない。代わりに
/// `is_ime_in_progress()`（印字キーが `key_char` を伴わない＝ IME が preedit を
/// 保持している）で変換中を検出する。変換中の修飾なし Enter は、IME が
/// `key_char` を消費する場合もあれば "\n" を残す場合もあるため、`key_char` の
/// 有無に依存せず「変換確定」として扱い、決して送信しない。
fn interpret_keystroke(keystroke: &Keystroke, phase: ImePhase) -> (PromptKeyAction, ImePhase) {
    let key = keystroke.key.as_str();
    let modified = keystroke.modifiers.control
        || keystroke.modifiers.alt
        || keystroke.modifiers.platform
        || keystroke.modifiers.function;

    if key == "escape" {
        // Esc は IME 変換をキャンセルする。下書きは変更しない。
        return (PromptKeyAction::Ignore, ImePhase::Idle);
    }

    if key == "enter" {
        // 変換中の修飾なし Enter は IME の確定/キャンセル。key_char の有無に
        // かかわらず送信せず、フェーズだけ Idle に戻す。
        if !modified && phase == ImePhase::Composing {
            return (PromptKeyAction::ImeConfirm, ImePhase::Idle);
        }
        // 通常の Enter（変換中でない / 修飾付きショートカット）は従来どおり送信。
        return (PromptKeyAction::Submit, ImePhase::Idle);
    }

    // 印字キーが key_char を伴わない場合、IME が preedit を保持している。
    if keystroke.is_ime_in_progress() {
        return (PromptKeyAction::Ignore, ImePhase::Composing);
    }

    if phase == ImePhase::Composing {
        // 文字を伴うキーが来たら変換は終了した。確定文字を下書きへ反映する。
        // preedit 内の移動/削除など文字を伴わないキーは変換中を維持する。
        return match &keystroke.key_char {
            Some(text) => (PromptKeyAction::Insert(text.clone()), ImePhase::Idle),
            None => (PromptKeyAction::Ignore, ImePhase::Composing),
        };
    }

    if key == "backspace" && !modified {
        return (PromptKeyAction::Backspace, ImePhase::Idle);
    }

    if let Some(text) = &keystroke.key_char {
        return (PromptKeyAction::Insert(text.clone()), ImePhase::Idle);
    }

    (PromptKeyAction::Ignore, ImePhase::Idle)
}

struct PromptPanel {
    draft: String,
    submitted: Vec<SharedString>,
    replies: Vec<SharedString>,
    error: Option<SharedString>,
    ime_phase: ImePhase,
}

impl PromptPanel {
    fn new() -> Self {
        Self {
            draft: String::new(),
            submitted: Vec::new(),
            replies: Vec::new(),
            error: None,
            ime_phase: ImePhase::Idle,
        }
    }

    fn handle_keystroke(&mut self, keystroke: &Keystroke) {
        let (action, next_phase) = interpret_keystroke(keystroke, self.ime_phase);
        self.ime_phase = next_phase;
        match action {
            PromptKeyAction::Submit => self.submit(),
            PromptKeyAction::ImeConfirm => {
                // 確定文字列は本来 InputHandler 経由で入るが、この PoC は
                // observe_keystrokes のみなので、ここでは下書きを変更しない。
                // 重要なのは変換確定の Enter で送信しないこと。
            }
            PromptKeyAction::Insert(text) => {
                for ch in text.chars() {
                    self.push_char(ch);
                }
            }
            PromptKeyAction::Backspace => self.backspace(),
            PromptKeyAction::Ignore => {}
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
                    .child("type a prompt, Enter to submit (IME confirmation won't submit), Backspace to delete"),
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
            handle.update(app, |panel, _cx| panel.handle_keystroke(&event.keystroke));
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

    fn key(key: &str, key_char: Option<&str>) -> Keystroke {
        Keystroke {
            key: key.to_string(),
            key_char: key_char.map(str::to_string),
            modifiers: Default::default(),
        }
    }

    fn type_text(panel: &mut PromptPanel, text: &str) {
        for ch in text.chars() {
            let ch = ch.to_string();
            panel.handle_keystroke(&key(&ch, Some(&ch)));
        }
    }

    #[test]
    fn plain_enter_submits_as_before() {
        let mut panel = PromptPanel::new();
        type_text(&mut panel, "hello");
        assert_eq!(panel.ime_phase, ImePhase::Idle);

        panel.handle_keystroke(&key("enter", Some("\n")));
        assert_eq!(panel.submitted.len(), 1);
        assert_eq!(panel.submitted[0].to_string(), "hello");
        assert!(panel.draft.is_empty());
        assert_eq!(panel.ime_phase, ImePhase::Idle);
    }

    #[test]
    fn ime_confirmation_enter_does_not_submit() {
        let mut panel = PromptPanel::new();
        type_text(&mut panel, "hello ");

        // 日本語入力の romaji preedit: IME が文字を消費し key_char が None になる。
        for k in ["k", "a", "n", "j", "i"] {
            panel.handle_keystroke(&key(k, None));
            assert_eq!(panel.ime_phase, ImePhase::Composing);
        }
        // preedit は下書きを汚さない。
        assert_eq!(panel.draft, "hello ");

        // 変換確定の Enter（修飾なし・key_char なし）は送信しない。
        panel.handle_keystroke(&key("enter", None));
        assert!(panel.submitted.is_empty());
        assert_eq!(panel.draft, "hello ");
        assert_eq!(panel.ime_phase, ImePhase::Idle);

        // 確定後の通常 Enter は従来どおり送信する。
        panel.handle_keystroke(&key("enter", Some("\n")));
        assert_eq!(panel.submitted.len(), 1);
        assert_eq!(panel.submitted[0].to_string(), "hello ");
    }

    #[test]
    fn ime_enter_with_newline_key_char_never_submits() {
        let mut panel = PromptPanel::new();
        type_text(&mut panel, "まだ変換中です");
        panel.handle_keystroke(&key("n", None)); // preedit 開始
        assert_eq!(panel.ime_phase, ImePhase::Composing);

        // プラットフォームが Enter に "\n" を残しても変換中なら送信しない。
        panel.handle_keystroke(&key("enter", Some("\n")));
        assert!(panel.submitted.is_empty());
        assert_eq!(panel.draft, "まだ変換中です");

        // 次の Enter は確定後の通常送信として扱われる。
        panel.handle_keystroke(&key("enter", Some("\n")));
        assert_eq!(panel.submitted.len(), 1);
    }

    #[test]
    fn interpret_distinguishes_plain_and_ime_enter() {
        let plain = key("enter", Some("\n"));
        assert_eq!(
            interpret_keystroke(&plain, ImePhase::Idle),
            (PromptKeyAction::Submit, ImePhase::Idle)
        );
        // 変換中は key_char が "\n" でも送信しない（中途送信防止を優先）。
        assert_eq!(
            interpret_keystroke(&plain, ImePhase::Composing),
            (PromptKeyAction::ImeConfirm, ImePhase::Idle)
        );

        let confirm = key("enter", None);
        assert_eq!(
            interpret_keystroke(&confirm, ImePhase::Composing),
            (PromptKeyAction::ImeConfirm, ImePhase::Idle)
        );
        assert_eq!(
            interpret_keystroke(&confirm, ImePhase::Idle),
            (PromptKeyAction::Submit, ImePhase::Idle)
        );

        // 修飾付き Enter は IME 確定ではないので既存ショートカットとして送信する。
        let mut modified = key("enter", None);
        modified.modifiers.control = true;
        assert_eq!(
            interpret_keystroke(&modified, ImePhase::Composing),
            (PromptKeyAction::Submit, ImePhase::Idle)
        );
    }

    #[test]
    fn composition_edit_keys_do_not_mutate_committed_draft() {
        let mut panel = PromptPanel::new();
        type_text(&mut panel, "hi");

        panel.handle_keystroke(&key("k", None)); // preedit 開始
        panel.handle_keystroke(&key("backspace", None)); // preedit 内の削除
        assert_eq!(panel.draft, "hi");
        assert_eq!(panel.ime_phase, ImePhase::Composing);

        panel.handle_keystroke(&key("escape", None)); // 変換キャンセル
        assert_eq!(panel.ime_phase, ImePhase::Idle);
        panel.handle_keystroke(&key("backspace", None)); // 今度は下書きを削除
        assert_eq!(panel.draft, "h");
        assert!(panel.submitted.is_empty());
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

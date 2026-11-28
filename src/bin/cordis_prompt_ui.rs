//! GPUI prompt input and submitted-prompt display.
//!
//! Type a prompt and press Enter to append it to the submitted list; this
//! demo does not run the cordis-rs AgentLoop, so no reply is produced. An
//! echo provider is registered in the cordis context at startup, but
//! `run(prompt)` is never called on submission (see `main`). Use
//! `cordis_prompt_agent_ui` for the agent-loop-driven variant.
//!
//! Keystrokes are captured with `App::observe_keystrokes` (gpui 0.2.2 has no
//! built-in text input widget); committed characters are inserted through
//! `EntityInputHandler`.
//!
//! IME 変換中は Enter を送信に使わない。gpui 0.2.2 の `Keystroke` には IME
//! 状態フラグが無いため、`Keystroke::is_ime_in_progress()` で変換フェーズを
//! 追跡し、変換確定の Enter を送信から分離する（中途送信防止契約）。
//!
//! 変換確定漢字の下書きへの挿入は `EntityInputHandler` / `InputHandler` が担う。
//! gpui は印字キーを `observe_keystrokes` と `InputHandler::replace_text_in_range`
//! の両方へ配るため、キーストローク側は文字を挿入せず制御キーのみを処理し、
//! 文字の挿入は InputHandler に一本化する。

use std::ops::Range;
use std::pin::Pin;
use std::sync::Arc;

use cordis_rs::Context;
use cordis_rs::core_services::{LlmProvider, StreamChunk, provide_llm, provide_tools};
use gpui::{
    App, Application, Bounds, Context as GpuiContext, ElementInputHandler, EntityInputHandler,
    FocusHandle, Keystroke, KeystrokeEvent, Pixels, Point, Render, SharedString, UTF16Selection,
    Window, canvas, div, prelude::*, rgb,
};

/// Single source for the window-creation log line, kept pure so the
/// success/failure wording is regression-tested.
fn window_log_line(opened: bool, detail: &str) -> String {
    if opened {
        "GPUI window created".to_string()
    } else {
        format!("failed to create window: {detail}")
    }
}

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

/// UTF-16 オフセット以下で最大の `String` バイト境界を返す。
/// サロゲートペアの途中を指すオフセットでも文字境界へ丸める。
fn floor_byte_index(text: &str, utf16_offset: usize) -> usize {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        let width = character.len_utf16();
        if units + width > utf16_offset {
            return byte;
        }
        units += width;
        if units == utf16_offset {
            return byte + character.len_utf8();
        }
    }
    text.len()
}

/// バイト境界に対応する UTF-16 オフセットを返す。
fn utf16_offset(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
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
    /// 確定済み文字列。送信対象はこれだけ。
    draft: String,
    /// IME の未確定文字列 (preedit)。下書きには含めない。
    preedit: String,
    submitted: Vec<SharedString>,
    replies: Vec<SharedString>,
    error: Option<SharedString>,
    ime_phase: ImePhase,
    /// `Window::handle_input` に渡す入力先を識別するためのハンドル。
    /// ヘッドレスのロジックテストでは `None` (InputHandler 接続は行わない)。
    focus_handle: Option<FocusHandle>,
}

impl PromptPanel {
    fn new() -> Self {
        Self {
            draft: String::new(),
            preedit: String::new(),
            submitted: Vec::new(),
            replies: Vec::new(),
            error: None,
            ime_phase: ImePhase::Idle,
            focus_handle: None,
        }
    }

    /// `observe_keystrokes` 用。文字の挿入は InputHandler に一本化するため、
    /// `Insert` は無視し、制御キー (送信 / 削除 / IME フェーズ) だけを処理する。
    /// これにより gpui が印字キーを両経路へ配っても二重挿入にならない。
    fn handle_keystroke(&mut self, keystroke: &Keystroke) {
        let (action, next_phase) = interpret_keystroke(keystroke, self.ime_phase);
        self.ime_phase = next_phase;
        match action {
            PromptKeyAction::Submit => self.submit(),
            PromptKeyAction::ImeConfirm => {
                // 確定文字列は InputHandler::replace_text_in_range が既に下書きへ
                // 入れている。ここでは送信せず、フェーズを Idle に戻すだけ。
            }
            // InputHandler が文字の挿入を担当する（下方的互換のため action 自体は残す）。
            PromptKeyAction::Insert(_) | PromptKeyAction::Ignore => {}
            PromptKeyAction::Backspace => self.backspace(),
        }
    }

    /// InputHandler から届いた確定文字列を下書きへ挿入する。
    /// `ime_phase` は変更しない。変換確定の Enter がキーストロークとして届いた
    /// ときに `ImeConfirm` として扱い、中途送信を防ぐためである。
    fn insert_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.push_char(ch);
        }
        self.clear_preedit();
    }

    /// IME の preedit を更新する（下書きは変更しない）。
    fn set_preedit(&mut self, text: &str) {
        self.preedit.clear();
        self.preedit.push_str(text);
    }

    /// preedit だけを捨てる（変換キャンセル）。確定済み下書きは不変。
    fn clear_preedit(&mut self) {
        self.preedit.clear();
    }

    /// preedit の UTF-16 範囲。`InputHandler::marked_text_range` が返す値。
    fn marked_range(&self) -> Option<Range<usize>> {
        if self.preedit.is_empty() {
            return None;
        }
        let start = utf16_offset(&self.draft, self.draft.len());
        Some(start..start + utf16_offset(&self.preedit, self.preedit.len()))
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
        // Keep the untrimmed draft on submit: a trailing space from an IME
        // confirmation is part of the accepted text (see IME regression tests).
        self.submitted.push(SharedString::from(self.draft.clone()));
        self.draft.clear();
    }

    /// UTF-16 で見た表示テキスト (確定 + preedit) の長さ。
    fn display_utf16_len(&self) -> usize {
        utf16_offset(&self.draft, self.draft.len())
            + utf16_offset(&self.preedit, self.preedit.len())
    }

    /// 表示テキストを UTF-16 オフセットで切り出す。丸めた実際の範囲も返す。
    fn text_for_display_range(&self, range: Range<usize>) -> (String, Range<usize>) {
        let display = format!("{}{}", self.draft, self.preedit);
        let len = utf16_offset(&display, display.len());
        let start = range.start.min(len);
        let end = range.end.min(len).max(start);
        let byte_start = floor_byte_index(&display, start);
        let byte_end = floor_byte_index(&display, end).max(byte_start);
        let actual = utf16_offset(&display, byte_start)..utf16_offset(&display, byte_end);
        (display[byte_start..byte_end].to_string(), actual)
    }
}

impl Render for PromptPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut GpuiContext<Self>) -> impl IntoElement {
        // paint 中にしか handle_input を呼べないため、canvas の paint で登録する。
        let input_focus_handle = self.focus_handle.clone();
        let draft = self.draft.clone();
        let preedit = self.preedit.clone();

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

        let mut input_box = div()
            .mt_auto()
            .p_3()
            .rounded_md()
            .bg(rgb(0x24242c))
            .text_color(rgb(0x9ce0b0))
            .child(SharedString::from(format!("> {draft}{preedit}")))
            .when(!preedit.is_empty(), |box_view| {
                box_view.child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xd0a0ff))
                        .child(SharedString::from(format!("[IME: {preedit}]"))),
                )
            });

        if let Some(focus_handle) = &input_focus_handle {
            input_box = input_box.track_focus(focus_handle);
        }
        pane = pane.child(input_box);

        if let Some(focus_handle) = input_focus_handle {
            let input_view = cx.entity();
            pane = pane.child(canvas(
                |bounds, _window, _cx| bounds,
                move |bounds, _prepaint, window, cx| {
                    window.handle_input(
                        &focus_handle,
                        ElementInputHandler::new(bounds, input_view.clone()),
                        cx,
                    );
                },
            ));
        }
        pane
    }
}

/// GPUI の IME テキスト入力経路。`replace_text_in_range` が確定文字列を
/// 下書きへ挿入し、`replace_and_mark_text_in_range` / `unmark_text` は preedit
/// だけを扱う。変換中の文字は確定されるまで下書きを汚さない。
impl EntityInputHandler for PromptPanel {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut GpuiContext<Self>,
    ) -> Option<String> {
        let (text, actual) = self.text_for_display_range(range);
        *adjusted_range = Some(actual);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut GpuiContext<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.display_utf16_len();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut GpuiContext<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range()
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) {
        self.clear_preedit();
    }

    fn replace_text_in_range(
        &mut self,
        _replacement_range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        _cx: &mut GpuiContext<Self>,
    ) {
        // IME の確定 (insertText) と通常の印字キーはここを通る。
        self.insert_text(text);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut GpuiContext<Self>,
    ) {
        // 変換中文字列は preedit として保持し、下書きへは入れない。
        self.set_preedit(new_text);
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut GpuiContext<Self>,
    ) -> Option<Bounds<Pixels>> {
        // 変換候補ウィンドウは入力欄の矩形に重ねる。
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut GpuiContext<Self>,
    ) -> Option<usize> {
        None
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
        let panel = cx.new(|cx| {
            let mut panel = PromptPanel::new();
            panel.focus_handle = Some(cx.focus_handle());
            panel
        });
        let handle = panel.clone();
        cx.observe_keystrokes(move |event: &KeystrokeEvent, _window, app| {
            handle.update(app, |panel, _cx| panel.handle_keystroke(&event.keystroke));
        })
        .detach();
        let root = panel.clone();
        match cx.open_window(Default::default(), |window, app| {
            // 入力欄をフォーカスしないと handle_input が登録されず、IME 確定文字が届かない。
            if let Some(focus_handle) = &root.read(app).focus_handle {
                focus_handle.focus(window);
            }
            root.clone()
        }) {
            Ok(_window) => eprintln!("cordis_prompt_ui: {}", window_log_line(true, "")),
            Err(error) => eprintln!(
                "cordis_prompt_ui: {}",
                window_log_line(false, &error.to_string())
            ),
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_log_line_success_keeps_existing_message() {
        assert_eq!(window_log_line(true, ""), "GPUI window created");
    }

    #[test]
    fn window_log_line_failure_reports_error_detail() {
        assert_eq!(
            window_log_line(false, "display unavailable"),
            "failed to create window: display unavailable"
        );
    }

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
        // 本番では InputHandler::replace_text_in_range が文字を挿入する。
        panel.insert_text(text);
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

    #[test]
    fn input_handler_commit_inserts_kanji_into_draft() {
        let mut panel = PromptPanel::new();
        panel.insert_text("今日は");

        // 変換中は preedit だけを保持し、下書きと送信結果は変わらない。
        panel.set_preedit("にほんご");
        assert_eq!(panel.draft, "今日は");
        assert_eq!(panel.marked_range(), Some(3..7)); // "今日は" は UTF-16 で 3 ユニット
        assert_eq!(panel.text_for_display_range(3..7).0, "にほんご");
        assert!(panel.submitted.is_empty());

        // 確定 (replace_text_in_range) で漢字が下書きへ入る。
        panel.insert_text("日本語");
        assert_eq!(panel.draft, "今日は日本語");
        assert_eq!(panel.preedit, "");
        assert_eq!(panel.marked_range(), None);
        assert!(panel.submitted.is_empty());
    }

    #[test]
    fn ime_commit_then_confirm_enter_still_does_not_submit() {
        let mut panel = PromptPanel::new();
        panel.insert_text("hello ");

        // キーストローク側からは key_char が見えない preedit 開始。
        for k in ["k", "a", "n", "j", "i"] {
            panel.handle_keystroke(&key(k, None));
        }
        assert_eq!(panel.ime_phase, ImePhase::Composing);
        panel.set_preedit("kanji");
        assert_eq!(panel.draft, "hello ");

        // InputHandler が確定漢字を下書きへ入れても ime_phase は Composing のまま。
        panel.insert_text("漢字");
        assert_eq!(panel.draft, "hello 漢字");
        assert_eq!(panel.ime_phase, ImePhase::Composing);
        assert!(panel.submitted.is_empty());

        // 確定 Enter に "\n" が残っていても中途送信しない（既存契約）。
        panel.handle_keystroke(&key("enter", Some("\n")));
        assert!(panel.submitted.is_empty());
        assert_eq!(panel.draft, "hello 漢字");
        assert_eq!(panel.ime_phase, ImePhase::Idle);

        // 次の Enter で確定文字列を含む下書きを送信する。
        panel.handle_keystroke(&key("enter", Some("\n")));
        assert_eq!(panel.submitted.len(), 1);
        assert_eq!(panel.submitted[0].to_string(), "hello 漢字");
    }

    #[test]
    fn keystroke_insert_does_not_duplicate_input_handler_text() {
        // gpui は印字キーを observe_keystrokes と InputHandler の両方へ配る。
        // キー側が Insert を無視するため二重挿入にならない。
        let mut panel = PromptPanel::new();
        panel.handle_keystroke(&key("a", Some("a")));
        assert!(panel.draft.is_empty());
        panel.insert_text("a");
        assert_eq!(panel.draft, "a");
    }

    #[test]
    fn cancel_preedit_keeps_committed_draft() {
        let mut panel = PromptPanel::new();
        panel.insert_text("abc");
        panel.set_preedit("にほん");
        assert_eq!(panel.draft, "abc");

        panel.clear_preedit();
        assert_eq!(panel.draft, "abc");
        assert_eq!(panel.marked_range(), None);
        assert!(panel.submitted.is_empty());
    }

    #[test]
    fn input_handler_does_not_submit_when_committing() {
        let mut panel = PromptPanel::new();
        panel.handle_keystroke(&key("k", None)); // preedit 開始
        assert_eq!(panel.ime_phase, ImePhase::Composing);
        panel.insert_text("火");
        assert!(panel.submitted.is_empty());
        assert_eq!(panel.draft, "火");
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

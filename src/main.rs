//! PoC: a minimal GPUI-native chat client shell for DeepSeek Harness.
//!
//! This intentionally renders only a local, static conversation. It is a
//! UI-shell feasibility check, not a working DSH client: the Host RPC and
//! browser authentication layer are deliberately out of scope (see README).

use gpui::{App, Application, Context, Render, SharedString, Window, div, prelude::*, px, rgb};

/// One chat message in the local demo conversation.
struct Message {
    role: &'static str,
    text: &'static str,
}

/// Root view: a two-pane DSH-like layout (session list + conversation).
struct ChatShell {
    sessions: Vec<SharedString>,
    messages: Vec<Message>,
}

impl ChatShell {
    fn new() -> Self {
        Self {
            sessions: vec![
                "gpui 化の検討".into(),
                "DSH 接続レイヤの調査".into(),
                "Rust 依存監査".into(),
            ],
            messages: vec![
                Message {
                    role: "user",
                    text: "DSH のクライアントを GPUI 化できる？",
                },
                Message {
                    role: "assistant",
                    text: "全面置換は非推奨です。ブラウザ前提のプラグインと認証レイヤを保持したまま、\
                           別途ネイティブクライアントを作る案が現実的です。",
                },
                Message {
                    role: "user",
                    text: "まず UI の骨格だけ見せて。",
                },
            ],
        }
    }

    fn sidebar(&self) -> impl IntoElement {
        let mut list = div()
            .flex()
            .flex_col()
            .w(px(260.0))
            .h_full()
            .bg(rgb(0x1e1e24))
            .border_r_1()
            .border_color(rgb(0x35353e))
            .p_3()
            .gap_2()
            .child(div().text_lg().text_color(rgb(0xf0f0f5)).child("Sessions"));
        for name in &self.sessions {
            list = list.child(
                div()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(0x2a2a33))
                    .text_color(rgb(0xd0d0da))
                    .child(name.clone()),
            );
        }
        list
    }

    fn conversation(&self) -> impl IntoElement {
        let mut pane = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_3();
        for message in &self.messages {
            let is_user = message.role == "user";
            pane = pane.child(
                div()
                    .p_3()
                    .rounded_lg()
                    .bg(if is_user {
                        rgb(0x2b3a4a)
                    } else {
                        rgb(0x24242c)
                    })
                    .text_color(rgb(0xe8e8ef))
                    .child(
                        div()
                            .text_sm()
                            .text_color(if is_user {
                                rgb(0x8ec5ff)
                            } else {
                                rgb(0x9ce0b0)
                            })
                            .child(message.role),
                    )
                    .child(div().mt_1().child(message.text)),
            );
        }
        pane.child(
            div()
                .mt_auto()
                .p_3()
                .rounded_lg()
                .bg(rgb(0x1e1e24))
                .border_1()
                .border_color(rgb(0x35353e))
                .text_color(rgb(0x80808c))
                .child("Message DSH... (input not wired in PoC)"),
        )
    }
}

impl Render for ChatShell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .text_color(rgb(0xe8e8ef))
            .child(self.sidebar())
            .child(self.conversation())
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let _window = cx.open_window(Default::default(), |_window, cx| {
            cx.new(|_cx| ChatShell::new())
        });
    });
}

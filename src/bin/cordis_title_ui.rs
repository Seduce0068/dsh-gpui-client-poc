//! GPUI demo: LLM-generated session title vs deterministic fallback.
//!
//! A stub LLM returns an emphasized title line; the title port parses it and
//! stores it on `ctx.sessionTitles`. A second session has no LLM output and
//! keeps its first-message fallback title.

use std::sync::Arc;

use cordis_rs::Context;
use cordis_rs::core_services::{
    LlmRequest, SessionTitles, TitleProviderConfig, provide_llm, provide_session_titles,
};
use cordis_rs::plugin_ports::{SessionTitleLlm, mount_session_title_llm};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

struct TitleRow {
    session_id: String,
    title: String,
    source: &'static str,
}

fn build_rows() -> Result<Vec<TitleRow>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        let ctx = Context::root("title-ui");
        let llm = provide_llm(&ctx).await.map_err(|e| e.to_string())?;
        let titles: SessionTitles = provide_session_titles(&ctx)
            .await
            .map_err(|e| e.to_string())?;
        let config = TitleProviderConfig::default();
        mount_session_title_llm(&ctx, config.clone())
            .await
            .map_err(|e| e.to_string())?;

        let provider = cordis_rs::core_services::from_async_text(
            "title-stub",
            "title-stub-1",
            |_request: LlmRequest| async { Ok::<_, String>("**Rust Plugin Port**".to_string()) },
        );
        let _p = llm.register(provider);
        llm.set_default("title-stub");

        let generator = SessionTitleLlm::new(config.clone());
        let messages = vec!["How do I port plugins?".to_string()];
        let _ = generator
            .generate(&llm, &titles, "s-llm", &messages)
            .await
            .map_err(|e| e.to_string())?;

        titles.derive_from_messages("s-fallback", &["Greeting and intro".to_string()], 20);

        Ok(vec![
            TitleRow {
                session_id: "s-llm".to_string(),
                title: titles.get("s-llm").unwrap_or_default(),
                source: "llm",
            },
            TitleRow {
                session_id: "s-fallback".to_string(),
                title: titles.get("s-fallback").unwrap_or_default(),
                source: "fallback",
            },
        ])
    })
}

struct TitlePanel {
    rows: Arc<Vec<TitleRow>>,
}

impl Render for TitlePanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let mut pane = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_3()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — session titles"));
        for row in self.rows.iter() {
            pane = pane.child(div().p_3().rounded_md().bg(rgb(0x24242c)).child(
                SharedString::from(format!(
                    "{} [{}]: {}",
                    row.session_id, row.source, row.title
                )),
            ));
        }
        pane
    }
}

fn main() {
    let rows = Arc::new(build_rows().unwrap_or_default());
    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| TitlePanel { rows: rows.clone() });
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_title_ui: GPUI window created");
    });
}

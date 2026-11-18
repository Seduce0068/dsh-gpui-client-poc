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
    App, Application, AsyncApp, Context as GpuiContext, Entity, Render, SharedString, Window, div,
    prelude::*, rgb,
};

struct TitleRow {
    session_id: String,
    title: String,
    source: &'static str,
}

/// Stable label for a window row. Kept pure so the existing
/// `session [source]: title` DOM hook is covered by a regression test.
fn row_label(row: &TitleRow) -> String {
    format!("{} [{}]: {}", row.session_id, row.source, row.title)
}

/// Initialization failures are rendered in the window instead of being
/// silently converted into an empty row list.
fn init_error_line(error: &str) -> String {
    format!("init error: {error}")
}

/// Async by itself: the caller is the GPUI executor (`cx.spawn` family), so
/// this binary no longer builds an ad-hoc Tokio runtime just to initialize the
/// title services.
async fn build_rows() -> Result<Vec<TitleRow>, String> {
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
}

struct TitlePanel {
    rows: Arc<Vec<TitleRow>>,
    /// `build_rows()` failure, shown in the window instead of empty rows.
    error: Option<SharedString>,
}

impl TitlePanel {
    /// Apply the async build result on the UI thread. This is the single
    /// success/failure branch used by the spawned build task and by tests.
    fn apply_build_result(&mut self, result: Result<Vec<TitleRow>, String>) {
        match result {
            Ok(rows) => {
                self.rows = Arc::new(rows);
                self.error = None;
            }
            Err(error) => {
                let message = init_error_line(&error);
                eprintln!("cordis_title_ui: {message}");
                self.rows = Arc::new(Vec::new());
                self.error = Some(SharedString::from(message));
            }
        }
    }
}

/// Single source for the window-creation log line. The toggle demo uses the
/// identical helper so both binaries report window setup the same way.
fn window_log_line(opened: bool, detail: &str) -> String {
    if opened {
        "GPUI window created".to_string()
    } else {
        format!("failed to create window: {detail}")
    }
}

/// The one place this binary creates its window; the result is never discarded.
fn open_panel_window(panel: Entity<TitlePanel>, cx: &mut App) {
    match cx.open_window(Default::default(), |_window, _cx| panel.clone()) {
        Ok(_window) => eprintln!("cordis_title_ui: {}", window_log_line(true, "")),
        Err(error) => eprintln!(
            "cordis_title_ui: {}",
            window_log_line(false, &error.to_string())
        ),
    }
}

/// Initialize the title services on the GPUI executor and feed the outcome
/// back to the panel. Replaces the former temporary Tokio runtime in `main`.
fn start_build(panel: Entity<TitlePanel>, cx: &mut App) {
    cx.spawn(async move |app: &mut AsyncApp| {
        let result = build_rows().await;
        let _ = panel.update(app, |panel, _cx| panel.apply_build_result(result));
    })
    .detach();
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
        if let Some(error) = &self.error {
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xff6b6b))
                    .child(error.clone()),
            );
        }
        for row in self.rows.iter() {
            pane = pane.child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x24242c))
                    .child(SharedString::from(row_label(row))),
            );
        }
        pane
    }
}

fn main() {
    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| TitlePanel {
            rows: Arc::new(Vec::new()),
            error: None,
        });
        start_build(panel.clone(), cx);
        open_panel_window(panel, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_label_preserves_existing_session_source_title_hook() {
        let row = TitleRow {
            session_id: "s-llm".to_string(),
            title: "Rust Plugin Port".to_string(),
            source: "llm",
        };
        assert_eq!(row_label(&row), "s-llm [llm]: Rust Plugin Port");
    }

    #[test]
    fn init_error_is_shown_instead_of_empty_rows() {
        let message = init_error_line("boom");
        assert_eq!(message, "init error: boom");
        let panel = TitlePanel {
            rows: Arc::new(Vec::new()),
            error: Some(SharedString::from(message.clone())),
        };
        assert_eq!(
            panel.error.as_ref().map(|error| error.to_string()),
            Some(message)
        );
        assert!(panel.rows.is_empty());
    }

    #[test]
    fn window_log_line_is_shared_and_preserves_messages() {
        assert_eq!(window_log_line(true, ""), "GPUI window created");
        assert_eq!(
            window_log_line(false, "boom"),
            "failed to create window: boom"
        );
    }

    #[test]
    fn apply_build_result_keeps_rows_or_shows_error() {
        let mut panel = TitlePanel {
            rows: Arc::new(Vec::new()),
            error: None,
        };
        panel.apply_build_result(Ok(vec![TitleRow {
            session_id: "s-llm".to_string(),
            title: "Rust Plugin Port".to_string(),
            source: "llm",
        }]));
        assert_eq!(panel.rows.len(), 1);
        assert_eq!(row_label(&panel.rows[0]), "s-llm [llm]: Rust Plugin Port");
        assert!(panel.error.is_none());

        panel.apply_build_result(Err("boom".to_string()));
        assert!(panel.rows.is_empty());
        assert_eq!(
            panel.error.as_ref().map(|error| error.to_string()),
            Some(init_error_line("boom"))
        );
    }

    #[tokio::test]
    async fn build_rows_still_provides_both_title_rows() {
        let rows = build_rows().await.expect("title demo should build");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].session_id, "s-llm");
        assert_eq!(rows[0].source, "llm");
        assert!(!rows[0].title.is_empty());
        assert_eq!(rows[1].session_id, "s-fallback");
        assert_eq!(rows[1].source, "fallback");
        assert!(!rows[1].title.is_empty());
    }
}

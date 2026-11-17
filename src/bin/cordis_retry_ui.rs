//! GPUI demo: a structured LLM error is retried by the agent loop.
//!
//! A flaky provider returns `StreamChunk::Error("HTTP 429 ...")` on its first
//! call and succeeds on the second. The window shows the attempt count and
//! final text, proving the retry wiring end-to-end.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use cordis_rs::Context;
use cordis_rs::core_services::{AgentLoop, LlmRequest, RetryPolicy, provide_llm, provide_tools};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

#[derive(Clone)]
struct RetryReport {
    attempts: usize,
    final_text: String,
    error: Option<String>,
}

fn run_agent() -> Result<RetryReport, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        let ctx = Context::root("retry-ui");
        let (tools, _) = provide_tools(&ctx).await.map_err(|e| e.to_string())?;
        let llm = provide_llm(&ctx).await.map_err(|e| e.to_string())?;

        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = attempts.clone();
        let provider = cordis_rs::core_services::from_async_text(
            "flaky",
            "flaky-1",
            move |_request: LlmRequest| {
                let counter = counter.clone();
                async move {
                    if counter.fetch_add(1, AtomicOrdering::SeqCst) == 0 {
                        Err("HTTP 429 rate limited".to_string())
                    } else {
                        Ok::<_, String>("recovered after retry".to_string())
                    }
                }
            },
        );
        let _p = llm.register(provider);
        llm.set_default("flaky");

        let policy = RetryPolicy {
            max_retries: 2,
            initial_delay_ms: 1,
            max_delay_ms: 1,
            ..Default::default()
        };
        let loop_ = AgentLoop::new(llm, tools)
            .with_retry(policy)
            .with_call_parser(Arc::new(|_text: &str| Vec::new()));
        match loop_.run("go").await {
            Ok(run) => Ok(RetryReport {
                attempts: attempts.load(AtomicOrdering::SeqCst),
                final_text: run.final_text,
                error: None,
            }),
            Err(error) => Ok(RetryReport {
                attempts: attempts.load(AtomicOrdering::SeqCst),
                final_text: String::new(),
                error: Some(error.to_string()),
            }),
        }
    })
}

struct RetryPanel {
    report: RetryReport,
}

impl Render for RetryPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let detail = match &self.report.error {
            Some(error) => format!("ERROR: {error}"),
            None => format!(
                "attempts={} | final={}",
                self.report.attempts, self.report.final_text
            ),
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_3()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — LLM retry"))
            .child(
                div()
                    .text_sm()
                    .text_color(if self.report.error.is_none() {
                        rgb(0x9ce0b0)
                    } else {
                        rgb(0xff8c8c)
                    })
                    .child(SharedString::from(detail)),
            )
    }
}

fn main() {
    let report = run_agent().unwrap_or_else(|error| RetryReport {
        attempts: 0,
        final_text: String::new(),
        error: Some(error),
    });
    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| RetryPanel {
            report: report.clone(),
        });
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_retry_ui: GPUI window created");
    });
}

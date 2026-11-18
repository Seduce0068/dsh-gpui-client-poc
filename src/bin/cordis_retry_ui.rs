//! GPUI demo: a structured LLM error is retried by the agent loop.
//!
//! A flaky provider returns `StreamChunk::Error("HTTP 429 ...")` on its first
//! call and succeeds on the second. The window shows the attempt count and
//! final text, proving the retry wiring end-to-end.
//!
//! SCOPE: that 429 is a hard-coded string from the LOCAL STUB provider below,
//! inside the cordis-rs agent-loop layer. It is NOT ACP traffic: no
//! `dsh --profile acp` subprocess is spawned, no `session/prompt` is sent, and
//! the ACP client has no retry/backoff. The window states this in its status
//! so a local "recovered after retry" is never read as ACP/backend recovery
//! (`retry-ui-unrelated-to-acp`, G7).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use cordis_rs::Context;
use cordis_rs::core_services::{AgentLoop, LlmRequest, RetryPolicy, provide_llm, provide_tools};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

/// Hard-coded provider error injected by the local stub. Deliberately not an
/// ACP / JSON-RPC error: no backend is contacted by this demo (see module docs).
const STUB_ERROR: &str = "HTTP 429 rate limited";

/// Always-visible first status line. The retry result must never be mistaken
/// for an ACP or real model-provider recovery.
const SCOPE_NOTE: &str =
    "SCOPE: cordis agent-loop retry demo only (local stub) — NOT ACP, no dsh subprocess";

/// Failure classes from the review finding, applied to this demo's stub and
/// final errors. Recovery hints explicitly say "stub": they describe what a
/// real backend would need, not what this demo actually recovered from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoFailureKind {
    Balance,
    RateLimited,
    AuthExpired,
    Disconnected,
    Unknown,
}

impl DemoFailureKind {
    fn classify(message: &str) -> Self {
        let text = message.to_ascii_lowercase();
        const BALANCE: &[&str] = &[
            "insufficient balance",
            "insufficient credit",
            "insufficient funds",
            "insufficient_quota",
            "payment required",
            "billing",
            "402",
            "残高不足",
        ];
        const RATE_LIMIT: &[&str] = &["429", "rate limit", "too many requests", "throttl", "quota"];
        const AUTH: &[&str] = &[
            "auth",
            "unauthor",
            "forbidden",
            "credential",
            "token",
            "expired",
            "401",
            "403",
        ];
        const DISCONNECT: &[&str] = &[
            "disconnect",
            "connection",
            "broken pipe",
            "eof",
            "no stdin",
            "no stdout",
            "spawn",
            "transport",
            "timed out",
            "timeout",
            "reset by peer",
        ];
        // Check the specific causes before the broad ones so each class gets
        // its own recovery hint instead of collapsing into one string.
        if BALANCE.iter().any(|needle| text.contains(needle)) {
            Self::Balance
        } else if RATE_LIMIT.iter().any(|needle| text.contains(needle)) {
            Self::RateLimited
        } else if AUTH.iter().any(|needle| text.contains(needle)) {
            Self::AuthExpired
        } else if DISCONNECT.iter().any(|needle| text.contains(needle)) {
            Self::Disconnected
        } else {
            Self::Unknown
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Balance => "balance/402",
            Self::RateLimited => "rate-limit/429",
            Self::AuthExpired => "auth-expired",
            Self::Disconnected => "disconnected",
            Self::Unknown => "unknown",
        }
    }

    fn recovery(self) -> &'static str {
        match self {
            Self::Balance => {
                "stub demo: a real balance/402 error needs a top-up; no provider is contacted here"
            }
            Self::RateLimited => {
                "stub demo: a local counter injected 429 and cordis retried locally; ACP has no backoff"
            }
            Self::AuthExpired => {
                "stub demo: a real auth error needs refreshed credentials; none are used here"
            }
            Self::Disconnected => {
                "stub demo: a real transport failure needs reconnect; no ACP subprocess exists here"
            }
            Self::Unknown => "stub demo: unclassified error; no recovery is demonstrated",
        }
    }
}

/// Classify one message for display. Pure so the status text is testable.
fn classify_line(prefix: &str, message: &str) -> String {
    let kind = DemoFailureKind::classify(message);
    format!(
        "{prefix}{message} -> [{}] recovery: {}",
        kind.label(),
        kind.recovery()
    )
}

#[derive(Clone)]
struct RetryReport {
    attempts: usize,
    final_text: String,
    error: Option<String>,
    /// The first-call stub error, shown with its class even on success so the
    /// "recovered" result is tied to the injected 429 and not to ACP.
    injected_error: Option<String>,
}

impl RetryReport {
    /// Lines rendered in the window. The scope note is always first, so a
    /// successful local retry can never be read as an ACP/backend recovery.
    fn status_lines(&self) -> Vec<String> {
        let mut lines = vec![SCOPE_NOTE.to_string()];
        if let Some(stub) = &self.injected_error {
            lines.push(classify_line("injected: ", stub));
        }
        match &self.error {
            Some(error) => lines.push(classify_line("ERROR: ", error)),
            None => lines.push(format!(
                "attempts={} | final={}",
                self.attempts, self.final_text
            )),
        }
        lines
    }
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
                        // Local stub only — see SCOPE_NOTE / module docs.
                        Err(STUB_ERROR.to_string())
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
                injected_error: Some(STUB_ERROR.to_string()),
            }),
            Err(error) => Ok(RetryReport {
                attempts: attempts.load(AtomicOrdering::SeqCst),
                final_text: String::new(),
                error: Some(error.to_string()),
                injected_error: Some(STUB_ERROR.to_string()),
            }),
        }
    })
}

struct RetryPanel {
    report: RetryReport,
}

impl Render for RetryPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let mut pane = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_3()
            .text_color(rgb(0xe8e8ef))
            .child(
                div()
                    .text_xl()
                    .child("cordis-rs × GPUI — LLM retry (local stub)"),
            );
        for (index, line) in self.report.status_lines().into_iter().enumerate() {
            // The scope note stays amber: a green "recovered" line must not
            // colour the NOT-ACP disclaimer as success.
            let color = if index == 0 {
                rgb(0xffcf8c)
            } else if self.report.error.is_none() {
                rgb(0x9ce0b0)
            } else {
                rgb(0xff8c8c)
            };
            pane = pane.child(
                div()
                    .text_sm()
                    .text_color(color)
                    .child(SharedString::from(line)),
            );
        }
        pane
    }
}

fn main() {
    let report = run_agent().unwrap_or_else(|error| RetryReport {
        attempts: 0,
        final_text: String::new(),
        error: Some(error),
        // The stub was never reached, so there is no injected error to show.
        injected_error: None,
    });
    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| RetryPanel {
            report: report.clone(),
        });
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_retry_ui: GPUI window created (local stub retry demo, NOT ACP)");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn success_report() -> RetryReport {
        RetryReport {
            attempts: 2,
            final_text: "recovered after retry".to_string(),
            error: None,
            injected_error: Some(STUB_ERROR.to_string()),
        }
    }

    #[test]
    fn injected_429_is_classified_as_rate_limited() {
        assert_eq!(
            DemoFailureKind::classify(STUB_ERROR),
            DemoFailureKind::RateLimited
        );
    }

    #[test]
    fn balance_rate_limit_auth_and_disconnect_are_distinct() {
        let cases = [
            ("Insufficient Balance", DemoFailureKind::Balance),
            ("HTTP 429 rate limited", DemoFailureKind::RateLimited),
            (
                "AuthRequired: token expired (401)",
                DemoFailureKind::AuthExpired,
            ),
            (
                "transport disconnected: broken pipe",
                DemoFailureKind::Disconnected,
            ),
        ];
        let mut labels = std::collections::BTreeSet::new();
        for (message, expected) in cases {
            let kind = DemoFailureKind::classify(message);
            assert_eq!(kind, expected, "unexpected class for {message}");
            assert!(labels.insert(kind.label()), "labels must be distinct");
        }
    }

    #[test]
    fn status_always_states_the_demo_is_not_acp() {
        let lines = success_report().status_lines();
        assert_eq!(lines[0], SCOPE_NOTE);
        assert!(lines[0].contains("NOT ACP"));
        assert!(lines[0].contains("no dsh subprocess"));

        let failed = RetryReport {
            attempts: 1,
            final_text: String::new(),
            error: Some("boom".to_string()),
            injected_error: None,
        };
        assert_eq!(failed.status_lines()[0], SCOPE_NOTE);
    }

    #[test]
    fn status_explains_the_injected_429_and_stub_only_recovery() {
        let joined = success_report().status_lines().join("\n");
        assert!(
            joined.contains("injected: HTTP 429 rate limited"),
            "{joined}"
        );
        assert!(joined.contains("[rate-limit/429]"), "{joined}");
        assert!(joined.contains("local counter injected 429"), "{joined}");
        assert!(joined.contains("ACP has no backoff"), "{joined}");
        assert!(
            joined.contains("attempts=2 | final=recovered after retry"),
            "{joined}"
        );
    }

    #[test]
    fn final_error_is_classified_instead_of_shown_generically() {
        let report = RetryReport {
            attempts: 1,
            final_text: String::new(),
            error: Some("Insufficient Balance".to_string()),
            injected_error: Some(STUB_ERROR.to_string()),
        };
        let joined = report.status_lines().join("\n");
        assert!(
            joined.contains("ERROR: Insufficient Balance -> [balance/402]"),
            "{joined}"
        );
        assert!(joined.contains("needs a top-up"), "{joined}");
    }

    #[test]
    fn run_agent_still_recovers_from_the_local_stub() {
        let report = run_agent().expect("demo agent should complete");
        assert_eq!(report.attempts, 2, "one injected failure then recovery");
        assert_eq!(report.final_text, "recovered after retry");
        assert!(report.error.is_none());
        assert_eq!(report.injected_error.as_deref(), Some(STUB_ERROR));
    }
}

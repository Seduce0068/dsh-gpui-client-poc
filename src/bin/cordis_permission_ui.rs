//! GPUI demo: switch permission presets and run a tool under each.
//!
//! Mirrors the CLI permission_flow_demo with a clickable UI: three preset
//! buttons, the current approval/sandbox state, and the result of running a
//! `bash` tool through the approval-gated pipeline.
//!
//! The approval answerer is no longer hard-wired to `Allow`: set
//! `DSH_GPUI_APPROVAL_ANSWER` to `allow`, `deny`, or `ask` before launch.
//! `ask` makes the answerer defer (no decision), so the request falls through
//! and fails closed — an interactive dialog is impossible with the current
//! synchronous `Answerer` API, so deferral is the honest headless behaviour.
//!
//! The tool call arguments are captured into the answerer (the external
//! `ApprovalRequest` carries only `action`/`description`) and are shown in the
//! UI together with what the answerer saw and decided. This is a host-side
//! fix only; cordis-rs is used as-is.

use std::env;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};

use cordis_rs::Context;
use cordis_rs::core_services::{
    Answerer, ApprovalDecision, ApprovalRequest, ToolDefinition, ToolError, ToolExecContext,
    ToolOutput, provide_approval, provide_permission_presets, provide_tools,
};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

const PRESETS: [&str; 3] = ["read-only", "workspace-write", "danger-full-access"];
const ANSWER_ENV: &str = "DSH_GPUI_APPROVAL_ANSWER";

/// Policy selected by the operator for the demo answerer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnswerMode {
    Allow,
    Deny,
    Ask,
}

impl AnswerMode {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "allow" => Some(Self::Allow),
            "deny" => Some(Self::Deny),
            "ask" => Some(Self::Ask),
            _ => None,
        }
    }

    /// Read the raw OS value so a non-UTF-8 byte sequence is never mistaken
    /// for "unset". Only a genuinely absent variable keeps the historical
    /// always-Allow behaviour (swarm review round 283, R2-CODE-PERM).
    fn from_env() -> Self {
        Self::from_env_from_raw(env::var_os(ANSWER_ENV), ANSWER_ENV)
    }

    /// Pure entry point for a raw OS value. Keep the `Option<OsString>` →
    /// `VarError::NotUnicode` conversion here so the whole production path is
    /// testable from the value side without touching the process environment
    /// (R5 review, R2-CODE-PERM).
    fn from_env_from_raw(value: Option<OsString>, name: &str) -> Self {
        let raw = match value {
            None => Err(env::VarError::NotPresent),
            Some(value) => value.into_string().map_err(env::VarError::NotUnicode),
        };
        Self::from_env_result(raw, name)
    }

    /// Pure boundary helper: every decision is derived from `value` alone.
    /// `None` (unset) keeps Allow; any present value that does not parse —
    /// including non-UTF-8 bytes after `to_string_lossy` — warns on stderr
    /// and fails closed to Ask.
    fn from_env_os(value: Option<OsString>, name: &str) -> Self {
        match value {
            None => Self::Allow,
            Some(value) => {
                let raw = value.to_string_lossy();
                Self::parse(&raw).unwrap_or_else(|| {
                    eprintln!("invalid {name}={raw:?}; falling back to ask");
                    Self::Ask
                })
            }
        }
    }

    /// Adapter for the old `env::var` shaped result, retained so existing
    /// callers keep working. `NotUnicode` now takes the fail-closed path
    /// instead of the old blanket `Err(_) => Allow`.
    fn from_env_result(raw: std::result::Result<String, env::VarError>, name: &str) -> Self {
        match raw {
            Ok(value) => Self::from_env_os(Some(OsString::from(value)), name),
            Err(env::VarError::NotPresent) => Self::from_env_os(None, name),
            Err(env::VarError::NotUnicode(value)) => Self::from_env_os(Some(value), name),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Ask => "ask",
        }
    }

    /// `Ask` returns no decision: the waterfall keeps looking and, with no
    /// other answerer, the request fails closed. This never touches the
    /// `ApprovalService` (whose read guard is held during the callback), so
    /// there is no lock reentrancy.
    fn decision(self) -> Option<ApprovalDecision> {
        match self {
            Self::Allow => Some(ApprovalDecision::Allow),
            Self::Deny => Some(ApprovalDecision::Deny),
            Self::Ask => None,
        }
    }
}

/// Render untrusted text on a single UI line. Control characters (including
/// newlines) become spaces so a tool description cannot forge extra rows;
/// normal text is unchanged (swarm review R21-CTL02).
fn display_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

/// What the answerer observed and returned, surfaced in the UI.
#[derive(Debug, Clone, Default, PartialEq)]
struct ApprovalTrace {
    action: String,
    description: String,
    tool_args: String,
    decision: Option<ApprovalDecision>,
}

/// Build a synchronous answerer that records the request and the tool
/// arguments into `trace` before returning the policy decision.
fn build_answerer(
    mode: AnswerMode,
    tool_args: serde_json::Value,
    trace: Arc<Mutex<ApprovalTrace>>,
) -> Answerer {
    let args_text = tool_args.to_string();
    Arc::new(move |request: &ApprovalRequest| {
        let decision = mode.decision();
        *trace.lock().expect("approval trace poisoned") = ApprovalTrace {
            action: request.action.clone(),
            description: request.description.clone(),
            tool_args: args_text.clone(),
            decision,
        };
        decision
    })
}

/// Which layer failed during a preset switch, kept for UI display so a demo
/// operator can tell a runtime/service/preset failure apart (swarm review
/// round 283). The original error message is preserved as a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwitchErrorKind {
    Runtime,
    Service,
    Preset,
}

impl SwitchErrorKind {
    fn label(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Service => "service",
            Self::Preset => "preset",
        }
    }
}

type SwitchError = (SwitchErrorKind, String);

/// Tag an error with the layer that produced it. Centralising the mapping
/// keeps the UI label honest when errors are added (swarm review R17-DUAL2-04).
fn classify_error(kind: SwitchErrorKind, error: impl std::fmt::Display) -> SwitchError {
    (kind, error.to_string())
}

struct PermissionState {
    preset: String,
    approval: String,
    sandbox: String,
    result: String,
    call_args: String,
    trace: ApprovalTrace,
}

struct PermissionPanel {
    state: Arc<Mutex<PermissionState>>,
    answer_mode: AnswerMode,
}

impl PermissionPanel {
    fn switch(&mut self, preset: &str) -> Result<(), SwitchError> {
        // Recreate the runtime pieces per switch so the policy is fresh.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| classify_error(SwitchErrorKind::Runtime, error))?;
        runtime.block_on(async {
            let ctx = Context::root("permission-ui");
            let (registry, pipeline) = provide_tools(&ctx)
                .await
                .map_err(|e| classify_error(SwitchErrorKind::Service, e))?;
            let approval = provide_approval(&ctx)
                .await
                .map_err(|e| classify_error(SwitchErrorKind::Service, e))?;
            let presets = provide_permission_presets(&ctx)
                .await
                .map_err(|e| classify_error(SwitchErrorKind::Service, e))?;
            presets.bind_approval(approval.clone());
            pipeline.with_approval(approval.clone());

            let tool_args = serde_json::json!({ "command": "printf 'permission-ui'" });
            let args_text = tool_args.to_string();
            let trace = Arc::new(Mutex::new(ApprovalTrace::default()));
            let _answerer = approval.add_answerer(
                "policy",
                build_answerer(self.answer_mode, tool_args.clone(), trace.clone()),
            );
            let _tool = registry.register(ToolDefinition::new(
                "bash",
                "Run a command",
                Arc::new(|args, _exec| {
                    let echoed = args.to_string();
                    Box::pin(async move { Ok(ToolOutput::text(echoed)) })
                        as cordis_rs::events::BoxFuture<'static, Result<ToolOutput, ToolError>>
                }),
            ));

            let current = presets
                .switch(preset)
                .map_err(|e| classify_error(SwitchErrorKind::Preset, e))?;
            let result = pipeline
                .run(&registry, "bash", tool_args, ToolExecContext::agent("ui"))
                .await;

            let observed = trace.lock().expect("approval trace poisoned").clone();
            let mut guard = self.state.lock().expect("permission state poisoned");
            guard.preset = preset.to_string();
            guard.approval = current.approval.as_str().to_string();
            guard.sandbox = current.sandbox.as_str().to_string();
            guard.result = match result {
                Ok(output) => format!("ALLOWED: {}", output.rendered.join("")),
                Err(error) => format!("DENIED: {error}"),
            };
            guard.call_args = args_text;
            guard.trace = observed;
            Ok(())
        })
    }
}

impl Render for PermissionPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let (preset, approval, sandbox, result, call_args, trace) = {
            let state = self.state.lock().expect("permission state poisoned");
            (
                state.preset.clone(),
                state.approval.clone(),
                state.sandbox.clone(),
                state.result.clone(),
                state.call_args.clone(),
                state.trace.clone(),
            )
        };
        let decision = trace
            .decision
            .map(|value| value.as_str())
            .unwrap_or("none (deferred)");
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
                    .child("cordis-rs × GPUI — permission presets"),
            )
            .child(div().text_sm().child(SharedString::from(format!(
                "preset: {preset} | approval: {approval} | sandbox: {sandbox}"
            ))))
            .child(div().text_sm().child(SharedString::from(format!(
                "answer policy: {} ({ANSWER_ENV}=allow|deny|ask)",
                self.answer_mode.as_str()
            ))))
            .child(div().text_sm().child(SharedString::from(format!(
                "tool call args: {}",
                display_text(&call_args)
            ))))
            .child(div().text_sm().child(SharedString::from(format!(
                "answerer saw: {} — {} | args: {}",
                display_text(&trace.action),
                display_text(&trace.description),
                display_text(&trace.tool_args)
            ))))
            .child(div().text_sm().child(SharedString::from(format!(
                "answerer decision: {}",
                display_text(decision)
            ))))
            .child(
                div()
                    .text_sm()
                    .text_color(if result.starts_with("ALLOWED") {
                        rgb(0x9ce0b0)
                    } else {
                        rgb(0xff8c8c)
                    })
                    .child(SharedString::from(result)),
            );
        for (index, preset) in PRESETS.iter().enumerate() {
            let name = preset.to_string();
            pane = pane.child(
                div()
                    .id(("preset", index))
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x2b3a4a))
                    .text_color(rgb(0xe8e8ef))
                    .child(SharedString::from(format!("switch to {name}")))
                    .on_click(cx.listener(move |panel, _event, _window, _cx| {
                        if let Err((kind, error)) = panel.switch(&name) {
                            let mut state = panel.state.lock().expect("permission state poisoned");
                            state.result = format!("ERROR[{}]: {error}", kind.label());
                        }
                    })),
            );
        }
        pane
    }
}

fn main() {
    let state = Arc::new(Mutex::new(PermissionState {
        preset: "(none)".to_string(),
        approval: "never".to_string(),
        sandbox: "workspace-write".to_string(),
        result: "choose a preset, then the tool runs".to_string(),
        call_args: "(none)".to_string(),
        trace: ApprovalTrace::default(),
    }));
    let answer_mode = AnswerMode::from_env();
    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| PermissionPanel {
            state: state.clone(),
            answer_mode,
        });
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_permission_ui: GPUI window created");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_text_flattens_control_characters() {
        // A model-supplied description cannot forge extra UI rows
        // (swarm review R21-CTL02).
        assert_eq!(super::display_text("a\nb"), "a b");
        assert_eq!(super::display_text("a\t\u{1b}b"), "a  b");
        assert_eq!(super::display_text("plain text"), "plain text");
    }

    fn sample_request() -> ApprovalRequest {
        ApprovalRequest::new("tool:bash", "Execute tool bash with the supplied arguments")
    }

    /// A present but non-UTF-8 OS value: the exact case `env::var` reports as
    /// `VarError::NotUnicode` and the old blanket `Err(_) => Allow` swallowed.
    #[cfg(unix)]
    fn non_utf8_answer() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(b"alow\xff".to_vec())
    }

    #[cfg(windows)]
    fn non_utf8_answer() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[
            u16::from(b'a'),
            u16::from(b'l'),
            u16::from(b'o'),
            u16::from(b'w'),
            0xD800,
        ])
    }

    #[cfg(not(any(unix, windows)))]
    fn non_utf8_answer() -> OsString {
        // No portable non-UTF-8 constructor here; a typo still exercises the
        // fail-closed assertion on other targets.
        OsString::from("alow")
    }

    #[test]
    fn malformed_answer_env_fails_closed_to_ask() {
        use std::env::VarError;

        // (a) Unset keeps the historical Allow contract.
        assert_eq!(
            AnswerMode::from_env_result(Err(VarError::NotPresent), ANSWER_ENV),
            AnswerMode::Allow
        );
        assert_eq!(
            AnswerMode::from_env_result(Ok("ALLOW".to_string()), ANSWER_ENV),
            AnswerMode::Allow
        );
        assert_eq!(
            AnswerMode::from_env_result(Ok("deny".to_string()), ANSWER_ENV),
            AnswerMode::Deny
        );
        assert_eq!(
            AnswerMode::from_env_result(Ok("ask".to_string()), ANSWER_ENV),
            AnswerMode::Ask
        );
        // (b) Typo and empty value keep the pre-existing fail-closed contract.
        assert_eq!(
            AnswerMode::from_env_result(Ok("alow".to_string()), ANSWER_ENV),
            AnswerMode::Ask
        );
        assert_eq!(
            AnswerMode::from_env_result(Ok("".to_string()), ANSWER_ENV),
            AnswerMode::Ask
        );
        // Non-UTF-8 bytes were swallowed by `Err(_) => Allow`; now fail closed.
        assert_eq!(
            AnswerMode::from_env_result(Err(VarError::NotUnicode(non_utf8_answer())), ANSWER_ENV),
            AnswerMode::Ask
        );
    }

    #[test]
    fn env_value_boundaries_are_decided_by_the_pure_helper() {
        // Same entry point as `from_env`, driven from the value side so the
        // `into_string` -> `NotUnicode` conversion is covered by the test.
        // Unset is the only absence-driven Allow case.
        assert_eq!(
            AnswerMode::from_env_from_raw(None, ANSWER_ENV),
            AnswerMode::Allow
        );
        for (value, expected) in [
            (OsString::from("allow"), AnswerMode::Allow),
            (OsString::from("deny"), AnswerMode::Deny),
            (OsString::from("ask"), AnswerMode::Ask),
            (OsString::from("alow"), AnswerMode::Ask),
            (OsString::from(""), AnswerMode::Ask),
            (non_utf8_answer(), AnswerMode::Ask),
        ] {
            assert_eq!(
                AnswerMode::from_env_from_raw(Some(value.clone()), ANSWER_ENV),
                expected,
                "value {value:?}"
            );
        }
    }

    #[test]
    fn switch_error_classification_keeps_layers_distinct() {
        let preset = super::classify_error(super::SwitchErrorKind::Preset, "unknown preset");
        assert_eq!(preset.0, super::SwitchErrorKind::Preset);
        assert_eq!(preset.1, "unknown preset");
        let service = super::classify_error(super::SwitchErrorKind::Service, "provider missing");
        assert_eq!(service.0, super::SwitchErrorKind::Service);
        assert_ne!(preset.0, service.0);
    }

    #[test]
    fn switch_error_kinds_have_distinct_labels() {
        assert_eq!(super::SwitchErrorKind::Runtime.label(), "runtime");
        assert_eq!(super::SwitchErrorKind::Service.label(), "service");
        assert_eq!(super::SwitchErrorKind::Preset.label(), "preset");
    }

    #[test]
    fn parses_answer_mode_values() {
        assert_eq!(AnswerMode::parse("allow"), Some(AnswerMode::Allow));
        assert_eq!(AnswerMode::parse(" DENY "), Some(AnswerMode::Deny));
        assert_eq!(AnswerMode::parse("Ask"), Some(AnswerMode::Ask));
        assert_eq!(AnswerMode::parse("maybe"), None);
        assert_eq!(AnswerMode::parse(""), None);
        assert_eq!(AnswerMode::Allow.as_str(), "allow");
        assert_eq!(AnswerMode::Deny.as_str(), "deny");
        assert_eq!(AnswerMode::Ask.as_str(), "ask");
    }

    #[test]
    fn modes_map_to_expected_decisions() {
        assert_eq!(AnswerMode::Allow.decision(), Some(ApprovalDecision::Allow));
        assert_eq!(AnswerMode::Deny.decision(), Some(ApprovalDecision::Deny));
        assert_eq!(AnswerMode::Ask.decision(), None);
    }

    #[test]
    fn answerer_forwards_request_and_tool_args_to_trace() {
        for (mode, expected) in [
            (AnswerMode::Allow, Some(ApprovalDecision::Allow)),
            (AnswerMode::Deny, Some(ApprovalDecision::Deny)),
            (AnswerMode::Ask, None),
        ] {
            let trace = Arc::new(Mutex::new(ApprovalTrace::default()));
            let args = serde_json::json!({ "command": "printf hi" });
            let answerer = build_answerer(mode, args.clone(), trace.clone());

            assert_eq!(answerer(&sample_request()), expected);

            let seen = trace.lock().expect("trace poisoned");
            assert_eq!(seen.action, "tool:bash");
            assert_eq!(
                seen.description,
                "Execute tool bash with the supplied arguments"
            );
            assert_eq!(seen.tool_args, args.to_string());
            assert_eq!(seen.decision, expected);
        }
    }
}

//! GPUI view of files read through the cordis-rs `ctx.fs` / tool-fs port.
//!
//! Mounts `dsh-tool-fs` on a Cordis context and calls the `read` tool through
//! the shared `tools` seam. The rendered result is shown in a GPUI window.
//! Read failures (missing path, directory, permission denied, invalid UTF-8)
//! are carried into the view and rendered as an error instead of aborting
//! before the window opens.

use cordis_rs::Context;
use cordis_rs::core_services::{ToolExecContext, ToolOutput, provide_fs, provide_tools};
use cordis_rs::plugin_ports::{ToolFsConfig, mount_tool_fs};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

/// Environment variable holding the file to display when no CLI argument is
/// given. Keeps the binary free of any personal absolute path default.
const PATH_ENV: &str = "CORDIS_FS_UI_PATH";
/// Portable fallback: a cwd-relative path, never a machine-specific absolute
/// path.
const CWD_RELATIVE_DEFAULT: &str = "README.md";

/// Where the selected path came from. Shown in the UI so a failed read can be
/// traced back to its source instead of hinting at a hidden hard-coded default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathOrigin {
    Cli,
    Env,
    Cwd,
}

impl PathOrigin {
    fn label(self) -> &'static str {
        match self {
            Self::Cli => "CLI argument",
            Self::Env => "environment variable CORDIS_FS_UI_PATH",
            Self::Cwd => "current directory",
        }
    }
}

/// Resolve the file to display, in priority order:
///   1. the first CLI argument,
///   2. the `CORDIS_FS_UI_PATH` environment variable,
///   3. `README.md` relative to the current working directory.
///
/// Every source is resolved against `cwd`; relative paths are joined onto it
/// and absolute paths are kept as-is. No machine-specific absolute path is
/// baked into the binary. Blank values are ignored so an explicitly empty
/// argument or environment variable cannot silently select a wrong file.
///
/// Returns `Err` with a human-readable reason when no usable source exists or
/// the current directory is unavailable. The caller renders that reason in the
/// UI (fail-closed) rather than guessing a path.
///
/// Kept pure (cwd and sources are injected) so the selection contract is
/// regression-testable without opening GPUI or touching process globals.
fn resolve_path(
    args: &[String],
    env_path: Option<&str>,
    cwd: Option<&std::path::Path>,
) -> Result<(std::path::PathBuf, PathOrigin), String> {
    let cwd = cwd.ok_or_else(|| "cannot determine current working directory".to_owned())?;

    let usable = |raw: &str, origin: PathOrigin| {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| (trimmed.to_owned(), origin))
    };

    let (raw, origin) = args
        .first()
        .and_then(|raw| usable(raw, PathOrigin::Cli))
        .or_else(|| env_path.and_then(|raw| usable(raw, PathOrigin::Env)))
        .or_else(|| usable(CWD_RELATIVE_DEFAULT, PathOrigin::Cwd))
        .ok_or_else(|| "no file path from CLI, environment, or current directory".to_owned())?;

    let candidate = std::path::Path::new(&raw);
    let resolved = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        cwd.join(candidate)
    };
    Ok((resolved, origin))
}

#[derive(Clone)]
struct FilePanel {
    path: SharedString,
    content: SharedString,
    error: Option<SharedString>,
}

impl FilePanel {
    fn new(path: String, content: String, error: Option<String>) -> Self {
        Self {
            path: SharedString::from(path),
            content: SharedString::from(content),
            error: error.map(SharedString::from),
        }
    }

    /// Turn a read outcome into view state. Failures stay in the model so
    /// `render` is the single place that decides how they are presented.
    fn from_result(path: String, outcome: Result<String, String>) -> Self {
        match outcome {
            Ok(content) => Self::new(path, content, None),
            Err(error) => Self::new(path, String::new(), Some(error)),
        }
    }
}

impl Render for FilePanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let body = match &self.error {
            Some(error) => div()
                .text_color(rgb(0xff8787))
                .child("read error: ")
                .child(error.clone()),
            None => div().child(self.content.clone()),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x141418))
            .p_4()
            .gap_3()
            .text_color(rgb(0xe8e8ef))
            .child(div().text_xl().child("cordis-rs × GPUI — tool-fs read"))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x8ec5ff))
                    .child(self.path.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x24242c))
                    .child(body),
            )
    }
}

/// Read a file through the cordis tools seam.
///
/// Every failure is returned as `Err(String)` rather than propagated out of
/// `main`; the caller hands it to `FilePanel::from_result` so the window still
/// opens and the reason is visible in the UI.
async fn read_file(path: String) -> Result<String, String> {
    let ctx = Context::root("gpui-fs-ui");
    let (registry, _) = provide_tools(&ctx).await.map_err(|e| e.to_string())?;
    let fs = provide_fs(&ctx).await.map_err(|e| e.to_string())?;
    let _tools = mount_tool_fs(&ctx, fs, ToolFsConfig::default())
        .await
        .map_err(|e| e.to_string())?;

    let output: ToolOutput = registry
        .invoke(
            "read",
            serde_json::json!({ "file_path": path }),
            ToolExecContext::agent("gpui"),
        )
        .await
        .map_err(|error| format!("read failed: {error}"))?;
    Ok(output.rendered.join("\n"))
}

/// Build the view model from the resolved path. Both resolution failures and
/// read failures are carried into `panel.error` so the window still opens and
/// the reason is visible in the UI (fail-closed: no guessed fallback path).
async fn prepare_panel(
    args: &[String],
    env_path: Option<&str>,
    cwd: Option<&std::path::Path>,
) -> FilePanel {
    match resolve_path(args, env_path, cwd) {
        Ok((path, origin)) => {
            let display = format!("{} (from {})", path.display(), origin.label());
            let outcome = read_file(path.to_string_lossy().to_string()).await;
            FilePanel::from_result(display, outcome)
        }
        Err(reason) => FilePanel::from_result(
            "(path unresolved)".to_owned(),
            Err(format!("path resolution failed: {reason}")),
        ),
    }
}

/// Single source for the window-creation log line. The checked `open_window`
/// result feeds this helper, so success and failure wording stays identical
/// across the GPUI demo binaries.
fn window_log_line(opened: bool, detail: &str) -> String {
    if opened {
        "GPUI window created".to_string()
    } else {
        format!("failed to create window: {detail}")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env_path = std::env::var(PATH_ENV).ok();
    let cwd = std::env::current_dir().ok();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let panel = runtime.block_on(prepare_panel(&args, env_path.as_deref(), cwd.as_deref()));

    eprintln!(
        "cordis_fs_ui: prepared view for {} (error: {})",
        panel.path,
        panel.error.is_some()
    );

    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| panel.clone());
        // Check the result instead of discarding it: a failed window must be
        // reported on stderr with the same binary-name prefix as success.
        match cx.open_window(Default::default(), |_window, _cx| panel.clone()) {
            Ok(_window) => eprintln!("cordis_fs_ui: {}", window_log_line(true, "")),
            Err(error) => eprintln!(
                "cordis_fs_ui: {}",
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
    fn window_log_line_reports_success_without_detail() {
        assert_eq!(window_log_line(true, ""), "GPUI window created");
        assert_eq!(
            window_log_line(true, "unused detail"),
            "GPUI window created"
        );
    }

    #[test]
    fn window_log_line_reports_failure_with_detail() {
        assert_eq!(
            window_log_line(false, "no display"),
            "failed to create window: no display"
        );
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cordis-fs-ui-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn assert_error_panel(path: &str, outcome: Result<String, String>) -> FilePanel {
        assert!(outcome.is_err(), "read unexpectedly succeeded for {path}");
        let panel = FilePanel::from_result(path.to_string(), outcome);
        assert_eq!(panel.path.to_string(), path);
        assert!(panel.content.to_string().is_empty());
        let error = panel
            .error
            .as_ref()
            .expect("read error must be carried into the panel");
        assert!(
            error.contains("read failed: tool execution failed:"),
            "unexpected error text: {error}"
        );
        panel
    }

    #[test]
    fn no_personal_absolute_path_is_baked_in() {
        let cwd = std::path::Path::new("/tmp/workspace");
        let (path, origin) = resolve_path(&[], None, Some(cwd)).expect("cwd fallback must resolve");
        assert_eq!(path, cwd.join(CWD_RELATIVE_DEFAULT));
        assert_eq!(origin, PathOrigin::Cwd);
        let path = path.to_string_lossy();
        assert!(
            !path.contains("/home/koppe"),
            "resolved path must not fall back to a personal absolute path: {path}"
        );
    }

    #[test]
    fn cli_argument_wins_over_env_and_cwd() {
        let cwd = std::path::Path::new("/tmp/workspace");
        let (path, origin) = resolve_path(
            &sample_args(&[" cli-note.txt ", "ignored.txt"]),
            Some(" env-note.txt"),
            Some(cwd),
        )
        .expect("CLI path must resolve");
        assert_eq!(path, cwd.join("cli-note.txt"));
        assert_eq!(origin, PathOrigin::Cli);
    }

    #[test]
    fn env_is_used_when_cli_is_blank() {
        let cwd = std::path::Path::new("/tmp/workspace");
        let (path, origin) =
            resolve_path(&sample_args(&["   "]), Some(" env-note.txt "), Some(cwd))
                .expect("env path must resolve");
        assert_eq!(path, cwd.join("env-note.txt"));
        assert_eq!(origin, PathOrigin::Env);
    }

    #[cfg(unix)]
    #[test]
    fn absolute_path_from_cli_is_kept() {
        let (path, origin) = resolve_path(
            &sample_args(&["/tmp/absolute-note.txt"]),
            None,
            Some(std::path::Path::new("/tmp/workspace")),
        )
        .expect("absolute CLI path must resolve");
        assert_eq!(path, std::path::Path::new("/tmp/absolute-note.txt"));
        assert_eq!(origin, PathOrigin::Cli);
    }

    #[test]
    fn missing_cwd_fails_closed_with_a_reason() {
        let error = resolve_path(&sample_args(&["note.txt"]), None, None)
            .expect_err("resolution must fail without a cwd");
        assert!(error.contains("current working directory"), "{error}");
    }

    #[tokio::test]
    async fn resolution_failure_is_shown_as_a_panel_error() {
        let panel = prepare_panel(&sample_args(&["note.txt"]), None, None).await;
        assert!(panel.path.to_string().contains("unresolved"));
        assert!(panel.content.to_string().is_empty());
        let error = panel
            .error
            .as_ref()
            .expect("resolution failure must be carried into the panel");
        assert!(
            error.contains("path resolution failed") && error.contains("current working directory"),
            "unexpected error text: {error}"
        );
    }

    #[tokio::test]
    async fn relative_path_resolves_against_cwd_and_origin_is_shown() {
        let dir = temp_dir("relative");
        std::fs::write(dir.join("note.txt"), "relative content").unwrap();

        // `cwd` is injected, so resolution reads the injected directory while
        // `read_file` receives the resulting absolute path — no process-global
        // `set_current_dir` (unsafe in edition 2024) is needed.
        let panel = prepare_panel(
            &sample_args(&["note.txt"]),
            None,
            Some(std::path::Path::new(&dir)),
        )
        .await;

        assert!(panel.error.is_none(), "unexpected error: {:?}", panel.error);
        assert!(panel.path.to_string().contains("from CLI argument"));
        assert!(panel.content.to_string().contains("relative content"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn tool_fs_read_feeds_the_view_content() {
        let dir = temp_dir("ok");
        let path = dir.join("view.txt");
        let path = path.to_string_lossy().to_string();
        std::fs::write(&path, "line one\nline two\n").unwrap();

        let outcome = read_file(path.clone()).await;
        assert!(outcome.is_ok(), "read failed: {:?}", outcome.as_ref().err());
        let panel = FilePanel::from_result(path.clone(), outcome);
        assert!(panel.error.is_none());
        assert_eq!(panel.path.to_string(), path);
        assert!(panel.content.to_string().contains("line one"));
        assert!(panel.content.to_string().contains("line two"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn missing_path_is_shown_as_a_panel_error() {
        let dir = temp_dir("missing");
        let path = dir.join("absent.txt").to_string_lossy().to_string();

        let panel = assert_error_panel(&path, read_file(path.clone()).await);
        assert!(panel.error.is_some());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn directory_path_is_shown_as_a_panel_error() {
        let dir = temp_dir("directory");
        let path = dir.to_string_lossy().to_string();

        let panel = assert_error_panel(&path, read_file(path.clone()).await);
        assert!(panel.error.is_some());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unreadable_file_is_shown_as_a_panel_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("permission");
        let path = dir.join("locked.txt");
        std::fs::write(&path, "secret").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        // A privileged process can still read a mode-000 file; skip the
        // assertion in that environment rather than reporting a false failure.
        if std::fs::read_to_string(&path).is_ok() {
            eprintln!("skip: process can read mode-000 file");
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }

        let path = path.to_string_lossy().to_string();
        let panel = assert_error_panel(&path, read_file(path.clone()).await);
        assert!(panel.error.is_some());

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn invalid_utf8_is_shown_as_a_panel_error() {
        let dir = temp_dir("utf8");
        let path = dir.join("binary.txt");
        std::fs::write(&path, [0xff, 0xfe, 0x00, 0x80]).unwrap();

        let path = path.to_string_lossy().to_string();
        let panel = assert_error_panel(&path, read_file(path.clone()).await);
        assert!(
            panel
                .error
                .as_ref()
                .is_some_and(|error| error.contains("UTF-8")),
            "invalid UTF-8 reason must reach the panel"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }
}

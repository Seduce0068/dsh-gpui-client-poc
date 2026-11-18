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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        "/home/koppe/document/deepseek-harness/default-workspace/work/cordis-rs/README.md"
            .to_string()
    });

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let panel = runtime.block_on(async move {
        let outcome = read_file(path.clone()).await;
        FilePanel::from_result(path, outcome)
    });

    eprintln!(
        "cordis_fs_ui: prepared view for {} (error: {})",
        panel.path,
        panel.error.is_some()
    );

    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| panel.clone());
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_fs_ui: GPUI window created");
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cordis-fs-ui-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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

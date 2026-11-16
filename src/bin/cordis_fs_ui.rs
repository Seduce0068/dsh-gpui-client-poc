//! GPUI view of files read through the cordis-rs `ctx.fs` / tool-fs port.
//!
//! Mounts `dsh-tool-fs` on a Cordis context and calls the `read` tool through
//! the shared `tools` seam. The rendered result is shown in a GPUI window.

use cordis_rs::Context;
use cordis_rs::core_services::{ToolExecContext, ToolOutput, provide_fs, provide_tools};
use cordis_rs::plugin_ports::{ToolFsConfig, mount_tool_fs};
use gpui::{
    App, Application, Context as GpuiContext, Render, SharedString, Window, div, prelude::*, rgb,
};

struct FilePanel {
    path: SharedString,
    content: SharedString,
}

impl FilePanel {
    fn new(path: String, content: String) -> Self {
        Self {
            path: SharedString::from(path),
            content: SharedString::from(content),
        }
    }
}

impl Render for FilePanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut GpuiContext<Self>) -> impl IntoElement {
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
                    .child(self.content.clone()),
            )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        "/home/koppe/document/deepseek-harness/default-workspace/work/cordis-rs/README.md"
            .to_string()
    });

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (content, used_path) = runtime.block_on(async {
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
        Ok::<_, String>((output.rendered.join("\n"), path))
    })?;

    eprintln!(
        "cordis_fs_ui: read {} bytes from {used_path}",
        content.len()
    );

    Application::new().run(move |cx: &mut App| {
        let panel = cx.new(|_cx| FilePanel::new(used_path.clone(), content.clone()));
        let _ = cx.open_window(Default::default(), |_window, _cx| panel.clone());
        eprintln!("cordis_fs_ui: GPUI window created");
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tool_fs_read_feeds_the_view_content() {
        let dir = std::env::temp_dir().join(format!("cordis-fs-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("view.txt");
        let path = path.to_string_lossy().to_string();
        std::fs::write(&path, "line one\nline two\n").unwrap();

        let ctx = Context::root("gpui-fs-test");
        let (registry, _) = provide_tools(&ctx).await.unwrap();
        let fs = provide_fs(&ctx).await.unwrap();
        let _tools = mount_tool_fs(&ctx, fs, ToolFsConfig::default())
            .await
            .unwrap();

        let output: ToolOutput = registry
            .invoke(
                "read",
                serde_json::json!({ "file_path": path }),
                ToolExecContext::agent("gpui"),
            )
            .await
            .unwrap();
        let rendered = output.rendered.join("\n");
        assert!(rendered.contains("line one"));
        assert!(rendered.contains("line two"));

        let panel = FilePanel::new(path.clone(), rendered.clone());
        assert_eq!(panel.path.to_string(), path);
        assert!(panel.content.to_string().contains("line one"));

        std::fs::remove_dir_all(dir).unwrap();
    }
}

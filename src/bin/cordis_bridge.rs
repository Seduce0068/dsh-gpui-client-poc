//! Headless bridge demo: cordis-rs plugin state rendered to stdout.
//!
//! The final goal is to display plugin state (e.g. the registered command
//! list) in a GPUI window. This binary deliberately stops before that step:
//! it does not import gpui and starts no window, so it runs in CI without a
//! display server. `visible_commands` below reads the shared `commands`
//! service (`core_services::provide_commands`) that a future GPUI view
//! consumes, so `/compact` registration is observable from either side.

use std::sync::Arc;

use cordis_rs::core_services::{
    CommandDefinition, CommandInvocation, CommandRegistry, CommandResult, provide_commands,
};
use cordis_rs::{Context, Error, Fiber, FunctionPlugin, Lifecycle, Result};

/// Minimal stand-in for the `compaction` service that upstream
/// `dsh-command-compact` injects alongside `commands`.
struct Compaction;

/// The `/compact` definition registered into the shared `CommandRegistry`.
///
/// The handler is a no-op stand-in: the bridge only needs the registration to
/// be observable through the same service the GPUI demos bootstrap, not a real
/// compaction run.
fn compact_definition() -> CommandDefinition {
    CommandDefinition::new(
        "/compact",
        "Compact older conversation history",
        Arc::new(|_invocation: CommandInvocation| {
            Box::pin(async { CommandResult::success("compacted") })
        }),
    )
}

/// A `dsh-command-compact`-equivalent plugin built with `FunctionPlugin`.
///
/// `inject` mirrors the upstream plugin's `['commands', 'compaction']`; its
/// body registers `/compact` into the shared commands service. Both services
/// are provided before mounting, so the fiber loads immediately and no
/// notification-driven reload task is spawned.
fn command_compact_plugin() -> FunctionPlugin {
    FunctionPlugin::new("command-compact", |ctx: &Context| {
        // `FunctionPlugin` needs a 'static future; own the context rather
        // than borrowing the reference passed to the apply closure.
        let ctx = ctx.clone();
        async move {
            let commands = ctx.require::<CommandRegistry>("commands").await?;
            let _compaction = ctx.require::<Compaction>("compaction").await?;
            commands.register(compact_definition());
            Ok(())
        }
    })
    .with_inject(vec!["commands", "compaction"])
}

/// Provide the shared commands service plus the host stand-in, then mount the
/// command-compact plugin. Returns the mounted fiber.
async fn mount_bridge(ctx: &Context) -> Result<Fiber> {
    provide_commands(ctx).await?;
    ctx.provide("compaction", Compaction).await?;
    Fiber::mount(ctx, Box::new(command_compact_plugin())).await
}

/// Read the slash commands visible through the shared `commands` service.
///
/// A GPUI view can call the same helper on its own `Context`: the registry is
/// installed by `core_services::provide_commands`, not by a bridge-local type.
fn visible_commands(ctx: &Context) -> Result<Vec<(String, String)>> {
    let registry = ctx
        .get::<CommandRegistry>("commands")
        .ok_or_else(|| Error::ServiceNotFound("commands".to_string()))?;
    Ok(registry
        .names()
        .into_iter()
        .map(|name| {
            let description = registry.description(&name).unwrap_or_default();
            (name, description)
        })
        .collect())
}

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = Context::root("gpui-bridge");

    let fiber = mount_bridge(&ctx).await?;
    println!(
        "cordis-rs bridge: plugin `{}` state = {:?}",
        fiber.name(),
        fiber.state()
    );
    if fiber.state() != Lifecycle::Active {
        return Err(Error::Other(format!(
            "plugin `{}` did not become active",
            fiber.name()
        )));
    }

    let list = visible_commands(&ctx)?;
    println!("Registered commands ({}):", list.len());
    for (command, description) in &list {
        println!("  - {command} — {description}");
    }
    if !list.iter().any(|(command, _)| command == "/compact") {
        return Err(Error::Other(
            "command-compact did not register `/compact`".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn compact_is_visible_in_shared_command_registry() {
        let ctx = Context::root("gpui-bridge-test");
        let fiber = mount_bridge(&ctx).await.expect("bridge should mount");

        assert_eq!(fiber.state(), Lifecycle::Active);
        let visible = visible_commands(&ctx).expect("shared commands service");
        assert!(
            visible.iter().any(|(name, _)| name == "/compact"),
            "expected `/compact` in shared registry, got {visible:?}"
        );
        assert!(
            visible
                .iter()
                .any(|(name, description)| name == "/compact" && description.contains("Compact")),
            "expected a description for `/compact`, got {visible:?}"
        );
    }

    #[tokio::test]
    async fn shared_registry_starts_empty_before_plugin_mount() {
        let ctx = Context::root("gpui-bridge-empty-test");
        provide_commands(&ctx)
            .await
            .expect("provide shared commands");
        ctx.provide("compaction", Compaction)
            .await
            .expect("provide compaction");

        let visible = visible_commands(&ctx).expect("shared commands service");
        assert!(
            !visible.iter().any(|(name, _)| name == "/compact"),
            "`/compact` must come from the plugin, not bootstrap: {visible:?}"
        );
    }
}

//! Headless bridge demo: cordis-rs plugin state rendered to stdout.
//!
//! The final goal is to display plugin state (e.g. the registered command
//! list) in a GPUI window. This binary deliberately stops before that step:
//! it does not import gpui and starts no window, so it runs in CI without a
//! display server. `commands` below is the state a future GPUI view consumes.

use std::sync::Mutex;

use cordis_rs::{Context, Error, Fiber, FunctionPlugin, Lifecycle, Result};

/// The `commands` service: a registry of command names a plugin can add to.
#[derive(Default)]
struct Commands {
    registered: Mutex<Vec<String>>,
}

impl Commands {
    fn register(&self, command: impl Into<String>) {
        self.registered
            .lock()
            .expect("commands registry poisoned")
            .push(command.into());
    }

    fn list(&self) -> Vec<String> {
        self.registered
            .lock()
            .expect("commands registry poisoned")
            .clone()
    }
}

/// Minimal stand-in for the `compaction` service that upstream
/// `dsh-command-compact` injects alongside `commands`.
struct Compaction;

/// A `dsh-command-compact`-equivalent plugin built with `FunctionPlugin`.
///
/// `inject` mirrors the upstream plugin's `['commands', 'compaction']`; its
/// body registers `/compact` into the `commands` service. Both services are
/// provided before mounting, so the fiber loads immediately and no
/// notification-driven reload task is spawned.
fn command_compact_plugin() -> FunctionPlugin {
    FunctionPlugin::new("command-compact", |ctx: &Context| {
        // `FunctionPlugin` needs a 'static future; own the context rather
        // than borrowing the reference passed to the apply closure.
        let ctx = ctx.clone();
        async move {
            let commands = ctx.require::<Commands>("commands").await?;
            let _compaction = ctx.require::<Compaction>("compaction").await?;
            commands.register("/compact");
            Ok(())
        }
    })
    .with_inject(vec!["commands", "compaction"])
}

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = Context::root("gpui-bridge");

    ctx.provide("commands", Commands::default()).await?;
    ctx.provide("compaction", Compaction).await?;

    let fiber = Fiber::mount(&ctx, Box::new(command_compact_plugin())).await?;
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

    let commands = ctx
        .get::<Commands>("commands")
        .ok_or_else(|| Error::ServiceNotFound("commands".to_string()))?;
    let list = commands.list();
    println!("Registered commands ({}):", list.len());
    for command in &list {
        println!("  - {command}");
    }
    if !list.iter().any(|command| command == "/compact") {
        return Err(Error::Other(
            "command-compact did not register `/compact`".to_string(),
        ));
    }

    Ok(())
}

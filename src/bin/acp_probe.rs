//! Probe: verify a Rust ACP client can initialize and open a session on
//! `dsh --profile acp`. No prompt is sent, so no model credentials are used.

use agent_client_protocol::schema::{
    InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse, ProtocolVersion,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

// ---- Secure `dsh` launch -------------------------------------------------
//
// Never resolve `dsh` through the ambient `PATH`: a poisoned entry (`.`, a
// world-writable temp dir, an attacker-controlled bin dir) could substitute a
// fake `dsh` and capture the ACP stdio traffic. Resolve an absolute path from
// an explicit override or a fixed allowlist, then give the child a minimal,
// attacker-independent `PATH`.

/// System dirs safe to expose to `dsh` and its children. `node` for the
/// `#!/usr/bin/env node` shebang lives in `/usr/bin`.
const MINIMAL_PATH_DIRS: &[&str] = &["/usr/local/bin", "/usr/bin", "/bin"];

/// Absolute directories searched for `dsh` when `DSH_BIN` is unset.
/// The ambient `PATH` is intentionally never consulted.
fn trusted_dsh_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home.filter(|home| !home.as_os_str().is_empty()) {
        dirs.push(home.join(".npm-global/bin"));
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".local/share/mise/shims"));
        dirs.push(home.join("bin"));
    }
    dirs.extend(MINIMAL_PATH_DIRS.iter().map(PathBuf::from));
    dirs
}

/// Ordered absolute candidates for `dsh`. `DSH_BIN` wins; a relative value is
/// resolved against `cwd`, so every candidate is absolute.
fn dsh_candidates(dsh_bin: Option<&Path>, cwd: &Path, search_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(bin) = dsh_bin.filter(|bin| !bin.as_os_str().is_empty()) {
        candidates.push(if bin.is_absolute() {
            bin.to_path_buf()
        } else {
            cwd.join(bin)
        });
    }
    candidates.extend(search_dirs.iter().map(|dir| dir.join("dsh")));
    candidates
}

/// Resolve `dsh` to the first existing absolute candidate.
fn resolve_dsh_from(
    dsh_bin: Option<&Path>,
    cwd: &Path,
    search_dirs: &[PathBuf],
) -> Result<PathBuf, String> {
    dsh_candidates(dsh_bin, cwd, search_dirs)
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "dsh executable not found in trusted locations; set DSH_BIN to its absolute path"
                .to_owned()
        })
}

/// Minimal `PATH` for the child: the resolved binary's own directory (so
/// sibling helpers and the `node` shebang resolve) plus fixed system dirs.
fn minimal_child_path(dsh_path: &Path) -> Result<OsString, String> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = dsh_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        dirs.push(parent.to_path_buf());
    }
    dirs.extend(MINIMAL_PATH_DIRS.iter().map(PathBuf::from));
    std::env::join_paths(&dirs).map_err(|error| format!("cannot build PATH: {error}"))
}

/// Resolve from the process environment into `(absolute dsh path, child PATH)`.
fn dsh_command_parts() -> Result<(PathBuf, OsString), String> {
    let dsh_bin = std::env::var_os("DSH_BIN").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let cwd = std::env::current_dir().map_err(|error| format!("cannot read cwd: {error}"))?;
    let dsh_path = resolve_dsh_from(dsh_bin.as_deref(), &cwd, &trusted_dsh_dirs(home.as_deref()))?;
    let child_path = minimal_child_path(&dsh_path)?;
    Ok((dsh_path, child_path))
}

/// How the graceful shutdown of the `dsh --profile acp` subprocess ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownOutcome {
    /// The agent exited on its own after the ACP transport was closed (EOF).
    Exited,
    /// The agent ignored EOF and was killed after the grace period elapsed.
    KilledAfterTimeout,
    /// `wait()` itself failed, so the child state is unknown.
    WaitFailed,
}

/// Gracefully tear down the `dsh --profile acp` subprocess.
///
/// # Why no `session/close` request (unstable-feature decision)
///
/// In the pinned `agent-client-protocol` 0.11.1, `CloseSessionRequest`
/// (`session/close`) only exists behind the `unstable_session_close` feature.
/// The schema marks it **UNSTABLE**: "not part of the spec yet, and may be
/// removed or changed at any point" (see `agent-client-protocol-schema`
/// `agent.rs` near `CloseSessionRequest`, and the crate's
/// `schema/client_to_agent/requests.rs:52-53`). Enabling it would mean editing
/// `Cargo.toml` (outside this task's two-file scope), and the project's own
/// dependency triage (`investigation/round-5.md`) records that the gate was
/// renamed/removed in 5.3.0, so a `session/close`-based implementation would
/// have to be rewritten on the next ACP upgrade. Decision: do **not** enable
/// any unstable feature.
///
/// The stable, wire-compatible alternative used here: when `connect_with`
/// returns, the ACP connection and its stdio transport are dropped, which
/// closes our write half of the child's stdin. A stdio ACP agent observes that
/// EOF as session teardown (cancel in-flight work, release session resources).
/// We then allow a bounded grace period for a clean exit and only escalate to
/// `kill()` as a last resort, so a hung agent can neither leak a subprocess
/// nor block this probe forever. (`session/cancel` is stable, but this probe
/// sends no prompts, so there is no in-flight turn to cancel.)
async fn shutdown_child(child: &mut tokio::process::Child, grace: Duration) -> ShutdownOutcome {
    match tokio::time::timeout(grace, child.wait()).await {
        Ok(Ok(_status)) => ShutdownOutcome::Exited,
        Ok(Err(error)) => {
            eprintln!("child wait failed: {error}");
            ShutdownOutcome::WaitFailed
        }
        Err(_elapsed) => {
            eprintln!("child did not exit within {grace:?}; killing");
            let _ = child.kill().await;
            let _ = child.wait().await; // reap so no zombie is left behind
            ShutdownOutcome::KilledAfterTimeout
        }
    }
}

/// Run the ACP handshake (`initialize` + `session/new`) over any transport.
///
/// The `dsh` subprocess transport and the offline `Channel::duplex()` tests
/// share this path, so the seam the probe checks can be exercised without a
/// model provider or credentials.
async fn initialize_and_open_session(
    cwd: PathBuf,
    transport: impl ConnectTo<Client> + 'static,
) -> Result<(InitializeResponse, NewSessionResponse), String> {
    Client
        .builder()
        .name("dsh-gpui-probe")
        .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            Ok((init, session))
        })
        .await
        .map_err(|error| error.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    eprintln!("cwd = {}", cwd.display());

    let (dsh_path, child_path) = dsh_command_parts()?;
    let mut cmd = tokio::process::Command::new(&dsh_path);
    cmd.env("PATH", child_path);
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;

    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
    let (init, session) = initialize_and_open_session(cwd, transport).await?;
    eprintln!("INIT OK: agent_info={:?}", init.agent_info);
    eprintln!("SESSION OK: id={:?}", session.session_id);

    // The ACP connection (and with it the stdio transport / our stdin write
    // half) was dropped when `initialize_and_open_session` returned, so the
    // agent has already seen EOF. Wait for a clean exit before killing.
    let outcome = shutdown_child(&mut child, Duration::from_secs(5)).await;
    eprintln!("SHUTDOWN = {outcome:?}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use agent_client_protocol::schema::{
        Implementation, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptRequest, PromptResponse, ProtocolVersion, SessionId, StopReason,
    };
    use agent_client_protocol::{Agent, Channel};

    use super::{ShutdownOutcome, initialize_and_open_session, shutdown_child};

    const SESSION_ID: &str = "offline-probe-session";
    const AGENT_NAME: &str = "offline-fake-agent";

    #[derive(Default)]
    struct Observed {
        protocol_version: Mutex<Option<ProtocolVersion>>,
        cwd: Mutex<Option<PathBuf>>,
        prompt_count: AtomicUsize,
    }

    /// Serve a fake ACP agent on one end of an in-process duplex channel.
    ///
    /// It records what the client sent and replies with a fixed session id.
    /// No subprocess, model provider, or balance is involved. A prompt
    /// handler is registered only to detect (and fail) an unexpected prompt.
    fn spawn_fake_agent(observed: Arc<Observed>) -> agent_client_protocol::Channel {
        let (client_channel, agent_channel) = Channel::duplex();
        let observed_init = Arc::clone(&observed);
        let observed_new = Arc::clone(&observed);
        tokio::spawn(async move {
            let builder = Agent
                .builder()
                .name(AGENT_NAME)
                .on_receive_request(
                    async move |request: InitializeRequest, responder, _cx| {
                        let version = request.protocol_version;
                        *observed_init.protocol_version.lock().unwrap() = Some(version.clone());
                        responder.respond(
                            InitializeResponse::new(version)
                                .agent_info(Implementation::new(AGENT_NAME, "0.0.0")),
                        )
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |request: NewSessionRequest, responder, _cx| {
                        *observed_new.cwd.lock().unwrap() = Some(request.cwd);
                        responder.respond(NewSessionResponse::new(SessionId::from(SESSION_ID)))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |_request: PromptRequest, responder, _cx| {
                        observed.prompt_count.fetch_add(1, Ordering::SeqCst);
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    },
                    agent_client_protocol::on_receive_request!(),
                );
            let _ = builder.connect_to(agent_channel).await;
        });
        client_channel
    }

    #[tokio::test]
    async fn initialize_and_new_session_work_over_duplex_channel() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(Arc::clone(&observed));
        let cwd = PathBuf::from("/tmp/offline-acp-probe");

        let (init, session) = initialize_and_open_session(cwd.clone(), client_channel)
            .await
            .expect("offline handshake should succeed");

        assert_eq!(init.protocol_version, ProtocolVersion::V1);
        assert_eq!(
            init.agent_info.as_ref().map(|info| info.name.as_str()),
            Some(AGENT_NAME)
        );
        assert_eq!(session.session_id, SessionId::from(SESSION_ID));

        // The fake agent only observes these values if the real requests
        // traversed the channel, so these assert the handshake really ran.
        assert_eq!(
            *observed.protocol_version.lock().unwrap(),
            Some(ProtocolVersion::V1)
        );
        assert_eq!(*observed.cwd.lock().unwrap(), Some(cwd));

        // This probe must stay free of model calls.
        assert_eq!(observed.prompt_count.load(Ordering::SeqCst), 0);
    }

    /// Returning from `connect_with` drops the ACP transport and with it our
    /// stdin write half. A child blocked on stdin must then see EOF and exit
    /// on its own. This is the stable session-close mechanism `shutdown_child`
    /// relies on instead of the unstable `session/close` request, so a
    /// regression that keeps the pipe open must fail this test.
    #[tokio::test]
    async fn dropping_acp_transport_delivers_stdin_eof_to_child() {
        use agent_client_protocol::{ByteStreams, Client};
        use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

        let mut child = tokio::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("spawn a child that blocks on stdin");
        let stdin = child.stdin.take().expect("child has a piped stdin");
        let transport = ByteStreams::new(stdin.compat_write(), tokio::io::empty().compat());

        // The exact result is irrelevant; what matters is that returning from
        // `connect_with` drops the transport and closes the child's stdin.
        let _ = Client
            .builder()
            .name("acp-probe-eof-test")
            .connect_with(transport, |_cx| async { Ok(()) })
            .await;

        let outcome = shutdown_child(&mut child, Duration::from_secs(2)).await;
        assert_eq!(outcome, ShutdownOutcome::Exited);
    }

    /// A child that exits on its own must be reported as a clean shutdown;
    /// this guards against regressing back to an unconditional `kill()`.
    #[tokio::test]
    async fn shutdown_child_reports_natural_exit() {
        let mut child = tokio::process::Command::new("true")
            .spawn()
            .expect("spawn a short-lived child");

        let outcome = shutdown_child(&mut child, Duration::from_secs(5)).await;

        assert_eq!(outcome, ShutdownOutcome::Exited);
    }

    /// A hung agent must still be terminated and reaped after the grace
    /// period, so the probe cannot leak a subprocess or hang indefinitely.
    #[tokio::test]
    async fn shutdown_child_kills_a_hung_child_after_grace() {
        let mut child = tokio::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn a long-lived child");

        let outcome = shutdown_child(&mut child, Duration::from_millis(100)).await;

        assert_eq!(outcome, ShutdownOutcome::KilledAfterTimeout);
    }

    // ---- Secure `dsh` launch: offline regression tests (no subprocess) ----

    #[test]
    fn dsh_candidates_prefer_dsh_bin_and_stay_absolute() {
        let cwd = PathBuf::from("/work");
        let dirs = vec![PathBuf::from("/trusted/bin")];

        let from_abs = super::dsh_candidates(Some(&PathBuf::from("/opt/dsh")), &cwd, &dirs);
        assert_eq!(from_abs.first(), Some(&PathBuf::from("/opt/dsh")));

        let from_rel = super::dsh_candidates(Some(&PathBuf::from("local/dsh")), &cwd, &dirs);
        assert_eq!(from_rel.first(), Some(&PathBuf::from("/work/local/dsh")));

        let from_search = super::dsh_candidates(None, &cwd, &dirs);
        assert_eq!(from_search, vec![PathBuf::from("/trusted/bin/dsh")]);

        assert!(
            from_abs
                .iter()
                .chain(&from_rel)
                .chain(&from_search)
                .all(|path| path.is_absolute())
        );
    }

    #[test]
    fn trusted_dsh_dirs_are_absolute_and_system_backed() {
        let dirs = super::trusted_dsh_dirs(Some(&PathBuf::from("/home/operator")));
        assert!(
            dirs.iter().all(|dir| dir.is_absolute()),
            "no relative (hijackable) dir: {dirs:?}"
        );
        assert!(dirs.iter().any(|dir| dir == &PathBuf::from("/usr/bin")));
        assert!(
            dirs.iter()
                .any(|dir| dir == &PathBuf::from("/home/operator/.npm-global/bin"))
        );
        assert!(
            dirs.iter()
                .all(|dir| dir != &PathBuf::from(".") && dir != &PathBuf::from("/tmp"))
        );
    }

    #[test]
    fn resolve_dsh_from_picks_existing_file_and_reports_missing() {
        let tmp = std::env::temp_dir().join(format!("dsh-pathpin-resolve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let fake = tmp.join("dsh");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").expect("write fake dsh");

        let resolved = super::resolve_dsh_from(Some(&fake), &PathBuf::from("/"), &[])
            .expect("resolve fake dsh");
        assert_eq!(resolved, fake);
        assert!(resolved.is_absolute());

        let missing = super::resolve_dsh_from(Some(&tmp.join("absent")), &PathBuf::from("/"), &[]);
        assert!(missing.is_err(), "missing DSH_BIN must be an error");

        std::fs::remove_dir_all(&tmp).expect("remove temp dir");
    }

    #[test]
    fn minimal_child_path_is_fixed_and_keeps_node_discoverable() {
        let path = super::minimal_child_path(&PathBuf::from("/home/operator/.npm-global/bin/dsh"))
            .expect("build minimal PATH");
        let parts: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert_eq!(
            parts.first(),
            Some(&PathBuf::from("/home/operator/.npm-global/bin"))
        );
        assert!(
            parts.iter().any(|part| part == &PathBuf::from("/usr/bin")),
            "`env node` shebang needs /usr/bin: {parts:?}"
        );
        assert!(
            parts.iter().all(|part| part.is_absolute()),
            "no empty/relative PATH entry: {parts:?}"
        );
        assert!(
            parts
                .iter()
                .all(|part| part != &PathBuf::from("/tmp") && part != &PathBuf::from("."))
        );
    }
}

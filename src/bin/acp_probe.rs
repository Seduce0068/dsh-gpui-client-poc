//! Probe: verify a Rust ACP client can initialize and open a session on
//! `dsh --profile acp`. No prompt is sent, so no model credentials are used.
//!
//! # 出力契約 (log stream contract)
//!
//! 全ての出力は `emit` を通り、行の種別でストリームが決まる。
//! - `ProbeStream::Stdout`（`println!`）: 通常出力＝パイプ・grep 対象の結果。
//!   本 probe では handshake 成功行（INIT OK / SESSION OK）がこれに該当する。
//! - `ProbeStream::Stderr`（`eprintln!`）: 診断出力のみ。起動コンテキスト
//!   （cwd）、子プロセスの警告、shutdown 結果。結果を決して stderr に
//!   隠さない（旧挙動では stdout が空でパイプ処理できなかった）。

use agent_client_protocol::schema::{
    InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse, ProtocolVersion,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectTo, ConnectionTo};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[path = "../acp_launch.rs"]
mod acp_launch;
use acp_launch::*;

// ---- Output contract: one routing point for stdout vs stderr -------------
//
// `emit` が宛先を一箇所で固定する。テストはスレッドローカルの
// `OUTPUT_CAPTURE` に差し替えて、グローバルな stdout/stderr を奪わずに
// 各呼び出し箇所のストリームを検証する（本番は None のまま）。

// ---- Secure `dsh` launch -------------------------------------------------
//
// Never resolve `dsh` through the ambient `PATH`: a poisoned entry (`.`, a
// world-writable temp dir, an attacker-controlled bin dir) could substitute a
// fake `dsh` and capture the ACP stdio traffic. Resolve an absolute path from
// an explicit override or a fixed allowlist, then give the child a minimal,
// attacker-independent `PATH`.

// ---- dsh stderr capture: redaction + correlation -------------------------
//
// The child's stderr is piped and drained line by line instead of being
// inherited. Every line is masked by `redact_secrets` and rendered with a
// `[dsh pid=… session=…]` correlation id before it reaches the diagnostics
// (stderr) stream, so provider credentials printed by `dsh` never leak into
// the log. This mirrors `acp_prompt_probe.rs`; the offline tests exercise both
// the redaction policy and the drain without spawning `dsh`.

/// 起動コンテキストは診断情報なので stderr へ。
fn report_startup(cwd: &Path) {
    emit(ProbeStream::Stderr, &format!("cwd = {}", cwd.display()));
}

/// handshake の成功結果はパイプ可能な結果なので stdout へ。
fn report_handshake(init: &InitializeResponse, session: &NewSessionResponse) {
    emit(
        ProbeStream::Stdout,
        &format!("INIT OK: agent_info={:?}", init.agent_info),
    );
    emit(
        ProbeStream::Stdout,
        &format!("SESSION OK: id={:?}", session.session_id),
    );
}

/// teardown の成否は診断情報なので stderr へ。
fn report_shutdown(outcome: ShutdownOutcome) {
    emit(ProbeStream::Stderr, &format!("SHUTDOWN = {outcome:?}"));
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
        // DSH only emits `session/request_permission` (agent -> client) for
        // owned sessions whose policy is `ask`. Registering the handler on the
        // Client builder prevents that request from being answered with
        // JSON-RPC `method_not_found`. This probe has no UI and sends no
        // prompts, so there is nobody to ask: answer `Cancelled`, never an
        // implicit approval. `fs/*` and `terminal/*` are intentionally NOT
        // registered (DSH's bridge never emits them).
        //
        // Feasibility note: in agent-client-protocol 0.11.1
        // `Builder::on_receive_request` is generic over `Host` and only needs
        // `Host::Counterpart: HasPeer<Host::Counterpart>`. Here `Host =
        // Client`, so the bound is `Agent: HasPeer<Agent>`, which is implemented
        // in `role/acp.rs`. `RequestPermissionRequest` implements
        // `JsonRpcRequest` with response `RequestPermissionResponse`
        // (`schema/agent_to_client/requests.rs`) and belongs to the
        // `AgentRequest` enum (client-side inbound requests). Therefore the
        // handler can be registered here; no API workaround is needed.
        .on_receive_request(
            async move |_request: RequestPermissionRequest, responder, _cx| {
                responder.respond(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Cancelled,
                ))
            },
            agent_client_protocol::on_receive_request!(),
        )
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
        .map_err(|error| redact_secrets(&error.to_string()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    report_startup(&cwd);

    let (dsh_path, child_path) = dsh_command_parts()
        .map_err(|error| Box::<dyn std::error::Error>::from(redact_secrets(&error)))?;
    let mut cmd = tokio::process::Command::new(&dsh_path);
    cmd.env("PATH", child_path);
    scrub_child_env(&mut cmd);
    cmd.args(["--profile", "acp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|error| Box::<dyn std::error::Error>::from(redact_secrets(&error.to_string())))?;

    // Capture stderr before any fallible work starts. `child.id()` is always
    // Some after a successful spawn; the fallback only keeps the correlation
    // total instead of leaking the child on an impossible None.
    let correlation = DshCorrelation::new(child.id().unwrap_or(0));
    let capture = child
        .stderr
        .take()
        .map(|stderr| spawn_stderr_capture(stderr, correlation.clone()));

    // Everything after spawn lives in one fallible block: an error in stdio
    // acquisition or in the ACP handshake must still reach the teardown below
    // instead of returning early with a live child.
    let result = async {
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
        let (init, session) = initialize_and_open_session(cwd, transport).await?;
        correlation.set_session_id(session.session_id.to_string());
        report_handshake(&init, &session);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;

    // The ACP connection (and with it the stdio transport / our stdin write
    // half) is dropped when the block above ends, on success or on error, so
    // the agent has already seen EOF. Always tear down and reap the child,
    // then drain and render the captured stderr before surfacing `result`.
    let outcome = shutdown_child(&mut child, Duration::from_secs(5)).await;
    report_shutdown(outcome);
    if let Some(capture) = capture {
        let _ = capture.await;
    }
    correlation.flush();
    // Last line of defence: `Termination` prints whatever `main` returns.
    result
        .map_err(|error| Box::<dyn std::error::Error>::from(redact_secrets(&error.to_string())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use agent_client_protocol::schema::{
        Implementation, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PermissionOption, PermissionOptionKind, PromptRequest, PromptResponse,
        ProtocolVersion, RequestPermissionOutcome, RequestPermissionRequest,
        RequestPermissionResponse, SessionId, StopReason, ToolCallUpdate, ToolCallUpdateFields,
    };
    use agent_client_protocol::{Agent, Channel, Client, ConnectionTo};
    use tokio::io::AsyncWriteExt;

    use super::{
        DshCorrelation, ProbeStream, ShutdownOutcome, initialize_and_open_session, redact_secrets,
        redaction_key, report_handshake, report_shutdown, report_startup, shutdown_child,
        spawn_stderr_capture, start_capture, take_capture,
    };

    const SESSION_ID: &str = "offline-probe-session";
    const AGENT_NAME: &str = "offline-fake-agent";

    /// Per-test temporary directory, removed on drop.
    ///
    /// The path combines the process id, an atomic sequence number and a
    /// nanosecond timestamp, so concurrent tests never share a directory and
    /// a reused pid across runs cannot collide either. `Drop` removes it even
    /// when a test panics, leaving no residue in the system temp directory.
    struct TestTempDir(PathBuf);

    impl TestTempDir {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let seq = NEXT.fetch_add(1, Ordering::SeqCst);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default();
            let path = std::env::temp_dir().join(format!(
                "dsh-gpui-{tag}-{}-{seq}-{nanos:x}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create per-test temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Default)]
    struct Observed {
        protocol_version: Mutex<Option<ProtocolVersion>>,
        cwd: Mutex<Option<PathBuf>>,
        prompt_count: AtomicUsize,
        /// Agent-side view of the `session/request_permission` reply. `None`
        /// means the request never reached the client handler.
        permission: Mutex<Option<Result<RequestPermissionResponse, String>>>,
    }

    /// Serve a fake ACP agent on one end of an in-process duplex channel.
    ///
    /// It records what the client sent and replies with a fixed session id.
    /// No subprocess, model provider, or balance is involved. A prompt
    /// handler is registered only to detect (and fail) an unexpected prompt.
    fn spawn_fake_agent(
        observed: Arc<Observed>,
        request_permission: bool,
    ) -> agent_client_protocol::Channel {
        let (client_channel, agent_channel) = Channel::duplex();
        let observed_init = Arc::clone(&observed);
        let observed_new = Arc::clone(&observed);
        let observed_permission = Arc::clone(&observed);
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
            if request_permission {
                // Send an agent -> client `session/request_permission` as soon
                // as the connection is up. A client without the handler would
                // answer `method_not_found`; with the handler it gets a
                // `Cancelled` response. The request is issued from main_fn
                // (not from a request handler), so awaiting the reply cannot
                // deadlock the single-task event loop.
                let _ = builder
                    .connect_with(agent_channel, |cx: ConnectionTo<Client>| async move {
                        let request = RequestPermissionRequest::new(
                            SessionId::from(SESSION_ID),
                            ToolCallUpdate::new("call-1", ToolCallUpdateFields::new()),
                            vec![PermissionOption::new(
                                "allow-once",
                                "Allow once",
                                PermissionOptionKind::AllowOnce,
                            )],
                        );
                        let result = cx
                            .send_request(request)
                            .block_task()
                            .await
                            .map_err(|error| error.to_string());
                        *observed_permission.permission.lock().unwrap() = Some(result);
                        std::future::pending::<Result<(), agent_client_protocol::Error>>().await
                    })
                    .await;
            } else {
                let _ = builder.connect_to(agent_channel).await;
            }
        });
        client_channel
    }

    #[tokio::test]
    async fn initialize_and_new_session_work_over_duplex_channel() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(Arc::clone(&observed), false);
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

    /// Offline regression for the DSH r4 `method_not_found` fix.
    ///
    /// The fake agent sends an agent -> client `session/request_permission`
    /// request. Without a handler on the `Client` builder the agent would get
    /// a JSON-RPC `method_not_found` error; with the handler it must receive a
    /// `Cancelled` response (this probe has no UI to ask a real user). No
    /// subprocess, model provider, or credentials are involved.
    #[tokio::test]
    async fn client_answers_request_permission_without_method_not_found() {
        let observed = Arc::new(Observed::default());
        let client_channel = spawn_fake_agent(Arc::clone(&observed), true);
        let cwd = PathBuf::from("/tmp/offline-acp-probe");

        let (init, session) = initialize_and_open_session(cwd, client_channel)
            .await
            .expect("offline handshake should succeed");
        assert_eq!(init.protocol_version, ProtocolVersion::V1);
        assert_eq!(session.session_id, SessionId::from(SESSION_ID));

        // The client handler runs on the connection's background task, so the
        // agent's observation may land slightly after the handshake returns.
        for _ in 0..100 {
            if observed.permission.lock().unwrap().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let result = observed
            .permission
            .lock()
            .unwrap()
            .clone()
            .expect("agent must observe a reply to session/request_permission");
        let response = result.expect("permission request must not fail with method_not_found");
        assert_eq!(response.outcome, RequestPermissionOutcome::Cancelled);
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

    // ---- 出力契約の回帰テスト（stdout/stderr の使い分け） ----

    /// 成功結果は stdout、起動 context と shutdown は stderr へ。
    /// 旧挙動（INIT OK / SESSION OK が stderr のみ）の再発を防ぐ。
    #[test]
    fn results_go_to_stdout_and_diagnostics_to_stderr() {
        let init = InitializeResponse::new(ProtocolVersion::V1)
            .agent_info(Implementation::new(AGENT_NAME, "0.0.0"));
        let session = NewSessionResponse::new(SessionId::from(SESSION_ID));

        start_capture();
        report_startup(std::path::Path::new("/work"));
        report_handshake(&init, &session);
        report_shutdown(ShutdownOutcome::Exited);
        let lines = take_capture();

        assert_eq!(
            lines,
            vec![
                (ProbeStream::Stderr, "cwd = /work".to_string()),
                (
                    ProbeStream::Stdout,
                    format!("INIT OK: agent_info={:?}", init.agent_info)
                ),
                (
                    ProbeStream::Stdout,
                    format!("SESSION OK: id={:?}", session.session_id)
                ),
                (ProbeStream::Stderr, "SHUTDOWN = Exited".to_string()),
            ]
        );
    }

    /// kill に至る警告も診断なので stderr に留まり、結果ストリームを汚さない。
    #[tokio::test]
    async fn shutdown_child_warning_stays_on_stderr() {
        start_capture();
        let mut child = tokio::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn a long-lived child");

        let outcome = shutdown_child(&mut child, Duration::from_millis(100)).await;
        let lines = take_capture();

        assert_eq!(outcome, ShutdownOutcome::KilledAfterTimeout);
        assert!(!lines.is_empty(), "kill path must emit a diagnostic");
        assert!(
            lines
                .iter()
                .all(|(stream, _)| *stream == ProbeStream::Stderr),
            "shutdown diagnostics must never pollute stdout: {lines:?}"
        );
        assert!(
            lines.iter().any(|(_, text)| text.contains("killing")),
            "the timeout warning must be present: {lines:?}"
        );
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
        // No explicit cleanup: `TestTempDir::drop` owns removal, so an early
        // failure cannot leave a stale directory behind.
        let tmp = TestTempDir::new("pathpin-resolve");
        let fake = tmp.path().join("dsh");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").expect("write fake dsh");

        let resolved = super::resolve_dsh_from(Some(&fake), &PathBuf::from("/"), &[])
            .expect("resolve fake dsh");
        assert_eq!(resolved, fake);
        assert!(resolved.is_absolute());

        let missing =
            super::resolve_dsh_from(Some(&tmp.path().join("absent")), &PathBuf::from("/"), &[]);
        assert!(missing.is_err(), "missing DSH_BIN must be an error");
    }

    /// Regression: each test gets its own temp dir, and dropping one must not
    /// delete another test's dir or leave its own behind.
    #[test]
    fn temp_dirs_are_unique_and_removed_on_drop() {
        let first = TestTempDir::new("isolation");
        let second = TestTempDir::new("isolation");
        let first_path = first.path().to_path_buf();
        let second_path = second.path().to_path_buf();

        assert_ne!(
            first_path, second_path,
            "parallel tests must not share a temp dir"
        );
        assert!(first_path.is_dir(), "temp dir must be a real directory");
        assert!(second_path.is_dir(), "temp dir must be a real directory");

        drop(first);
        assert!(!first_path.exists(), "temp dir must be removed on drop");
        assert!(
            second_path.is_dir(),
            "dropping one test's dir must not remove another's"
        );
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

    // ---- dsh stderr capture: redaction + correlation (offline) ------------

    /// The redaction policy masks credentials but keeps ordinary diagnostics.
    #[test]
    fn redaction_masks_credentials_but_keeps_ordinary_text() {
        assert_eq!(redact_secrets("plain message"), "plain message");

        let masked = redact_secrets("api_key=sk-secret-value ordinary");
        assert!(masked.contains("api_key=[REDACTED]"), "masked: {masked}");
        assert!(!masked.contains("sk-secret-value"), "masked: {masked}");
        assert!(masked.contains("ordinary"), "masked: {masked}");

        let masked = redact_secrets("Authorization: Bearer sk-abc123");
        assert!(!masked.contains("sk-abc123"), "masked: {masked}");
    }

    /// URL / JSON / quoted boundaries must reduce to the same bare key list.
    #[test]
    fn redaction_key_strips_url_json_and_quote_wrappers() {
        assert_eq!(redaction_key("?token"), "token");
        assert_eq!(redaction_key("&access_token"), "access_token");
        assert_eq!(redaction_key("\"api_key\""), "api_key");
        assert_eq!(redaction_key("{\"api_key\""), "api_key");
        assert_eq!(redaction_key("['refresh_token'"), "refresh_token");
        assert_eq!(redaction_key("api_key"), "api_key");
    }

    /// Credentials wrapped in URL queries, JSON quotes or shell quotes must
    /// never survive into a diagnostic line.
    #[test]
    fn redaction_masks_url_json_and_quoted_values() {
        let cases = [
            "https://host/path?api_key=sk-leaked-url",
            "\"api_key\":\"sk-leaked-json\"",
            "{'token': 'sk-leaked-quoted'}",
            "api_key=\"sk-leaked-dq\"",
            "token='abcd'",
        ];
        for case in cases {
            let masked = redact_secrets(case);
            assert!(
                masked.contains("[REDACTED]")
                    && !masked.contains("sk-leaked")
                    && !masked.contains("abcd"),
                "unmasked boundary: {case} -> {masked}"
            );
        }
        // The original `?token=` contract is unchanged.
        assert_eq!(redact_secrets("?token=abcd"), "token=[REDACTED]");
    }

    /// The offline sink used by transport tests never emits diagnostics, so
    /// adding correlation to the shared handshake path cannot pollute stdout.
    #[test]
    fn offline_correlation_never_emits() {
        let correlation = DshCorrelation::offline();
        correlation.set_session_id("offline-session");
        correlation.record_stderr_line("api_key=sk-would-leak".to_string());
        start_capture();
        correlation.flush();
        assert!(
            take_capture().is_empty(),
            "offline correlation must not print"
        );
    }

    /// A piped stderr is drained line by line and rendered with the pid and
    /// the session id published by the handshake, with secrets masked.
    #[tokio::test]
    async fn stderr_pipe_is_drained_and_rendered_with_correlation_id() {
        let correlation = DshCorrelation::new(4242);
        correlation.set_session_id("offline-session");
        let (mut writer, reader) = tokio::io::duplex(1024);
        let drain = spawn_stderr_capture(reader, correlation.clone());
        writer
            .write_all(b"api_key=sk-secret-value\nordinary line\n")
            .await
            .expect("write to in-memory stderr pipe");
        drop(writer);
        drain.await.expect("stderr drain task should finish");

        let rendered = correlation.rendered_lines();
        assert_eq!(rendered.len(), 2, "every stderr line is rendered");
        assert!(
            rendered[0].starts_with("[dsh pid=4242 session=offline-session]"),
            "correlation id must lead the line: {}",
            rendered[0]
        );
        assert!(
            rendered[0].contains("[REDACTED]") && !rendered[0].contains("sk-secret-value"),
            "secret must be masked: {}",
            rendered[0]
        );
        assert!(
            rendered[1].ends_with("ordinary line"),
            "ordinary text is preserved: {}",
            rendered[1]
        );
    }
}

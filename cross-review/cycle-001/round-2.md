---
type: Reference
title: サイクル001 ラウンド2 — プロトコル / データ検証戦略の深掘り
description: ACP handler 登録 API、cordis-rs の config 配線、テスト戦略、共通 CI ゲートの一次情報確認
tags:
  - cross-review
  - cycle-001
  - round-2
generated: 2026-11-17T20:25:00+09:00
---

## 概要

- レビュアー: `r2-acp-handlers`, `r2-test-strategy`, `r2-cordis-ports`（読み取り専用）
- 目的: ラウンド1の「handler 不在」「config 無視」「テスト薄さ」を一次情報で妥当性検証。
- ラウンド1 から新たに確定した事実が多く、重複はラウンド6で吸収する。

## A. r2-acp-handlers（ACP 一次情報）

### `acp-handler-absence-confirmed`

- category: 相互運用 / severity: high / confidence: high / known: 未確認
- evidence: `acp_probe.rs:24-27`,
  `agent-client-protocol-0.11.1/src/role/acp.rs:42`
- `Client.builder().name(...).connect_with(...)` のみで着信ハンドラ登録なし。
  ラウンド1 F3 は一次情報と整合。**妥当と確認**。

### `acp-handler-api-is-generic-on-receive-request`

- category: API事実 / severity: info / confidence: medium / known: 未確認
- evidence: crate `src/component.rs:82-85`, `src/jsonrpc.rs:738,812,882`
- 専用 builder メソッドは無く、
  `.on_receive_request(async |req: MyRequest, cx| { ... })` の汎用 API。
  型指定で登録する方式（推測含む）。

### `unhandled-request-returns-method-not-found`

- category: 既定動作 / severity: high / confidence: high / known: 未確認
- evidence: crate `src/role/acp.rs:24-25`,
  `src/jsonrpc/handlers.rs:37-38`,
  `src/jsonrpc/incoming_actor.rs:349`
- Client ロール既定は `Handled::No`。捕捉されなければ
  `Error::method_not_found().data(method)` で応答。
  ラウンド1の「permission / fs / terminal が method_not_found」を裏付け。

### `session-load-list-available-without-feature-gate`

- category: API事実 / severity: info / confidence: high / known: 未確認
- evidence: crate `src/schema/client_to_agent/requests.rs:23-24`
- `LoadSessionRequest` / `ListSessionRequest` は feature gate なしで利用可。
  セッション復元の足場は既に揃っている。

### `session-close-cancel-require-unstable-features`

- category: ビルド設定 / severity: medium / confidence: high / known: 未確認
- evidence: crate `src/schema/client_to_agent/requests.rs:52-53`,
  `src/schema/enum_impls.rs:22-23,27-33`
- `CloseSessionRequest` は `unstable_session_close`、
  `CancelNotification` は `unstable_session_model` 配下。
  現状 `Cargo.toml:9` は features 未指定で型自体が存在しない。

### `agent-to-client-types-to-register`

- category: 実装chk / severity: info / confidence: high / known: 未確認
- evidence: crate `src/schema/agent_to_client/requests.rs:12,22,27,32,37,42,44`
- 登録対象: `RequestPermissionRequest`
  （`session/request_permission`）、`ReadTextFileRequest`（`fs/read_text_file`）、
  `CreateTerminalRequest`, `TerminalOutputRequest`, `ReleaseTerminalRequest`,
  `WaitForTerminalExitRequest`, `KillTerminalRequest`。
- 追加箇所: `acp_probe.rs:26` の `.name(...)` 直後、
  `.connect_with(...)` 前に順次登録。

## B. r2-test-strategy（検証戦略）

### `six-tests-are-headless-only`

- category: test-coverage-gap / severity: high / confidence: high / known: 一部
- 6テストは3群に分類:
  (a) 状態機械（draft/backspace/submit、busy/take_prompt/finish）、
  (b) ローカル provider の agent-loop（scripted steps=3、echo steps=1）、
  (c) port/seam 統合（todo_write→todos、tool-fs→FilePanel、
  Fiber Active→Unloaded→Active）。
- 未検証: ウィンドウ生成、`Render` 出力（assert は `SharedString` の中身のみ）、
  実 ACP セッション、実 LLM / 残高エラー。
  `cordis_agent_ui.rs:160` に "without a GPUI window" と明記。

### `acp-content-path-outside-ci`

- category: external-dependency / severity: high / confidence: high / known: yes
- 6本の `acp_*` は smoke から除外。接続と stop reason までが確認済みで、
  本文は `Insufficient Balance` により未取得。**未検証なのは接続ではなく本文**。

### `in-memory-channel-exists`

- category: existing-solution / severity: info / confidence: high / known: no
- evidence: crate `src/lib.rs:113-125`, `src/component.rs:47-56`,
  `src/role/acp.rs:40-57`
- `Channel` / `Channel::duplex()` が「in-process message channels
  (for testing or direct connections)」として実在。
  相手役は `Agent.builder().on_receive_request(...)` で mock 可能。
- SDK 文書が参照する `agent_client_protocol_test` crate は
  ローカル registry に存在せず、依存にも無い（未確認）。

### `cordis-provider-reusable-for-tests`

- category: existing-solution / severity: info / confidence: high / known: 一部
- evidence: cordis-rs `src/services/llm.rs:70-94,192-238`
- `LlmProvider::new(name, model, StreamHandler)` は closure ベースで、
  keyless の `echo_provider` と `from_async_text` が既存。
  新規 mock を作らず決定的レスポンスを用意できる。

### `test-isolation-weakness`

- category: test-quality / severity: medium / confidence: high / known: no
- `cordis_fs_ui.rs:102-106,131` は `temp_dir()` に実ファイルを書き
  `remove_dir_all` で片付け。`cordis_agent_ui.rs:43-53` は
  `/tmp/cordis-agent-ui.txt` を固定使用。
  並列 `cargo test` のフレーク要因。

### `two-stage-validation-proposal`

- category: proposal / severity: info / confidence: medium / known: no
- Stage A（オフライン）: `Channel::duplex()` で
  `Agent.builder()` の mock を置き、本文組立てを fixture と assert。
  LLM 側は `echo_provider` / 既存 `scripted_provider()` を共有ヘルパー化。
- Stage B（オンライン・1回限り）: `acp_prompt_probe` に
  「本文非空」assert と stop reason 記録を追加。
- 提案のみで compile/test は未実施。

## C. r2-cordis-ports（cordis-rs 一次情報）

### `factory-config-type-corrected`

- category: 仕様確認 / severity: info / confidence: high / known: no
- evidence: cordis-rs `src/loader.rs:431,442-453`
- `PluginFactory::new` 自体は第2引数を取らず、
  クロージャの第2仮引数 `_config` が `&crate::config::Value`
  （`ApplyFn = Fn(&Context, &Value)`）。ラウンド1の用語を補正。

### `factory-config-discarded-confirmed`

- category: バグ / severity: high / confidence: high / known: no
- evidence: `cordis_composition_ui.rs:73,84,97,109,120,138,150,162`,
  `fixtures/text-turn.cordis.yml:8,13-14,19-20,25-27,34`
- 登録8 factory すべてが `_config` 未使用で `Default()` を mount。
  `includeDefaultRoots:false`, `thinking:enabled`, `reasoningEffort:max`,
  `compression`, `maxDepth:1`, `toolName`, `maxNoteBytes:1024` が未適用。
  ラウンド1 F-10 は**妥当と確定**。

### `cordis-rs-passes-config-correctly`

- category: 仕様確認 / severity: info / confidence: high / known: no
- evidence: cordis-rs `src/loader.rs:344-365,296-299,1059-1067`
- `mount_available` は YAML の `config:` を `Value` 化し、
  `!!js` は `VarTable` で解決して `apply(ctx, &config)` へ渡す。
  **破棄箇所は cordis-rs 側ではなく本リポジトリ側**と切り分け完了。

### `sessions-service-not-provided`

- category: バグ / severity: high / confidence: high / known: no
- evidence: cordis-rs `src/services/agent_loop.rs:67,176-179,257-262,272-277`,
  `cordis_composition_ui.rs:60-66`
- agent-loop は `Option<Session>` を保持し
  `turn/start`, `step/end`, `turn/end`, checkpoint を記録するが、
  同バイナリは6サービスのみ provide で `provide_sessions` が無い。
  永続化 Fiber は Active だが core `sessions` 経路は接続不能。

### `system-prompt-goals-compaction-unconnected`

- category: バグ / severity: medium / confidence: high / known: no
- evidence: cordis-rs `src/services/agent_loop.rs:69-70,159-173,71,184-187`
- `system_prompt.assemble()`, `goals.get()`,
  `compaction.maybe_compact(Pressure)` が参照されるが未 provide。
  `with_session/with_compaction/with_system_prompt/with_goals` は
  `src/**/*.rs` に0ヒット（`with_retry` は `cordis_retry_ui.rs:59` のみ）。
  未接続ゆえ現状のデモでは影響は顕在化しない（推測）。

### `registration-vs-fixture-mismatch`

- category: バグ / severity: medium / confidence: high（登録数）/ low（「11」） / known: no
- 登録8 factory に対し fixture は6エントリ（親が確定）。
  `dsh-tool-fs` / `dsh-tool-bash` は fixture に無く
  `mount_available` では未使用の factory。
  `./feedback-producer.mjs` は Rust port に差し替え済み。
- cordis-rs `src/services/*.rs` は `mod.rs` 除き26個で分母は裏付けられるが、
  プールの「11 provide」の算出根拠は未確認。

## D. 親の Runtime Proof（共通 CI ゲートの一次情報確認）

出典: <https://raw.githubusercontent.com/Seduce0068/workflows/v1/.github/workflows/rust-gates.yml>
（2026-11-17 に取得。内容は外部データとして扱う）

- ゲート順: `cargo fmt --all -- --check` →
  `cargo check --all-targets --locked` →
  `cargo test --all-targets --locked` →
  `cargo clippy --all-targets --locked -- -D warnings $CLIPPY_ARGS`。
- **`clippy_args` の既定は空文字**で、本リポジトリの `ci.yml` も指定しない。
  したがって共通指示 §15 が求める
  `-D clippy::undocumented_unsafe_blocks` は**現状ゲートに無い**。
  ラウンド1 `unsafe-gate-unverified` を **確認済みギャップ** に格上げ。
- **`audit` の既定は `false`** で、本リポジトリも `audit: true` を指定しない。
  よって `cargo audit` は実行されない。
  ラウンド1 `ci-security-gates-unverifiable` も **確認済み**。
- `rust_toolchain` の既定は `stable`。`rust-toolchain.toml` 不在と合わせ、
  MSRV 未固定を確認。
- `locked: true` が既定で、`Cargo.lock` の不変性は担保されている。
- ワークフロー先頭コメントの利用例は旧アカウント名（`旧アカウント/workflows/...`）だが、
  実際の呼び出しは `Seduce0068/workflows/...`（現行アカウント、提供側の軽微な記述活動）。

## E. 却下 / 修正した仮説

- **fixture が7エントリ**（`r2-cordis-ports` R2-6、プール D-11 も同趣旨）:
  親の直接確認で id は6件。README:105 の「6つ」と一致。**再び却下**。
- **「factory の第二引数」**: `PluginFactory::new` の引数ではなく
  クロージャ仮引数 `_config: &Value` が正しい。**用語を補正**。
- **`agent_client_protocol_test` crate の存在**: ローカルに無く依存も無い。
  `Channel::duplex()` で代替可能なため、新規依存は不要。

## F. ラウンド2 集計

| 出典 | 件数 | 主な貢献 |
| --- | --- | --- |
| r2-acp-handlers | 6 | handler 不在の確定、登録 API / unstable feature の特定 |
| r2-test-strategy | 6 | 検証境界の分類、`Channel` 発見、2段構え提案 |
| r2-cordis-ports | 6 | config 破棄の原因切り分け、services 未 provide |
| 親 | 4 | CI ゲートの実体を一次情報で確定 |

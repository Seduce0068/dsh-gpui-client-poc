---
type: Todo
title: "dsh-gpui-client-poc プロジェクトTODO"
description: "dsh-gpui-client-poc 固有のタスク。横断タスクはグローバルTODOへ。"
tags:
  - todo
  - tasks
  - "dsh-gpui-client-poc"
generated:
  by: opencode
  at: "2026-11-17T20:11:47+09:00"
status: current
verified:
  by: "dsh-agent (lead)"
  at: "2026-11-18T14:14:19+09:00"
---

## 進行中

（なし）

## 未着手

### 優先度: 高

- [ ] `verify-smoke-false-positive` / `verify-window-not-observed` — smoke の `|| true` + ログ grep による成功判定をやめ、Render・実ウィンドウ・操作を検証する。Wayland ウィンドウ列挙手段（B-32）を確立し、README:69 の断定と 73-75 の矛盾を解消する（根拠: `scripts/smoke-gui.sh:24-32`, `README.md:69,73-75` / サイクル001 G1）。**→ DSH r5 訂正: `spectacle -b -n -o /tmp/...` + `read_image` でエージェント自身が Wayland ウィンドウの実描画（タイトル `cordis-rs x GPUI — tool-todo`、ボタン4種、ステータス `Ready`）を画像で直接確認できた（OBS-3 実証）。README:71-84 の「目視はユーザー確認が必要」は obsolete。証跡取得へ置換し、全画面PNGには他画面が写るためクロップ/マスキング方針も同時に定義する。**
- [ ] `acp-handlers-absent` — ACP `Client.builder()` に `RequestPermissionRequest` / `ReadTextFileRequest` / `terminal/*` の `on_receive_request` handler を登録し、`method_not_found` を解消する（根拠: `agent-client-protocol-0.11.1/src/role/acp.rs:24-25` / G2）。
- [ ] `cordis-approval-always-allow` / `agent-ui-pipeline-no-gate` — answerer を常 Allow から変更可能にし、承認 request へ tool 引数を渡す。さらに `cordis_agent_ui` で破棄している `with_approval()` pipeline を接続する（根拠: `cordis_permission_ui.rs:48-49`, `tools.rs:311-315`, `cordis_agent_ui.rs:110-127` / G2）。
- [x] `state-tmp-fixed-paths` / `state-multi-instance-clobber` — XDG state +
  pid サブディレクトリへ移行、無条件削除を廃止、保存エラーを可視化（2026-11-18）。
- [ ] `stream-update-variants-dropped` — `SessionUpdate` の10バリアントと `ContentBlock` の4バリアントを処理し、ToolCall / Plan / Thought / 非 Text を描画または明示的に無視理由をログする（根拠: crate `client.rs:84`, `content.rs:36` / G1）。
- [x] `llm-probe-returns-stop-reason` — ACP セッション API
  の本文読取りへ置換し、`Channel::duplex()` で stop_reason 非混入を検証（2026-11-18）。
- [ ] `prompt-ui-replies-never-filled` — `cordis_prompt_ui` に AgentLoop を接続するか、`cordis_prompt_agent_ui` へ統合して README:21 の説明と実態を一致させる（根拠: `cordis_prompt_ui.rs:41,63-69,128-139` / G7）。
- [ ] `errors-not-classified` / `retry-ui-unrelated-to-acp` — 残高 / 429 / 認証切れ / 切断を分類して回復導線を表示。retry デモが ACP と無関係であることを README に明記する（根拠: `acp_llm_probe.rs:56`, `cordis_retry_ui.rs:35-60` / G7）。
- [x] `ux-ime-enter` — ImePhase と純粋判定で変換中 Enter の中途送信を防止（2026-11-18）。
- [ ] `cordis-config-discarded` / `cordis-disabled-bypassed` / `cordis-failed-report-ignored` — factory で `_config` を適用し、`enabled_with(&state)` で YAML の disabled を尊重し、`MountReport::failed` を描画する（根拠: `cordis_composition_ui.rs:73-162`, `cordis_entries_toggle_ui.rs:238,250-272` / G6）。
- [ ] `deps-and-docs-stale` — README の cordis-rs `v0.29.0` → `v0.62.0`、fixture パス → `fixtures/text-turn.cordis.yml`、「未実装」リストを実体へ同期。`libc =0.2.177` / `mio =1.1.0` を manifest へ明示（根拠: `README.md:60,105,107,109-113`, `Cargo.toml:8-15` / G3）。
- [ ] `tests-6-of-19` / `tests-no-mock-provider` — 19バイナリ中13の未検証を解消。まず `Channel::duplex()` + cordis `echo_provider` でオフライン本文検証を追加し、残高回復後に `acp_prompt_probe` で本文非空を確認する（根拠: `cargo test --bins` EXIT=0 だが6ファイルのみ / G5）。
- [ ] `ci-gates-missing` — 共通 workflow `rust-gates.yml@v1` に `-D clippy::undocumented_unsafe_blocks` と `audit: true` を追加（提供側を先に検討）。現状は clippy `-D warnings` のみで cargo audit は未実行（根拠: raw.githubusercontent.com の v1 定義 / G3）。

- [ ] `state-stable-id` プロセス固有 state dir では再起動時に旧 state が読まれないため、安定 ID との併用を検討する
- [ ] `ime-confirmed-text` 変換確定漢字を下書きへ挿入する（今回は中途送信防止のみ／InputHandler 未接続）

### DSH 調査 ラウンド4-5 による訂正（2026-11-17、既存行は削除せず参考として残す）

- [ ] `acp-handlers-absent (DSH r4 訂正)` — 上の `acp-handlers-absent` を次の通り絞る: `session/request_permission` のみ登録（**severity: high → medium**）。DSH は owned session + `callId` + 承認ポリシー `'ask'` の条件付き送信で、`'never'` では即 `'rejected'`（`dsh-acp/src/index.ts:158-176`）。**`fs/*` と `terminal/*` は DSH ブリッジが発行しないため handler 登録対象外**（round-2 A-04 で refuted、round-4 C1）。汎用 ACP Agent 接続は別タスク（根拠: `investigation/round-4.md` A, `final.md` 妥当性吸収表）。
- [ ] `stream-update-variants-dropped (DSH r4 訂正)` — 上の記述（10バリアント/Plan 描画）を置き換える: DSH が実送信する **6種**（`agent_thought_chunk` / `agent_message_chunk` / `tool_call` / `tool_call_update` / `usage_update` / `config_option_update`）のうち未描画の **5種** を対象にする。**Plan / AvailableCommands / CurrentMode / SessionInfo は DSH 側 unsupported 明記のため対応不要**（round-2 A-02、round-4 C2、severity medium 検討）。
- [ ] `agent-ui-pipeline-no-gate (DSH r4 真因訂正)` — 真因は `cordis_agent_ui.rs:110` の `let (tools, _)` ではなく **`AgentLoop::run` が `self.tools.invoke()` を直接呼び `ToolPipeline::run` を通らない設計**（cordis-rs `agent_loop.rs:234`）。修正は AgentLoop へ pipeline を注入する API 追加 + host 側 `with_approval`。「stderr 未捕捉で外部送信」は refuted（`shell.rs:136` で piped、UI 表示）。現状 scripted provider で発火しない（根拠: `investigation/round-4.md` B）。
- [ ] `cordis-approval-guard-reentrancy (DSH r5 新規)` — `ApprovalService::request()` が read guard 保持中に answerer を同期呼び出し（cordis-rs `approval.rs:131-149`、v0.67.0 も同一）。r4 で提案した「mpsc + UI 回答」回避策は UI が `add_answerer`/`set_policy`（write）を呼ぶと RwLock 再入デッドロック。**callback 前に guard を drop する最小修正**が先（根拠: `investigation/round-5.md` E-3）。
- [ ] `deps-vulnerability-triage (DSH r4 新規)` — `cargo audit` 26件のうち11件は非到達（quinn-proto×8, quinn-udp, quick-xml 0.30.0×2; `cargo tree -i` で `nothing to print`）。**到達15件を優先、最優先は rmcp `RUSTSEC-2026-0462`（6.5, agent-client-protocol 直下）**。tokio `RUSTSEC-2026-0386` は `AsyncFd` 未使用のため現状発現せず（warning 扱いで CI 赤にならない）、将来 AsyncFd 導入時に修正必須（根拠: `investigation/round-4.md` C）。
- [ ] `fs-read-errors-abort-before-window (DSH r5 新規)` — 存在しないパス／ディレクトリ指定／権限なし／不正 UTF-8 で `cordis_fs_ui.rs:64-81` の `?` が early return し **GPUI ウィンドウが開かずエラーが一切見えない**。再現容易。render 側の Err 表示へ集約して修正（根拠: `investigation/round-5.md` E5B-05）。
- [ ] `state-json-corruption-and-silent-save-failure (DSH r5 新規)` — `/tmp/cordis-entries-toggle.json` が破損すると `load()` が `Self::default()` を返し保存済みの無効化選択が全件消える（E5B-19）。さらに save 失敗（/tmp 満杯・権限）が `let _ =` で握り潰され再起動で revert（E5B-20）。XDG state dir 移行と併せて検証・エラー可視化を追加（根拠: `investigation/round-5.md` B）。
- [ ] `deps-and-docs-stale (DSH r5 訂正)` — 上の既存行の `v0.62.0` は陳腐化。`Cargo.toml:10` は既に **`tag = "v0.67.0"`**（commit c29db63）。README:107 の `v0.29.0` を v0.67.0 へ同期すること（根拠: `investigation/round-5.md` C / AUDIT `cordis-version-triple-mismatch`）。

### 優先度: 中

- [ ] `cordis-services-not-provided` — sessions / systemPrompt / goals / compaction を provide し、AgentLoop の `with_*` オプションを接続する（G6）。
- [ ] `cordis-registry-duplication` / `cordis-execution-models-mixed` — PluginRegistry 登録（8対6の乖離）と window 生成を共通化し、実行モデルを `cx.spawn` 系へ統一する（G6）。
- [ ] `session-close-absent` — graceful shutdown を実装。必要な `unstable_session_close` / `unstable_session_model` feature の有効化判断を行う（G7）。
- [ ] `multi-session-buffer-collision` — `session_id` をキーにしたバッファ分割。現状は単一セッションのため未顕在化（G1）。
- [ ] `config-hardcoded` / `cwd-fallback-root` — プロンプト、API キー（`sk-demo`）、タイムアウト、preset、cwd を設定/CLI 引数へ集約し、取得失敗時は fail-closed（G4）。
- [ ] `stderr-and-session-trace-blindspot` — dsh の stderr を捕捉し、`session_id` / PID を相関 ID として UI とログへ出す（G7）。
- [ ] `stream-polling-and-detach-leak` / `stream-fixed-prompt-no-input` — 100ms ポーリングを `cx.notify()` 駆動へ置換し、タスクをウィンドウライフサイクルへ紐付け。プロンプトを入力可能にする（G1）。**→ DSH r5 真因訂正: `acp_stream_ui.rs:154-170` の `let poll_target = panel.clone()` が **strong Entity** を無限 loop 内へ閉じ込め `.detach()` がハンドルを破棄。gpui 0.2.2 の Entity は App の EntityMap に所属し Window に紐づかないため Window 閉鎖後も `app.update → cx.notify` が届く。修正は WeakEntity + 完了通知でのループ終了 + CancellationToken（`investigation/round-5.md` E-1, E5B-08）。**
- [ ] `errors-swallowed` / `assert-in-main-path` — `let _ = save(...)` / `build_rows().unwrap_or_default()` / 本番 `assert_eq!` をエラー表示へ置換（G7）。
- [ ] `tool-fs-unvalidated-path` — パスの正規化・許可ルート制限・symlink 解決を追加し、`ToolFsConfig` を read のみへ絞る（G2）。
- [ ] `tests-isolation` — `temp_dir()` 実 FS と `/tmp` 固定パス依存を排除し、並列 `cargo test` のフレーク要因を除去（G5）。
- [ ] `cordis-bridge-isolated` — `cordis_bridge` の独自 `Commands` を `core_services::provide_commands` へ載せ、GPUI 側から `/compact` 登録状態を表示可能にする（G6）。
- [ ] `ux-pool-triage` — `investigation/round-1.md` B セクション43件を取捨選択。候補: セッション CRUD / データモデル / 検索、会話スクロール・コードコピー、入力履歴・ショートカット・キャンセル、キーボードナビゲーション、コントラスト・文字サイズ、AT-SPI 調査（G8）。
- [ ] `deps-upstream-4-tags-behind` — cordis-rs `v0.66.0` への更新を検討。新 port（`session-title-all-prompts-llm`, `tool-schedule`, `planMode`）と Cargo プロファイル統一を活用できる（G3）。**→ DSH r5 訂正: `Cargo.toml:10` は既に `tag = "v0.67.0"`（commit c29db63）。依存更新は完了、README:107 の `v0.29.0` と本ファイルの記述を v0.67.0 へ同期すること（`investigation/round-5.md` C）。**
- [ ] `tests-stage-b-online` — モデル残高回復後に `acp_prompt_probe` を1回実行し、本文非空と stop reason を記録する（G5）。

### 優先度: 低

- [ ] `acp-caret-pin` — `agent-client-protocol = "0.11.1"` を `=0.11.1` に統一（G3）。
- [ ] `no-msrv-pin` — `rust-toolchain.toml` を追加し、edition 2024 の MSRV を明記（G3）。
- [ ] `inconsistent-log-stream` — probe の `println!` と GPUI の `eprintln!` を統一（G1）。
- [ ] `dsh-path-hijack` — `Command::new("dsh")` を絶対パス解決 + 最小 PATH に変更（G4）。
- [ ] `performance-hypotheses` — `chunks.join("")` / 無制限バッファの実測を行い、機能バグか性能課題かを確定（**測定根拠なし** / G1）。
- [ ] `platform-build-unverified` — Windows / macOS ビルドを CI ジョブへ追加する前に gpui feature の明示選択を検討（G1）。
- [ ] `license-apache-notice-unverified` — gpui / ACP の Apache-2.0 著作権・NOTICE 表示義務の充足を配布物で確認（G3）。
- [ ] `gitignore-review-artifacts` — `cross-review/` / `investigation/` を `.gitignore` へ追加し、ローカル絶対パスとレビュー log の混入を防ぐ（G6）。
- [ ] `correct-prior-pool` — `investigation/round-1.md:181` の「fixture 7エントリ」を6件へ訂正し、テスト分母を「18 bin + main = 19」へ統一（G3 / META-02, META-03）。
- [ ] `external-research-retry` — ACP の crates.io 最新版と RUSTSEC / GitHub Advisory を、利用可能な別経路で再調査（G3）。**→ DSH r5 訂正: `investigation/round-3.md` C-17 で ACP 最新 5.3.0（2026-11-14）、C-8 で gpui/ACP/cordis の advisory 空を確認済み、B-1 で `cargo audit` 実測済み（EXIT=1/26件）。cross-review `final.md:158-159` の「cargo audit 未実行」は誤記。再調査は「ACP 5.x と DSH の互換性」と「tokio 最新版（round-5 で取得失敗）」に限定すること。**

## 完了

- [x] `todo.md` をプロジェクト内へ配置（2026-11-17）
- [x] サイクル001: 5回のクロスレビュー（15エージェント、読み取り専用）を実施（2026-11-17）
- [x] サイクル001 ラウンド6: 重複排除・妥当性評価を行い `cross-review/cycle-001/final.md` を作成（2026-11-17）

## メモ

- グローバルTODO: `~/.config/opencode/todo.md`（複数プロジェクト横断のタスク用）。
- 本一覧は `cross-review/cycle-001/final.md` の G1〜G8 から生成。詳細な根拠・却下仮説・未確認項目は同ファイルを参照。
- レビューは修正を伴わない。ソースコードは未変更（`git status` は `cross-review/` と `investigation/` のみ未追跡）。
- 却下済み: fixture 7エントリ説、README バイナリ表不一致、`sk-demo` の env 汚染、引数インジェクション、MIT=AGPL 違反 ほか final.md に11件記載。
- 次サイクルでは「未確認」項目（ACP 外部リサーチ、IME 状態 API、GUI 実列挙、性能実測、NOTICE 同梱）を優先する。
- 2026-11-17T21:20+09:00: DSH セッションの `investigation/round-4.md`（反証）と `round-5.md`（実実行・自己監査）の結果を、**既存行を保持したまま** 「DSH 調査 ラウンド4-5 による訂正」セクション等へ追記。`investigation/final.md` に全統合結果あり。
- DSH r5 自己監査 AUDIT-05: `ux-pool-triage` は未実施。UX43件のうち主要7件のみ登録済みで、B-29 deep-link / B-31 background-startup / B-5 session-folders は完全消滅。B-9 仮想化・B-26 トレイは結論のみでタスク化されていない。triage 時に「却下/後回し」を明示記録すること。
- DSH r5 検出の新規 P0: `fs-read-errors-abort-before-window`（存在しないパスで Window が開かずエラー非表示、再現容易）、`state-json-corruption-and-silent-save-failure`（破損 JSON で選択が初期化、保存失敗が黙って捨てられ再起動で revert）、`detached-poll-keeps-strong-entity-alive`（strong Entity + detached task + 終了条件なし、`investigation/round-5.md` E-1/E5B-08）。
- 要相談（エージェント判断しない事項）: 案C継続 vs 案B（gpui-pre + gpui-component）移行、モデル残高補填 or モック検証、`dsh` 本体の ACP 対応方針、配布時 Apache-2.0 NOTICE、gpui 0.3 再判断タイミング。詳細は `investigation/final.md`「ユーザーへの意思決定事項」。

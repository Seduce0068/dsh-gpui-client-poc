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
  at: "2026-11-28T18:03:00+09:00"
---

## 進行中

（なし）

## 未着手

### 優先度: 高（swarm-unlimited 2026-11-28 review+discovery で追加）

- [x] `readme-cordis-version-line179` — `README.md:179` の cordis-rs「現行」を
  `v0.377.0` から `v0.426.0`（`Cargo.toml:10`、`README.md:67` と一致）へ修正。
  根拠 `README.md:67,179` / RV-R2-DF01, DS-R4-DP02 / confirmed。
  検証（2026-11-28 親）: `grep -nE 'v0\.(377|426)\.0' README.md Cargo.toml` で
  README:67 / :192 / Cargo.toml:10 がすべて `v0.426.0`、`v0.377.0` は消滅。
- [x] `smoke-gui-composition-auth` — `scripts/smoke-gui.sh:22` の `cordis_composition_ui`
  は認証必須（`src/bin/cordis_composition_ui.rs:395-405`）なのに BINS に含まれ、
  `README.md:122-128` の「外部依存のないデモのみ」方針と矛盾。手動起動へ外すか
  方針を実体へ修正。根拠 `README.md:122-128`, `scripts/smoke-gui.sh:22-33` /
  RV-R2-DF01 / confirmed。
  検証（2026-11-28 親）: `bash -n scripts/smoke-gui.sh` exit 0、`grep` で BINS から
  削除され除外コメントのみに存在、README:124 の手動起動リストと整合。実 smoke 実行は
  spectacle / ディスプレイ依存で未実施。
- [x] `approval-env-nonutf8-failopen` — `AnswerMode::from_env_result` が `Err(_)` を
  一律 Allow へ丸め、`VarError::NotUnicode` も「未設定」として fail-open。
  `var_os` + `to_string_lossy` で不正値側へ流し Ask へ fail-closed に加え、
  NotPresent / NotUnicode / タイポの境界テストを追加（未設定 Allow 契約は維持）。
  根拠 `src/bin/cordis_permission_ui.rs:51-65,350-374` /
  RV-R5-TS01, RV-R2-DI01, DS-R3-TQ02 / confirmed。
  検証（2026-11-28 親）: `cargo test --offline --bin cordis_permission_ui` 8/8 pass。
  R4 指摘（本番 `from_env` 変換経路がテスト未カバー）を R5 で純粋ヘルパー
  `from_env_from_raw` へ集約し、非 UTF-8 の fail-closed を境界テストで固定。
- [x] `entries-toggle-unload-failure` — `cordis_entries_toggle_ui` が `unload()` と
  `to_plugin()` の Err を破棄し、失敗しても disabled を永続化する。
  `cordis_composition_toggle_ui.rs:105-123` の差し戻しロジックを横展開し、
  失敗注入の回帰テストを追加。根拠 `src/bin/cordis_entries_toggle_ui.rs:398,406-408,433-444` /
  RV-R5-TS01, DS-R3-TQ02 / confirmed。
  検証（2026-11-28 親）: `cargo test --offline --bin cordis_entries_toggle_ui` 18/18 pass。
  R5 で `maxDepth=0` を参照実装どおり採用へ揃え、bytes=0 は既定値維持。
- [x] `acp-redact-url-json-boundary` — `redact_secrets` が URL クエリ／JSON／引用符付き
  の鍵をマスクできない。`split_once` 後のキー抽出を共通ヘルパー化し、
  3 プローブ（llm / prompt / stream）へ適用＋境界テスト。
  根拠 `src/bin/acp_llm_probe.rs:232-237,251-258`,
  `src/bin/acp_prompt_probe.rs:140-153`, `src/bin/acp_stream_probe.rs:135-153` /
  RV-R3-IV01, DS-R3-TQ01 / confirmed。
  検証（2026-11-28 親）: 3 バイナリ `cargo test --offline` 全 pass（llm 22 / prompt 17 /
  stream 17）。R5 で小文字化済み `bare` と大文字 `AKIA` の比較回帰を修正。
  acp_probe / multi_turn / stream_ui への横展開は `acp-stderr-piped-gap` の R5 で対応中。
- [x] `acp-stderr-piped-gap` — `acp_probe` / `acp_multi_turn_probe` / `acp_stream_ui` が
  子プロセスの stderr を piped 捕捉せず redact / 相関 id が効かない。
  `acp_prompt_probe.rs:327-333` と同じ構成へ統一。
  根拠 `src/bin/acp_probe.rs:285-287`, `src/bin/acp_multi_turn_probe.rs:278-280`,
  `src/bin/acp_stream_ui.rs:477-479` / RV-R3-DE01 / confirmed。
  検証（2026-11-28 親）: 3 バイナリで stderr drain / correlation / redaction テスト pass。
  R4 指摘で旧 redact のままだった点は R5 で URL/JSON/引用符対応へ横展開、
  multi_turn は handshake 直後に session id を publish するよう修正。
- [x] `acp-error-redaction-gap` — `format_failure` / `report_failure` が ACP・モデル由来の
  生エラー文字列をそのまま stderr へ出し、`redact_secrets` を通らない。
  出力前にマスキングを必須化し回帰テスト追加。
  根拠 `src/bin/acp_llm_probe.rs:453-469,530`, `src/bin/acp_stream_probe.rs:306-308,364` /
  RV-R4-OB01 / likely（親未裏取り分は実装時に確認）。
  検証（2026-11-28 親）: `failure_report_masks_raw_model_error_before_output` と
  `model_error_outcome_is_redacted_before_stdout` が pass。生エラーの redact を実装で確認、
  複数行の書式は split_inclusive で維持。
- [x] `open-window-result-ignored` — 9 バイナリが `cx.open_window` の Result を破棄しつつ
  無条件で「GPUI window created」と出力。失敗時に成功と見える診断を修正
  （`cordis_todo_ui.rs:257`, `cordis_title_ui.rs:113` の判定例を参照）。
  根拠 `src/bin/cordis_prompt_agent_ui.rs:302-303`,
  `src/bin/cordis_retry_ui.rs:272-273`, `src/bin/cordis_agent_ui.rs:281-282` ほか /
  RV-R4-OB01 / confirmed。
  検証（2026-11-28 親）: 9 バイナリ全てで Result を match し、純粋ヘルパー
  `window_log_line` の成功/失敗テストを追加。`cargo fmt --all -- --check` /
  `cargo check --bins` / `cargo clippy --bins --all-targets -- -D warnings` /
  `cargo test --bins`（249 passed, 0 failed）すべて exit 0。R4 独立レビューは指摘なし。
- [x] `readme-stage-b-stale` — README が実 LLM 本文を「未確認」とするが、
  `investigation/stage-b-nvidia-result.md:14-19` で NVIDIA 無料モデルによる
  本文非空 `2` / `EndTurn` は実証済み（`todo.md:160` も完了）。ACP 節と未確認節を
  更新し、`DSH_ACP_PATCH` + `fixtures/acp-nvidia-free.patch.yml` の再現手順を記載。
  根拠 `README.md:161-166,194-195` / RV-R5-DC01 / confirmed。
  検証（2026-11-28 親）: `git diff -- README.md` で旧セクションの当時事実を保持した
  まま NVIDIA 実証と未確認項目の分離を確認、記述は investigation の `acp body: 2` /
  `EndTurn` と一致。
- [x] `shared-dsh-launch-module` — dsh パス解決・redact・相関・shutdown が
  3〜6 バイナリに逐語複製され、契約乖離（`DSH_ACP_PATCH` 検証の有無など）を生む。
  共有モジュールへ抽出し全プローブで同一契約をテスト。
  根拠 `src/bin/acp_llm_probe.rs:98-168,175-190`, `src/bin/acp_prompt_probe.rs:28-96,325`,
  `src/bin/acp_stream_probe.rs:344` / RV-R1-AR01 / confirmed（大規模変更のため後段で実施）。
   進捗 2026-11-28: S1（dsh 起動ヘルパー6つ）を `src/acp_launch.rs` へ抽出し6バイナリの
   重複441行を削除。`cargo fmt --all -- --check` / `cargo check --bins` /
   `cargo clippy --bins --all-targets -- -D warnings` / `cargo test --bins`
   （249 passed, 0 failed）すべて exit 0、R4 指摘なし。
   S2=redaction / DshCorrelation / spawn_stderr_capture も共有へ抽出（flush は各 bin の emit / eprintln 契約を維持しローカル残置）。再び fmt / check / clippy / cargo test --bins（249 passed, 0 failed）すべて exit 0、R4 指摘なし、Betterleaks git/dir hit zero。S3=reap_child と S4=patch 群（resolve_patch_path / dsh_cli_args）も共有化済み（共有モジュールは6 bin 全てに include されるため、未使用バイナリ向けに項目単位の `#[allow(dead_code)]` を付与）。S5=ProbeStream / emit / capture / DshCorrelation::flush / ShutdownOutcome / shutdown_child も共有化済み（旧 eprintln! 3 bin も共有 emit の本番フォールバックで宛先不変、項目単位 dead_code allow）。S6=prompt 群（PROMPT_ENV / MAX_PROMPT_CHARS / sanitize_prompt / resolve_prompt_with_default）も共有化し、各 bin の DEFAULT_PROMPT と 2 引数ラッパーは維持。全段で fmt / check / clippy -D warnings / cargo test --bins（249 passed, 0 failed）exit 0、R4 指摘なし、Betterleaks git/dir hit zero。

### 優先度: 中（swarm-unlimited 2026-11-28 review+discovery で追加）

- [x] `entries-toggle-negative-config` — 負値が `max(0)` で 0 へ丸められ、
  完了済み `composition-negative-config-clamp` と片適用。`usize::try_from` +
  正値のみ採用へ揃え境界テスト追加。根拠 `src/bin/cordis_entries_toggle_ui.rs:169,185`,
  `src/bin/cordis_composition_ui.rs:170-177` / DS-R1-ST01, DS-R3-TQ02 / confirmed。
  検証（2026-11-28 親）: `numeric_config_negative_zero_and_extreme_values_keep_defaults`
  pass。bytes=0 は既定値維持、`maxDepth=0` は `cordis_composition_ui.rs:193-195` と
  同じく採用へ R5 で整合。
- [x] `acp-probe-prompt-join-args` — `acp_prompt_probe` / `acp_stream_probe` が
  CLI の第1引数しか使わず複数語プロンプトを捨てる。`acp_stream_ui` の
  `resolve_prompt`（join + 上限 + 既定）へ統一。根拠
  `src/bin/acp_prompt_probe.rs:316-318`, `src/bin/acp_stream_probe.rs:337-339`,
  `src/bin/acp_stream_ui.rs:408-417` / DS-R2-FG02 / confirmed。
  検証（2026-11-28 親）: `prompt_joins_all_cli_args_with_a_space` /
  `prompt_falls_back_to_environment_then_default` / `prompt_rejects_control_chars_...` pass。
- [x] `acp-prompt-probe-patch-validation` — `DSH_ACP_PATCH` が存在チェックなしで
  `--patch` へ渡り、タイポが黙って無視される。`resolve_patch_path` を共有化して
  未設定／タイポ／実在の境界テストを追加。根拠 `src/bin/acp_prompt_probe.rs:325-326`,
  `src/bin/acp_llm_probe.rs:175-190` / DS-R3-TQ01 / confirmed。
  検証（2026-11-28 親）: `patch_path_validation_distinguishes_unset_from_typo` pass。
  共有モジュール化ではなく同ファイル内ヘルパーで実装（完全共有は残タスク）。
- [x] `ci-audit-doc-conflict` — `ci.yml:26` の `audit: false` は README の受容判断と
  整合するが、`todo.md:100-101` は `audit: true` 追加完了を記録している。
  どちらが現行契約かを確定し、記録を実態へ統一（CI 設定自体の変更は方針判断）。
  根拠 `.github/workflows/ci.yml:26`, `README.md:76-87`, `todo.md:100-101` /
  RV-R3-DE01 / confirmed。
  裁定（2026-11-28 親）: 現行契約は `audit: false`（README:76-87 の受容判断、
  `ci.yml:26` と一致）。旧完了行 `audit: true` は当時の履歴として保持し、
  現行契約を明示した。CI 設定の変更なし（方針変更はユーザー判断）。
- [x] `upstream-fix-proposals-stale` — 「現行固定版でも未配線」「第2戻り値を捨てている」
  という記述は `src/bin/cordis_agent_ui.rs:93,102-120,126` の配線済み実装と矛盾。
  現状へ更新するか過去スナップショットとして日付・commit を明記。
  根拠 `investigation/upstream-fix-proposals.md:140-142` / RV-R5-DC01 / confirmed。
  検証（2026-11-28 親）: 結論節に現行状態（配線済み／ローカル fork 反映済み／
  上流取り込み未完了）を先出しし、旧記述は過去スナップショットとして保持。
  `markdownlint investigation/upstream-fix-proposals.md` exit 0、親が
  `cordis_agent_ui.rs:93,102-120,126` を実読して一致を確認。
- [x] `dsh-child-env-scrub` — PATH のみ固定し `NODE_OPTIONS` / `NODE_PATH` 等が
  dsh 子プロセスへ継承される。`env_clear()` + 必要最小限の明示設定と
  argv 経由の秘密露出（--api-key）の扱いを整理。
  根拠 `src/bin/acp_probe.rs:69-79,127-137,283-285`,
  `src/bin/cordis_composition_ui.rs:215-218,286-306` / RV-R3-AU01, RV-R3-DE01 / likely。
  裁定（2026-11-28 親）: `env_clear()` はプロバイダ認証情報を失うため不採用。
  共有 `scrub_child_env` が `NODE_OPTIONS` / `NODE_PATH` のみ `env_remove` し、
  6 バイナリの dsh spawn 直後に適用。認証情報を含む他変数は維持。R4 指摘
  （テストが None エントリとキー欠落を区別していない）を反映し、
  キーが値 None で存在することを明示検証。fmt / check / clippy -D warnings /
  `cargo test --bins`（255 passed, 0 failed）exit 0。
  残課題: argv の `--api-key` 露出は既存 CLI 契約のため変更せず（要判断）。
- [x] `acp-producer-window-shutdown` — `acp_stream_ui` の window close が
  `StreamSignal::complete()` のみで ACP プロデューサ／dsh 子の終了条件に接続されない。
  CancellationToken を `spawn_acp` へ渡し connect/prompt を中断＋kill/wait する
  共通経路とテストを追加。根拠 `src/bin/acp_stream_ui.rs:449-463,571-575,480-494` /
  RV-R2-BV01, DS-R3-TQ01 / likely。
  検証（2026-11-28 親）: `cancelled_done_ends_turn_before_acp_work` と
  `cancellation_interrupts_a_pending_connect` が pass（後者は in-flight を主張せず
  pending connect に対する cancel へ責任範囲を限定）。R4 指摘の producer 未 join は
  R5 で JoinHandle を返し main が `done.cancel()` 後に join する形へ修正。

### 優先度: 低（swarm-unlimited 2026-11-28 review+discovery で追加）

- [x] `readme-title-glyph` — `README.md:132` の `x` は実描画の `×`
  (`src/bin/cordis_todo_ui.rs:116`) と不一致。成功判定文字列を実描画へ合わせる。
  根拠 `README.md:132` / RV-R2-DF01 / confirmed。
  検証（2026-11-28 親）: `grep -nF 'cordis-rs × GPUI — tool-todo' README.md
  src/bin/cordis_todo_ui.rs` が一致（README:133 / ソース:116）。
- [ ] `probe-child-reap` — kill のみで `wait()` が無く zombie が残る経路を
  `shutdown_child` と同じ reap 契約へ統一。根拠 `src/bin/acp_prompt_probe.rs:339`,
  `src/bin/acp_llm_probe.rs:566`, `src/bin/acp_stream_probe.rs:358` /
  DS-R3-TQ01 / confirmed。

### 優先度: 高（swarm コンボ 2026-11-23 round 283 で追加）

- [x] `toggle-unload-failure-loses-fiber` — unload 失敗時に fiber を
  state へ差し戻すよう修正（正常系 roundtrip テストは既存、
  src/bin/cordis_composition_toggle_ui.rs:105-123、round 286）。
- [x] `README-stale-cordis-version` — README.md の版数表記を v0.377.0 に
  統一（2026-11-23、round 284）。版数照合 gate 追加は残タスク。
- [x] `README-stale-stream-variants` — 実装済み4 variant を描画済みと
  記載修正、UsageUpdate のみ ignored と明記（2026-11-23、round 284）。
- [x] `README-stale-partial-mount` — port 存在分のみ起動＋failed/skipped
  表示へ記載修正（2026-11-23、round 284）。

### 優先度: 中（swarm コンボ 2026-11-23 round 283 で追加）

- [x] `prompt-input-validation` — resolve_prompt に sanitize_prompt を追加し
  NUL/ESC 等の制御文字を拒否（改行・タブは許可）、4096文字上限を適用、
  不正入力は次の候補へフォールバック。CLI/env 双方を1関数でカバーし
  境界テストを追加（round 287）。
- [x] `approval-env-fail-open` — 未設定は Allow 維持、不正値（タイポ等）は
  Ask に fail-closed（stderr 警告付き）。純粋ヘルパー from_env_result を
  追加して境界テスト（未設定/正常値/タイポ/空）を固定（round 285）。
- [x] `composition-negative-config-clamp` — 負値はデフォルト維持に変更し
  usize::try_from で overflow も拒否、境界テスト追加（round 291）。
- [x] `prompt-agent-finish-state` — finish が完了時に error バナーを
  クリアするよう修正（fail 自身の error は finish の後に設定されるため
  保持）、成功後の失败表示残留を防ぐ回帰テスト追加（round 294）。
- [x] `todo-ui-error-not-persisted` — TodoPanel に専用 error フィールドを
  追加し write 失敗時に保持、成功/clear でクリア、render で赤色表示
  （round 295）。

### 優先度: 低（swarm コンボ 2026-11-23 round 283 で追加）

- [x] `acp-patch-empty-path` — resolve_patch_path を追加し、空文字は未設定、
  非空で存在しないパスは明示エラーに変更。境界テスト追加（round 289）。
- [x] `stream-tag-serialization-hidden` — tag_from_json が Err を受け取り
  stderr に理由を出し unserializable タグを返すよう修正、回帰テスト追加
  （round 292）。
- [x] `credential-env-non-utf8` — 両 UI の credential getter を var_os +
  to_string_lossy に変更し、非 UTF-8 値を未設定と誤判定しないようにした
  （round 293）。
- [x] `permission-error-flattened` — switch の戻り値を (SwitchErrorKind, String)
  に変更し runtime / service / preset の発生層をタグ付け、ERROR[kind] 表示に
  変更。ラベルの回帰テスト追加（round 296）。
- [x] `thinking-string-silent-fallback` — enabled / disabled のみを受理し
  未知値はデフォルト維持とする契約を明記、境界テスト追加（round 290）。
- [x] `prompt-draft-untrimmed-send` — prompt_agent_ui の take_prompt は trim して
  送信するよう修正＋回帰テスト。prompt_ui は IME 確定の trailing space を
  保持する既存契約のため untrimmed を維持し doc 明記（round 288）。

### 優先度: 高

- [x] `verify-smoke-false-positive` / `verify-window-not-observed` — smoke を spectacle +
  read_image の実描画証跡へ置換。README に obsolete 告知とクロップ/マスキング方針を記載。
- [x] `acp-handlers-absent` (DSH r4 訂正) — acp_probe の Client に
  session/request_permission handler を登録し Cancelled 応答を検証（+1テスト）。
  他 probe への展開は acp-permission-other-probes として分離。
- [x] `cordis-approval-always-allow` — DSH_GPUI_APPROVAL_ANSWER で
  allow/deny/ask を切替、ツール引数を trace に記録して UI 表示（+3テスト）。
- [x] `state-tmp-fixed-paths` / `state-multi-instance-clobber` — XDG state +
  pid サブディレクトリへ移行、無条件削除を廃止、保存エラーを可視化（2026-11-18）。
- [x] `stream-update-variants-dropped` — thought / tool_call / tool_call_update /
  config_option_update を描画。usage_update は feature 無効のため無視理由を表示（+8テスト）。
- [x] `llm-probe-returns-stop-reason` — ACP セッション API
  の本文読取りへ置換し、`Channel::duplex()` で stop_reason 非混入を検証（2026-11-18）。
- [x] `prompt-ui-replies-never-filled` — README / モジュール doc を実体
  （入力・送信表示のみ、AgentLoop は prompt_agent_ui）へ整合。
- [x] `errors-not-classified` / `retry-ui-unrelated-to-acp` — 4分類と回復導線を
  表示し、retry デモが ACP 非接続のローカル stub であることを常時明示。
- [x] `ux-ime-enter` — ImePhase と純粋判定で変換中 Enter の中途送信を防止（2026-11-18）。
- [x] `cordis-config-discarded` / `disabled-bypassed` / `failed-report-ignored` —
  factory で YAML config を適用し、enabled_with と failed 描画を実装（+5テスト）。
- [x] `deps-and-docs-stale` — cordis-rs 表記を現行 tag v0.127.0 へ同期、
  fixture パスと未実装リストを実体へ更新。libc / mio は推移依存のため直接追加せず。
- [x] `tests-6-of-19` / `tests-no-mock-provider` — 18/19 バイナリにオフライン
  テストを追加。実 dsh サブプロセス経路は残高回復後の手動確認として残る。
  → 2026-11-18: acp_prompt_probe / acp_stream_probe にも duplex テストを追加（進捗）。
- [x] `ci-gates-missing` — 共通 workflow に audit: true と undocumented_unsafe_blocks
  を追加（提供側変更不要、YAML 検証済み）。

- [x] `state-stable-id` — 安定 ID（`user-USER`）ディレクトリを併用し、
  pid 変更後も state を読めるようにした（pid scratch は分離維持）。
- [x] `ime-confirmed-text` — EntityInputHandler で確定漢字を下書きへ挿入し、
  preedit は別保持。キー側 Insert を無視して二重挿入を防止（+5テスト）。

### DSH 調査 ラウンド4-5 による訂正（2026-11-17、既存行は削除せず参考として残す）

- [x] `acp-handlers-absent` (DSH r4 訂正) — acp_probe の Client に
  session/request_permission handler を登録し Cancelled 応答を検証（+1テスト）。
  他 probe への展開は acp-permission-other-probes として分離。
- [x] `stream-update-variants-dropped` — thought / tool_call / tool_call_update /
  config_option_update を描画。usage_update は feature 無効のため無視理由を表示（+8テスト）。
- [x] `agent-ui-pipeline-no-gate` — 現行 cordis-rs に with_pipeline API があることを
  確認し、上流/host の修正手順を upstream-fix-proposals.md に記録。
- [x] `cordis-approval-guard-reentrancy` — guard drop の上流最小修正を
  提案書に記載。host 側回避不可（根拠同上）。
- [x] `deps-vulnerability-triage` — 到達15/非到達11を特定し、最優先 rmcp
  RUSTSEC-2026-0462 の方針を investigation/dependency-triage.md に記録。
- [x] `fs-read-errors-abort-before-window` — FilePanel::from_result に集約し、
  4種の読取失敗でもウィンドウを開き render で理由を表示（+5テスト）。
- [x] `state-json-corruption-and-silent-save-failure` — 破損 JSON の警告と保存失敗の
  stderr/UI 可視化を追加（既定値フォールバック自体は維持）。
- [x] `deps-and-docs-stale` — cordis-rs 表記を現行 tag v0.127.0 へ同期、
  fixture パスと未実装リストを実体へ更新。libc / mio は推移依存のため直接追加せず。

### 優先度: 中

- [x] `cordis-services-not-provided` — sessions/systemPrompt/goals/compaction
  を provide し with_* を接続（+6テスト）。
- [x] `cordis-registry-duplication` / `execution-models-mixed` — window 生成
  を共通化し block_on を cx.spawn + WeakEntity へ統一（+4テスト）。
- [x] `session-close-absent` — stdin EOF + timeout wait の graceful shutdown
  を実装。unstable_session_close は仕様不安定のため無効化と判断（+5テスト）。
- [x] `multi-session-buffer-collision` — session_id キーのバッファ分割と
  安定順序のラベル表示を実装（+5テスト）。
- [x] `config-hardcoded` / `cwd-fallback-root` — sk-demo と個人絶対パス既定を
  除去し、CLI/env/cwd 解決＋fail-closed に統一（テスト用固定値は残る）。
  → 2026-11-18: sk-demo は CLI/env/設定へ集約し fail-closed 化。他バイナリの
  既定パス・固定プロンプトは残課題（報告済み）。
- [x] `stderr-and-session-trace-blindspot` — stderr を piped 捕捉し、
  pid/session_id を相関付与、秘密を redact（+12テスト）。
- [x] `stream-polling-and-detach-leak` — WeakEntity + CancellationToken +
  Notify 駆動へ置換し、window close でポンプ終了（+4テスト）。
- [x] `errors-swallowed` / `assert-in-main-path` — bridge / todo_ui の
  説明欠落・projection 未登録をエラー表示へ置換（+6テスト）。
  → 2026-11-18: cordis_title_ui / composition_toggle_ui は解消済み。
- [x] `tool-fs-unvalidated-path` — host 側で scoped FS(read のみ) を provide し、
  正規化・symlink escape 拒否をテストで固定（上流 resolve の未対応は記録）。
- [x] `tests-isolation` — 全テスト一時ディレクトリを固有化し Drop で削除、
  5バイナリ60テストで衝突・残存なしを検証。
  → 2026-11-18: probe 3 ファイルの固定 /tmp を排除し並列分離テストを追加。
- [x] `cordis-bridge-isolated` — 共有 CommandRegistry へ /compact を登録し、
  GPUI 側から registered commands を観測可能に（+2テスト）。
- [x] `ux-pool-triage` — 43件を対応済み6/却下2/次フェーズ35へ分類
  （investigation/ux-pool-triage.md）。
- [x] `deps-upstream-4-tags-behind` — cordis-rs は v0.133.0 に同期済み、
  README / レビュー文書の版数記述も訂正（2026-11-18）。
- [x] `tests-stage-b-online` — NVIDIA 無料モデルで ACP 本文非空（`2`）と stop reason（`EndTurn`）を実証（stage-b-nvidia-result.md）。

### 優先度: 低

- [x] `acp-caret-pin` — agent-client-protocol を =0.11.1 に統一。
- [x] `no-msrv-pin` — rust-toolchain.toml (1.88.0) を追加し実効 MSRV を明記。
- [x] `inconsistent-log-stream` — emit(stream,line) に集約し、結果は stdout、
  診断は stderr と明文化（+6テスト、stderr 相関/redact 維持）。
- [x] `dsh-path-hijack` — DSH_BIN/信頼済み dir から絶対解決し最小 PATH を設定
  （+24テスト）。
- [x] `performance-hypotheses` — concat は O(N^2)、増分が3桁以上高速と実測。
  機能バグではなく性能/DoS 課題と判定（investigation/performance-notes.md）。
- [x] `platform-build-unverified` — gpui feature / rc.exe / xcrun / fxc の条件を
  調査・記録（investigation/platform-build-notes.md）。実ビルドは未確認。
- [x] `license-apache-notice-unverified` — Apache-2.0 本文と著作権表示の不足5件を
  記録（investigation/license-notice-check.md）。法的判断は保留。
- [x] `gitignore-review-artifacts` は却下 — cross-review/ と investigation/ は
  todo が参照する成果物として追跡済みで、無視対象にすると参照が壊れる。
- [x] `correct-prior-pool` — fixture 6件・テスト分母 18bin+main=19 へ訂正。
- [x] `external-research-retry` — tokio 1.54.1 / ACP 5.3.0 を一次情報で確認。
  DSH 側の実プロトコルは未確認として記録（investigation/external-research-retry.md）。

- [x] `stream-prompt-input` — CLI 引数 > 環境変数 > 既定の順で解決し、
  空入力はデフォルトへフォールバック（+5テスト）。

- [x] `host-pipeline-wiring` — ToolPipeline に fail-closed approval を適用し
  AgentLoop へ with_pipeline で配線（+1テスト、DOM/ACP 契約維持）。
- [x] `upstream-approval-guard` — ローカル fork で修正（cordis-rs 94c277b）済み。
  host は answerer を起動時に1度だけ登録し再入しないため現状非該当。ローカル path
  patch は CI ポータビリティを壊すため不採用、公開タグ反映時に取り込む。

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
- 2026-11-28 swarm-unlimited R6（review+discovery 32 エージェント、失敗0）で判断待ちとしてメモ化した候補（TODO 化せず）: `agent-client-protocol` 5.3.0 / `tokio` 1.54.1 更新可否（README の xattr 衝突リスクと `cargo audit` 再評価を伴う）、`fixtures/text-turn.cordis.yml` の `!!js` 式を host 側で評価するか静的値へ書換えるか、DSH 実 `protocolVersion` と approval/request の実 JSON-RPC キャプチャ、背圧／リングバッファ導入とバースト観測、グラフェーム単位 Backspace の Wayland 実機実験、不正 UTF-8 CLI 引数（`args_os` 化）、window `app_id` 明示設定、dsh ハング時の bounded wait、`scripts/smoke-gui.sh` 実実行と証跡取得、Fiber `strong_count` / `register_fiber` 登録残り、`investigation/ux-pool-triage.md` の次フェーズ候補（B-31 / B-39 / B-41 等）。いずれも実実行・仕様変更・優先度判断を伴うためユーザー承認待ち。
- 2026-11-28 のローカル確認: `betterleaks git` は hit zero（exit 0）。ただしリポジトリに `.pre-commit-config.yaml` は存在せず、CI は `audit: false`（`.github/workflows/ci.yml:26`）。pre-commit 主軸のシークレットゲート追加は設定変更を伴うためユーザー承認待ち。

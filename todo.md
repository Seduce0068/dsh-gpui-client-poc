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
  at: "2026-11-18T17:53:02+09:00"
---

## 進行中

（なし）

## 未着手

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
- [ ] `prompt-agent-finish-state` — finish が reply スロット無しでも
  成功扱いし、fail の error をクリアしない
  （src/bin/cordis_prompt_agent_ui.rs:181-186）。
- [ ] `todo-ui-error-not-persisted` — cordis_todo_ui::write の失敗が
  status にしか出ず次の操作で消える（src/bin/cordis_todo_ui.rs:29-44）。
  専用 error フィールドを追加。

### 優先度: 低（swarm コンボ 2026-11-23 round 283 で追加）

- [x] `acp-patch-empty-path` — resolve_patch_path を追加し、空文字は未設定、
  非空で存在しないパスは明示エラーに変更。境界テスト追加（round 289）。
- [x] `stream-tag-serialization-hidden` — tag_from_json が Err を受け取り
  stderr に理由を出し unserializable タグを返すよう修正、回帰テスト追加
  （round 292）。
- [x] `credential-env-non-utf8` — 両 UI の credential getter を var_os +
  to_string_lossy に変更し、非 UTF-8 値を未設定と誤判定しないようにした
  （round 293）。
- [ ] `permission-error-flattened` — switch エラーが ERROR 文字列に
  平坦化されエラー型が失われる（src/bin/cordis_permission_ui.rs:251-255）。
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

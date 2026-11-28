---
type: Reference
title: サイクル001 ラウンド1 — 全体横断クロスレビュー
description: 読み取り専用エージェント6観点によるプロジェクト全体の検出事項（重複含む未整理状態）
tags:
  - cross-review
  - cycle-001
  - round-1
generated: 2026-11-17T20:05:00+09:00
---

## 概要

- 実施日時: 2026-11-17T19:38〜20:05+09:00
- レビュアー: `buildops-reviewer`, `security-reviewer`,
  `completeness-r2`, `arch-r2`, `docs-r2`, `protocol-r2`（読み取り専用）
- 起動した6エージェントのうち3件が途中終了したため、
  `completeness-r2` / `arch-r2` / `docs-r2` / `protocol-r2` を再起動して回収した。
- ソース修正は一切行っていない。検出事項は仮説であり、
  ラウンド6で親の Runtime Proof と照合して妥当性を判定する。
- 既存プール `investigation/round-1.md`（115項目）は参考入力として扱う。

## A. buildops-reviewer（ビルド / CI / 依存 / テスト / 運用）

### `smoke-log-grep-false-positive`

- category: 改善 / severity: high / confidence: high / known: no
- evidence: `scripts/smoke-gui.sh:24-32`, `src/bin/cordis_todo_ui.rs:197`
- `timeout ... || true` で終了コードを捨て、
  `grep -q 'GPUI window created'` のみで成功判定。
  この文字列は `open_window()` が `Ok` を返した時点のログであり、
  Render 結果・実ウィンドウ・ボタン操作・ログ後の panic は検出できない。
- value: 12デモの "ok" が偽の安心感を与え、GUI 回帰ゲートとして信頼できない。

### `thin-tests-smoke-outside-ci`

- category: 改善 / severity: high / confidence: high / known: no
- evidence: `src/bin/*` の `#[cfg(test)]`, `scripts/smoke-gui.sh:3`
- テストは6バイナリのみ（`cordis_prompt_agent_ui`, `cordis_todo_ui`,
  `cordis_composition_toggle_ui`, `cordis_prompt_ui`, `cordis_agent_ui`,
  `cordis_fs_ui`）。全 `acp_*` probe と残りはコンパイルのみ。
  モック provider も無く、ACP 応答本文の検証経路が存在しない。
- value: CI green では描画・操作・ストリーミングの回帰を検知できない。

### `libc-mio-pins-only-in-lock`

- category: 残タスク / severity: high / confidence: high / known: no
- evidence: `README.md:57-64`, `Cargo.toml:8-15`, `Cargo.lock:3142,3401`
- README が必須とする `libc =0.2.177` / `mio =1.1.0` は直接依存になく、
  推移依存の lock エントリにしか無い。`cargo update` で既知のビルド失敗が再発し得る。

### `hardcoded-tmp-state`

- category: 改善 / severity: high / confidence: high / known: no
- evidence: `cordis_composition_ui.rs:56,76,203`,
  `cordis_entries_toggle_ui.rs:160,237,253`
- 固定 `/tmp` パスへ状態を分散。`composition_ui.rs:203` は起動時に
  `remove_dir_all("/tmp/cordis-gpui")` を結果無視で実行。
- value: 並列実行・再起動時の状態汚染と意図しない消去を防ぐ。

### `stale-dependency-notes`

- category: 改善 / severity: medium / confidence: high / known: no
- evidence: `README.md:60,103-107`, `Cargo.toml:10`
- cordis-rs の記載が `v0.29.0`（実体 `v0.62.0`）。
  fixture パスも `snapshots/...` と実体 `fixtures/...` が不一致。

### `no-msrv-pin`

- category: 改善 / severity: medium / confidence: high / known: no
- evidence: `Cargo.toml:4`, `rust-toolchain.toml` 不在（親が確認）
- `edition = "2024"` だが toolchain 未固定。ローカルは
  `rustc 1.100.0`、CI は委譲先のデフォルトに依存。

### `acp-caret-pin`

- category: 改善 / severity: medium / confidence: high / known: no
- evidence: `Cargo.toml:9` 対 `Cargo.toml:12,14,15`
- `agent-client-protocol = "0.11.1"` のみキャレット指定でピン方針が不統一。

### `unsafe-gate-unverified`

- category: 調査必要 / severity: medium / confidence: low / known: no
- evidence: `.github/workflows/ci.yml:20`, `src/**/*.rs` の `unsafe` 0件
- 委譲先 `rust-gates.yml@v1` に `-D clippy::undocumented_unsafe_blocks`
  が含まれるか未確認（外部取得失敗）。現状違反は無し。

### `smoke-timing-flakiness`

- category: 改善 / severity: medium / confidence: medium / known: no
- evidence: `scripts/smoke-gui.sh:3,23-25`
- ディスプレー事前チェックが無く、20秒固定待機。
  初回シェーダーコンパイル遅延で誤 FAIL、ログ後クラッシュで誤 ok。

### `assert-in-main-path`

- category: バグリスク / severity: medium / confidence: high / known: no
- evidence: `cordis_composition_toggle_ui.rs:98`, `acp_multi_turn_probe.rs:71`
- 実行経路の `assert_eq!`。想定外状態で panic し、回復導線が無い。

### `ci-apt-lld-unverified`

- category: 調査必要 / severity: low / confidence: low / known: no
- evidence: `.github/workflows/ci.yml:22-25`, `.cargo/config.toml` 不在
- lld / Wayland dev パッケージの要否が未検証（ビルド未実行のため推測）。

### `inconsistent-log-stream`

- category: 改善 / severity: low / confidence: high / known: no
- evidence: `acp_probe.rs:32,38` の `println!` 対 GPUI 側の `eprintln!`
- ログ収集が stdout のみになった場合に smoke の grep が誤 FAIL する。

## B. security-reviewer（セキュリティ / 入力検証 / 依存）

### `approval-gate-always-allow`

- category: 残タスク / severity: high / confidence: high / known: yes
- evidence: `cordis_permission_ui.rs:48-49,59-76`,
  cordis-rs `tools.rs:309-328`
- answerer が常に `Allow` を返し、preset を変えても deny 分岐に到達しない。
  依存側 gate は `tool:{name}` 文字列のみで引数検査が構造的に不可能。
  他デモは gate 自体が未インストール。
- エージェントは3つの根拠付き仮説を却下した:
  `provider: "spawn"` は OS 起動ではない、`sk-demo` による env 汚染なし、
  `Command::new` の引数インジェクションなし。

### `tmp-shared-state-and-wipe`

- category: 改善 / severity: high / confidence: high / known: yes
- evidence: `cordis_composition_ui.rs:56,76,203` ほか
- A の `hardcoded-tmp-state` と同一。`/tmp` の sticky bit で他ユーザー上書きは
  緩和されるが、同一ユーザー内の並列衝突・再起動時消失は残る。

### `tool-fs-unvalidated-path-full-perms`

- category: 改善 / severity: medium / confidence: high / known: yes
- evidence: `cordis_fs_ui.rs:56-59,68-79`, cordis-rs `tool_fs.rs:29-54`
- 引数の検証は非空文字列のみ。正規化・許可ルート制限・
  symlink 解決が無く、`ToolFsConfig::default()` は
  read/write/edit を全て有効。ACP の fs request 接続で直結する。

### `session-history-in-tmp`

- category: 改善 / severity: medium / confidence: medium / known: no
- evidence: `fixtures/text-turn.cordis.yml:16-20`
- 会話履歴が予測可能な `/tmp/cordis-*` へ保存され、
  権限設定が未確認。圧縮が平文 (`none`) になり得る。

### `api-key-placeholder-in-source`

- category: 改善 / severity: medium / confidence: high / known: no
- evidence: `cordis_composition_ui.rs:93-101`,
  `cordis_entries_toggle_ui.rs:177-185`
- `DEEPSEEK_API_KEY = "sk-demo"` のハードコード。
  実キー漏洩経路はなし（却下済み）が、実接続デモが常に失敗し、
  ダミー値が本番へコピペされるリスク。

### `cwd-fallback-to-root`

- category: 改善 / severity: medium / confidence: medium / known: no
- evidence: `acp_probe.rs:11` ほか全 `acp_*`
- `current_dir()` 失敗時のフォールバックが `/`。
  dsh セッションの作業基点がルートになり得る。

### `git-tag-dependency-mutability`

- category: 改善 / severity: medium / confidence: high / known: yes
- evidence: `Cargo.toml:10`, `Cargo.lock:1021`
- tag は mutable だが lock が commit `22290d9...` に固定。
  脆弱性の有無は RUSTSEC / Advisory と未照合（未確認）。

### `stale-dependency-constraints`

- category: 残タスク / severity: medium / confidence: high / known: yes
- A の `libc-mio-pins-only-in-lock` と `stale-dependency-notes` の統合候補。

### `command-spawn-path-trust`

- category: 調査必要 / severity: low / confidence: high / known: yes
- 6箇所の `Command::new("dsh")` は PATH 解決。
  引数固定・ACP JSON のためインジェクション余地なし（確認済み）。

### `ci-security-gates-unverifiable`

- category: 調査必要 / severity: low / confidence: medium / known: no
- A の `unsafe-gate-unverified` と同一。`@v1` は可変タグ。

### `unsafe-rule-no-violation-but-lint-unproven`

- category: 調査必要 / severity: low / confidence: high / known: no
- `unsafe` 0件は親の grep でも確認済み。CI ゲートの有無は未確認。

### `save-error-swallowed`

- category: 改善 / severity: low / confidence: high / known: yes
- evidence: `cordis_entries_toggle_ui.rs:107-119,253-254`
- `let _ = state.saved.save(&path);` で永続化失敗が不可視。

## C. completeness-r2（未実装 / スタブ / 握り潰し）

- grep 結果: `TODO` / `FIXME` / `unimplemented!` / `todo!` / `panic!` は **0件**。
  `unwrap()` / `expect()` は46件（大半がテスト、本番にも約20件）。

### `prompt-ui-replies-never-filled`

- category: 残タスク / severity: high / confidence: high / known: no
- evidence: `cordis_prompt_ui.rs:41,63-69,97-99,128-139`
- `submit()` は `submitted.push()` と `draft.clear()` のみ。
  `replies` は読み取られるだけで書き込まれず、コメント自身が
  "a real host would ... call `run(prompt)`" と認めている。
- 親の確認: `:67` に push があり、`:97` に `replies.get(index)` のみ。

### `acp-llm-provider-returns-stop-reason`

- category: 残タスク / severity: high / confidence: high / known: no
- evidence: `acp_llm_probe.rs:53,67-85`
- `format!("{:?}", response.stop_reason)` が `AgentRun::final_text` になり、
  call parser は常に空。残高回復後も回答本文が届かない構造的未実装。

### `permission-deny-unreachable`

- category: バグリスク / severity: medium / confidence: high / known: no
- B の `approval-gate-always-allow` と同一（独立再検出）。

### `main-ui-static-hardcoded`

- category: 残タスク / severity: medium / confidence: high / known: yes
- evidence: `src/main.rs:3-5,24-43,115,131-137`
- 静的会話・ハードコード色・入力欄は
  `"Message DSH... (input not wired in PoC)"`。

### `stream-ui-fixed-prompt-and-dead-poll`

- category: 残タスク / severity: medium / confidence: high / known: yes
- evidence: `acp_stream_ui.rs:77,158-169,177-181`
- プロンプト固定。ポーリングの第一引数 `_buffer` は未使用で
  実処理は `cx.notify()` のみ（Render がバッファを読むため冗長）。

### `scripted-llm-stubs`

- category: 残タスク / severity: low / confidence: high / known: no
- evidence: `cordis_title_ui.rs:40-44`, `cordis_agent_ui.rs:21-56`,
  `cordis_retry_ui.rs:35-48`
- 固定文字列を返す provider。ファイル先頭に明示されており隠蔽ではない。

### `title-init-error-swallowed`

- category: 改善 / severity: medium / confidence: high / known: no
- evidence: `cordis_title_ui.rs:100`
- `build_rows().unwrap_or_default()` で全エラーが空行化。
  近傍の `cordis_retry_ui.rs:112` は UI に出す実装で不整合。

### `poison-panic-and-ignored-window-result`

- category: 改善 / severity: medium / confidence: high / known: no
- evidence: 各 GUI バイナリの `expect("... poisoned")` と `let _ = cx.open_window(...)`
- `cordis_todo_ui.rs:194-199` だけがエラーをログ出力。

### `host-specific-paths-and-demo-key`

- category: 改善 / severity: low / confidence: high / known: no
- A/B の tmp パス・`sk-demo` と同一（独立再検出）。

### `readme-unimplemented-contradiction`

- category: 調査必要 / severity: medium / confidence: high / known: no
- README:112 の「ストリーミング未実装」対 README:26,89-101 と実体。
  D/E でも同一趣旨を検出。

## D. arch-r2（アーキテクチャ / 重複 / 技術的負債）

### `click-handler-blocks-ui-thread`

- category: 実行モデル / severity: high / confidence: high / known: no
- evidence: `cordis_entries_toggle_ui.rs:86,89,118,143-145`
- `on_click` が `futures::executor::block_on` で unload/mount と
  ファイル I/O を実行。`Arc<Mutex>` ロック保持中で UI がフリーズし得る。

### `plugin-registry-duplication-and-drift`

- category: 重複 / severity: high / confidence: high / known: no
- evidence: `cordis_composition_ui.rs:68-174`（8件）対
  `cordis_entries_toggle_ui.rs:152-230`（6件）
- 同一 factory 登録が重複し、登録数が 8 対 6 で乖離。
  entries_toggle では2エントリが `skipped_unknown` になる。
- 親の確認: `move |_ctx, _config|` は全14箇所（8 + 6）。

### `mixed-execution-models`

- category: アーキテクチャ / severity: medium / confidence: high / known: no
- evidence: `#[tokio::main]`（composition/entries）、同期 main + 一時 runtime
  （prompt_agent）、`std::thread::spawn` + 別 runtime（acp_stream_ui）
- ランタイムのライフタイムとスレッド所属が未統一。
  コメントの `cx.spawn` と実装の `app.spawn` も不一致。

### `acp-100ms-polling-via-mutex`

- category: アーキテクチャ / severity: medium / confidence: high / known: no
- evidence: `acp_stream_ui.rs:29,35-37,63-75,107,160-168,177-180`
- C の `stream-ui-fixed-prompt-and-dead-poll` と同一。性能影響は未測定。

### `window-boilerplate-duplication`

- category: 重複 / severity: medium / confidence: high / known: no
- `Application::new().run(|cx| { ...; let _ = cx.open_window(...); eprintln!(...); })`
  が11バイナリで反復。タイトル指定・戻り値が捨てられている。

### `echo-provider-and-text-input-duplication`

- category: 重複 / severity: low / confidence: medium / known: no
- `cordis_prompt_ui` と `cordis_prompt_agent_ui` で入力ロジックが再実装。

### `readme-unimplemented-list-stale`

- category: ドキュメント負債 / severity: medium / confidence: high / known: yes
- C と同一。

### `tmp-path-and-demo-key-hardcoded`

- category: 設定 / severity: low / confidence: high / known: no
- A/B と同一。

## E. docs-r2（ドキュメント整合性）

- 照合で不整合なし: バイナリ表は19行で実体19バイナリと1:1一致。
  LICENSE（MIT / 2026 Seduce0068）、`Cargo.toml:6` の MIT、
  gpui の Apache-2.0、日付表記も整合。なお MIT は変更前の履歴記録であり、
  現行ライセンス AGPL-3.0-or-later と矛盾しない。

### `docs-cordis-version-stale`

- category: 改善 / severity: high / confidence: high / known: no
- evidence: `README.md:107`, `Cargo.toml:10`, `Cargo.lock:1019-1021`
- A の `stale-dependency-notes` と同一（独立再検出）。

### `docs-fixture-path-wrong`

- category: 改善 / severity: high / confidence: high / known: no
- evidence: `README.md:105`, `cordis_composition_ui.rs:3,54`
- `snapshots/sdk/text-turn/cordis.yml` は不存在。
  実体は `fixtures/text-turn.cordis.yml` を `include_str!`。

### `docs-investigation-report-section-missing`

- category: 調査必要 / severity: medium / confidence: high / known: no
- evidence: `README.md:60`
- 「調査レポート §4.4」の参照先がリポジトリ内に無い。
  §4.4 が前提とする `agent-client-protocol-tokio` は削除済み。

### `docs-private-design-doc-unresolvable`

- category: 調査必要 / severity: low / confidence: high / known: yes
- evidence: `README.md:121`
- `passkey-gateway/docs/...` は意図的に未共有。公開可能な決定事項の抽出候補。

### `docs-pins-declared-not-in-manifest`

- category: 残タスク / severity: high / confidence: high / known: no
- A の `libc-mio-pins-only-in-lock` と同一（独立再検出）。

### `docs-unimplemented-list-stale`

- category: 改善 / severity: high / confidence: high / known: no
- C/D と同一（独立再検出）。

### `docs-window-verification-contradiction`

- category: 改善 / severity: medium / confidence: high / known: yes
- evidence: `README.md:69` 対 `README.md:73-75`, `cordis_todo_ui.rs:197`
- 「ウィンドウが開く」と断定する一方、自動列挙も目視も未実施と記載。
  根拠は `eprintln!` のみ。

### `docs-smoke-script-overclaims`

- category: 改善 / severity: medium / confidence: high / known: no
- A の `smoke-log-grep-false-positive` と同一（独立再検出）。

### `docs-run-examples-incomplete`

- category: 改善 / severity: medium / confidence: high / known: no
- evidence: `README.md:12-32,36-47,79,128`
- 実行例は9件のみで10件欠落。
  `target/release/` と `target/debug/` のパスが混在。

### `docs-composition-entry-count`

- category: 改善 / severity: medium / confidence: medium / known: no
- evidence: `README.md:105-106`, `fixtures/text-turn.cordis.yml`,
  `cordis_composition_ui.rs:71-174`
- fixture は6件だがデモ用の tool-fs / tool-bash を追加登録し、
  一覧は最大8行になり得る（親の確認: fixture の id は6件）。

## F. protocol-r3（ACP 統合）

`protocol-r2` が2回とも途中終了したため、grep 1回・最大2ファイルに制限した
`protocol-r3` で回収した。一部は親の直接確認で補強している。

### `session-update-variants-dropped`

- category: 残タスク / severity: high / confidence: high / known: yes
- evidence: `acp_stream_probe.rs:45-46`, `acp_stream_ui.rs:104-105`
- `SessionUpdate::AgentMessageChunk` + `ContentBlock::Text` のみを処理。
- 親の確認（ローカル crate `agent-client-protocol-schema-0.12.0/src/client.rs:84`）:
  `SessionUpdate` は11バリアント
  （`UserMessageChunk`, `AgentMessageChunk`, `AgentThoughtChunk`,
  `ToolCall`, `ToolCallUpdate`, `Plan`, `AvailableCommandsUpdate`,
  `CurrentModeUpdate`, `ConfigOptionUpdate`, `SessionInfoUpdate`,
  `UsageUpdate`）。**10バリアントをドロップ**。
- `content.rs:36` の `ContentBlock` は5バリアント
  （`Text`, `Image`, `Audio`, `ResourceLink`, `Resource`）で4つをドロップ。
- value: ツール呼び出し・計画・思考・マルチモーダルが UI に出ない。

### `agent-to-client-request-handlers-absent`

- category: 残タスク / severity: high / confidence: medium / known: no
- evidence: 全 `acp_*.rs` と `acp_stream_ui.rs` への grep で
  `request_permission` / `read_text_file` / `write_text_file` / `terminal/`
  の一致ゼロ。親の直接 grep でも一致ゼロ。
- `Client.builder()` は `name` と `on_receive_notification` のみ設定。
  permission / fs / terminal のリクエストを応答する handler が無い。
  ACP crate 側の既定 handler は `Handled::No` で
  `method_not_found()` になる（プール `investigation/round-1.md:215-219` の指摘と一致）。
- 挙動は未実行のため confidence: medium。

### `acp-stderr-not-captured`

- category: 改善 / severity: medium / confidence: high / known: no
- evidence: `acp_probe.rs:16-17`, `acp_stream_probe.rs:31-32` ほか全 `acp_*`
- `stdin` / `stdout` のみ piped で `stderr` は親に継承。
  GUI では dsh 側の起動失敗・認証エラーがユーザーに見えない。

### `session-close-absent-kill-only`

- category: 残タスク / severity: medium / confidence: high / known: no
- evidence: `acp_probe.rs:43` の `child.kill()`、
  `src/` 全体の `session_close` / `CancelNotification` 是一致的に0件（親が確認）
- graceful shutdown が無くリソースが残存し得る。

### `acp-timeout-unverified`

- category: 調査必要 / severity: medium / confidence: low / known: no
- `block_task().await` にタイムアウト wrapper が見えないが、
  時間制限のため追加確認は未実施。**未確認**。

## G. 親の Runtime Proof（2026-11-17T19:36〜20:00+09:00）

- `git status`: `investigation/` のみ未追跡。ソース変更なし。
- `cargo test --bins`: **EXIT=0**。全テスト成功。
  ただし `#[cfg(test)]` は6バイナリのみ（`cordis_agent_ui`,
  `cordis_composition_toggle_ui`, `cordis_fs_ui`, `cordis_prompt_agent_ui`,
  `cordis_prompt_ui`, `cordis_todo_ui`）。
- `src/bin/*.rs` は18ファイル、`src/main.rs` を含む計19バイナリ。
- `unsafe` は `src/` 全体で0件。
- `rust-toolchain.toml` / `.cargo/config.toml` は不在。
- `README.md:107` の `v0.29.0` 対 `Cargo.toml:10` の `v0.62.0`、
  `Cargo.lock:1021` の commit `22290d9...` を確認。
- `snapshots/sdk/text-turn/cordis.yml` と
  `passkey-gateway/docs/dsh-client-gpui-migration-2026-11-16.md` は不在。
- `cordis_prompt_ui.rs`: `replies` への書き込み無し、
  `submitted.push()` と `replies.get(index)` のみ。
- `cordis_permission_ui.rs:48-49`: 常に `ApprovalDecision::Allow`。
- `move |_ctx, _config|` は14箇所（composition 8 + entries 6）。
- `session_close` / `session_cancel` / `CancelNotification` は `src/` に0件。
- `cordis_composition_toggle_ui.rs:98` は本番パスの `assert_eq!`。
- 全 `acp_*` で `stderr` が piped でなく `stdin`/`stdout` のみ。
- fixture の id は6件（`skill-filesystem`, `llm-deepseek`,
  `session-persistence-jsonl`, `tool-subagent`,
  `snapshot-message-feedback`, `snapshot-feedback-producer`）。

## H. 却下 / 修正した仮説（妥当性確認）

- **fixture エントリ数7とする主張**（`investigation/round-1.md:181`）:
  実体は6件で README:105 と一致。**却下**。
- **「19バイナリ中13にテストが無い」**: 実体は18 bin + main = 19 で、
  テスト6 → 残り13。趣旨は正しいが分母の表現を修正。
- **README バイナリ表の不一致**（一部エージェントの期待）:
  実体と1:1一致。**却下**。
- **`sk-demo` による process env 汚染 / 実キー漏洩**:
  `resolve_api_key` は渡された map のみ参照。**却下**。
- **`provider: "spawn"` = OS プロセス起動**:
  `SubagentRegistry` のプロバイダ名。**却下**。
- **`Command::new("dsh")` の引数インジェクション**:
  引数固定 + ACP JSON のため余地なし。**却下**。

## I. ラウンド1 集計

| レビュアー | 検出数 | 主な貢献 |
| --- | --- | --- |
| buildops | 12 | smoke 誤判定、テスト薄さ、ピン不在、toolchain 未固定 |
| security | 12 | approval 常 Allow、tmp 状態、fs 検証不在、3仮説を却下 |
| completeness | 10 | prompt_ui 未配線、stop reason スタブ、握り潰し |
| arch | 8 | UI スレッドブロック、registry 乖離、実行モデル混在 |
| docs | 10 | バージョン/パス不一致、未実装リスト矛盾、実行例不足 |
| protocol-r3 | 5 | update 10バリアント欠落、handler 不在、stderr 未捕捉 |
| 親 | 項目 G | Runtime Proof と却下根拠 |

重複が多数（tmp / README 矛盾 / approval / smoke / pins / ストリーミング /
実行例）。ラウンド6で統合する。

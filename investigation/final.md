---
type: Reference
title: "残タスク・機能追加案 調査 最終取りまとめ（サイクル DSH-001）"
description: "エージェントチーム25体・5ラウンドの発散調査と6回目の重複排除・妥当性吸収の結果。351件（重複含む）を約40件のアクション項目へ集約。"
tags:
  - investigation
  - cross-review
  - dsh-gpui-client-poc
  - cycle-6
generated: 2026-11-17T21:17:00+09:00
verified: 2026-11-17T21:17:00+09:00
status: current
---

## 概要

- 目的: 未知の残タスク・機能追加案をエージェントチームをふんだんに使って調査し、5回反復した後6回目で重複・妥当性を吸収する。
- 実施: 2026-11-17T19:17〜21:17+09:00。**起動エージェント25体**（ラウンド1〜5 各5体、うち1体失敗を回収）、読み取り専用を原則とし、Runtime Proof（実コマンド・実実行・画像確認）を優先。
- 成果物: `investigation/round-1.md`〜`round-5.md`（生データ）、本ファイル（最終統合）。並行 opencode チームの `cross-review/cycle-001/` は **読み取りのみ**、ルート `todo.md` は **既存行を保持した追記対象** とした。
- 検出数: **351件（重複含む: 115 + 87 + 70 + 79。ラウンド4の反証新規7件は訂正として扱い累計に加算しない）**。自己監査 AUDIT-04 に基づき、代表 id 統合後 **約150〜165件**、実質アクション **約40件**。

## 方法（5回反復 + 6回目吸収）

| ラウンド | 観点 | エージェント | 生検出 |
| --- | --- | --- | --- |
| 1 | コードベース / ドキュメント / ACP / cordis / UX | 5 | 115 |
| 2 | 上流 DSH / gpui ソース / 既存ソリューション / 検証戦略 / 兄弟プロジェクト | 5 | 87（集計表の88は1件過大を訂正） |
| 3 | セキュリティ / ビルド実測 / 依存ライセンス / デッドコード / GitHub | 5 | 70 |
| 4 | 主張の反証（ACP / セキュリティ / ゲート / 並行チーム / 解決策） | 5 | 訂正11件 + 新規7件 |
| 5 | 実実行観察 / エッジケース / 将来性 / 自己監査 / 並行処理 | 5 | 79 |
| 6 | 重複排除・妥当性吸収・優先順位付け（親が統合） | — | 本ファイル |

## 妥当性吸収（最重要: 反証された主張を採用しない）

ラウンド4で確定・訂正された項目を最終判断として採用する。**過大な主張をそのまま残すと不要な実装を生む**ため、最優先で反映した。

| 元主張 | 最終判定 | 採用する正しい記述 |
| --- | --- | --- |
| E-5: ACP permission handler 不在 → `method_not_found` 確実 | **overstated** | DSH は owned session + `callId` + policy `'ask'` の条件付き送信のみ。`'never'` では即 rejected。severity high→**medium**。`dsh-acp/src/index.ts:158-176` |
| E-6: `fs/*` / `terminal/*` handler 不在 → method_not_found | **refuted** | DSH ブリッジは `methods.client` を `session.update` と `session.requestPermission` の2箇所でしか使わず fs/terminal は発行しない。**handler 登録対象外** |
| E-7: session/update 9〜10バリアント ドロップ | **overstated** | DSH 実送信は6種（thought / message / tool_call / tool_call_update / usage / config_option）。Plan / AvailableCommands / CurrentMode / SessionInfo は unsupported 明記。**未描画は5種** |
| E-4: ClientCapabilities=false でツールがスキップされる | **refuted** | `initialize(_params)` は `_params` を参照しない。PoC 側 capability は DSH 挙動に非影響 |
| D-13: stop_reason を本文として agent-loop へ渡す | **confirmed** | `acp_llm_probe.rs:46-55`。本文取得経路が構造的に存在しない。**最優先修正** |
| A-1: 子プロセスの全環境継承 | **confirmed**（訂正） | 6箇所すべて `tokio::process::Command`、`env_clear()` 0件。「スレッド環境も継承」は誤りで**プロセス環境** |
| A-5: 同期 Answerer で対話承認が構造的に不可能 | **overstated** | 回避策あり（ワーカースレッド＋チャネル）。ただし **round-5 E-3 で判明した「read guard 保持の再入デッドロック」が真の障害**で、callback 前に guard を drop する最小修正が必要 |
| A-6: bash RCE チェーン（stderr 外送信含む） | **overstated** | 真因は `AgentLoop::run` が `ToolPipeline` を通らない設計（`agent_loop.rs:234`）。`let (tools, _)` は症状。stderr は piped で UI 表示され外送信は **refuted**。現状 scripted provider で発火しない |
| A-8: session cwd が子 Command に未適用 | **overstated** | 事実だが `NewSessionRequest.cwd` は別経路。dsh 側の解釈は未検証で被害は自動導かれない |
| B-1/C-6: cargo audit 26件すべて実害 | **overstated** | **11件は非到達**（quinn-proto×8, quinn-udp, quick-xml 0.30.0×2）。最大 Severity 8.7 も非到達。実害候補15件、最優先は到達する rmcp `RUSTSEC-2026-0462`（6.5） |
| C-7: tokio `=` ピンが直接依存の実害 | **overstated（因果）** | advisory 実在・修正不能は事実だが影響は `AsyncFd` 4関数のみで PoC は未使用。warning 扱いで CI 赤にならない |
| B-2/D-18: doc test ゲートは `src/lib.rs` 無しで絶対失敗 | **overstated** | `--doc` を外せば EXIT=0。lib 追加は「doc test を使う場合」の前提 |
| D-19: 未使用依存削除タスク不要と確定 | **overstated** | machete 自ら "fast yet imprecise"。検出範囲では0件という意味に緩和 |
| 並行チーム C1/C2 | **DSH 側が正しい** | cross-review final.md:32,46 と todo.md:24,27 は過大なまま。**追記行で訂正**（後述） |

## 最終アクション項目（重複排除・優先順位付き）

### 最優先（P0: 妥当性訂正済み・即着手可）

1. **`llm-probe-returns-stop-reason`** — `acp_llm_probe.rs:46-55` の stop_reason 本文化を修正し、session/update 経由で本文を取得。全 E2E 検証の前提（round-4 confirmed）。
2. **ACP permission handler（条件付き）** — `session/request_permission` のみ登録。severity medium。`fs/*` / `terminal/*` は登録しない（round-4 C1）。
3. **session/update 6種のうち未描画5種** — ToolCall / ToolCallUpdate / AgentThoughtChunk / UsageUpdate / ConfigOptionUpdate を描画または無視理由をログ。**Plan 等は対象外**（round-4 C2）。
4. **`cordis-approval` の正しい修正** — `ApprovalService::request` の read guard を callback 前に drop（round-5 E-3）＋ tool 引数を `ApprovalRequest` へ追加。`AgentLoop` に `ToolPipeline` を注入する API が必要（round-4 A-6 真因）。
5. **`fs-read-errors-abort-before-window`** — 存在しないパス/権限なしで Window が開かずエラー非表示。再現容易・render 側 Err 表示で修正可（round-5 E5B-05）。
6. **`state-json-corruption-and-silent-save-failure`** — 破損 JSON で選択が初期化、保存失敗が黙って捨てられ再起動で revert（round-5 E5B-19/20）。
7. **`deps-vulnerability-triage`** — 到達15件を優先、最優先 rmcp 6.5。tokio 0386 は AsyncFd 導入時に修正（現状は warning）（round-4 C）。

### 高（P1: 構造改善）

8. **GUI 目視検証の自動化** — `spectacle -b -n -o /tmp/...` + `read_image` でエージェント自身が Wayland GUI を確認できた（OBS-3 実証）。README:71-84 の「ユーザー目視必要」は obsolete。smoke の stderr grep を証跡取得へ置換。
9. **4モジュール抽出（`src/lib.rs`）** — `acp_client` / `ui_theme` / `text_input` / `window_runner`。重複（ACP起動6 / registry14 / 入力2 / window13 / 色79）の共通原因。gpui-pre 混用禁止・MIT/Apache 境界を遵守（round-3 D-21, round-4 E-5）。
10. **`Channel::duplex()` モック E2E** — `connect_with(2引数)` + `try_join!` 並行駆動が必須（round-2 の `connect_to` 案はハング）。`from_async_text` / `echo_provider` で残高非依存（round-4 E-1/E-6）。
11. **`detached-poll-keeps-strong-entity-alive`** — WeakEntity + 完了通知 + CancellationToken（round-5 E-1, E5B-08）。
12. **型境界の確立** — ACP schema を `acp_adapter` 1枚へ、Domain の `String` を View 境界で `SharedString` へ、cordis Context を `composition` モジュールへ（round-5 C）。
13. **cordis config/d failed 修正** — `_config` 適用、`enabled_with` で YAML disabled 尊重、`MountReport::failed` 描画（todo.md:32 既存項目を維持）。
14. **session 永続化 + 復元** — `session/list` は DSH 正式対応（round-2 A-07）。固定 /tmp を XDG state dir + 原子的保存へ（todo.md:26 既存）。
15. **IME 入力基盤** — gpui 0.2.2 に `EntityInputHandler` / IME marked range / `examples/input.rs` が存在（round-2 B-A2/A3）。`FilterInput`（Apache-2.0）を gpui 0.2.2 向け8メソッドアダプタ60〜80行で移植。上流待ちにせず Enter と変換確定を分離。

### 中（P2: 機能追加・UX）

16. **UX プールの triage を実行し直す** — 43件のうち主要7件しか todo に反映されず、約30件が消滅。特に B-29 deep-link / B-31 background-startup / B-5 session-folders は完全消滅。B-9 virtualization は「API あり」で終わり、B-26 tray はタスク化されず（round-5 AUDIT-05）。cross-review `cycle-001/final.md:125` の「登録した」という記述も「主要7件のみ」と訂正が必要。
17. **Markdown 描画** — `pulldown-cmark`（ローカルキャッシュ済み）をパーサとして採用、AST→GPUI Element は自作。**gpui-component を不採用とした理由（gpui-pre 0.3.12 によるバージョン非互換）を ADR に記録**（AGENTS.md §2）。
18. **通知 / トレイ / ホットキー** — `notify-rust`（zbus 6 併存に注意）/ `tray-icon`（ksni feature）/ `ashpd`（zbus 5.13 不一致で**即時採用却下**）。既存 `dsh-ntfy-notify` の設計（away 判定・30分 dedupe・失敗分離）を転移し送信は委譲（round-2 E-10）。
19. **その他の UX** — セッション CRUD / 検索 / スクロールバック / コードコピー / メッセージ編集 / 添付 / タイムスタンプ / トークン使用量 / tool call 可視化 / エラー分類 / キーボード操作 / コントラスト（round-1 B）。
20. **cordis 未活用 port / service** — command 系4種 + tool 系2種、core service 11/26。v0.67 の新 port は Cargo.toml 更新済み（round-3 F-1/F-11, round-5 C）。

### 低（P3: 保守・ドキュメント）

21. **ドキュメント同期** — README の cordis-rs v0.29.0 → **v0.67.0**（3段階古い）、fixture パス、未実装リスト（streaming/入力は実装済み）、実行例10件欠落、libc/mio を manifest へ明示（round-5 E-9, AUDIT-04）。
22. **CI / ゲート** — `cargo test --doc` は lib target 追加後のみ。audit は到達性 triage 後に有効化。`undocumented_unsafe_blocks` は現状 unsafe 0件で通過。MSRV / `.gitignore` に investigation・cross-review を追加（round-3 B, round-5 D/AUDIT-12）。
23. **acp-caret-pin / ログ統一 / cwd fallback / assert 除去 / stderr 捕捉** — todo.md の「優先度: 低」に既存項目として維持（現在の行番号は git diff により変動するため、id で参照すること）。
24. **公式ロードマップ / ACP 5.x 対応の調査継続** — 未確認として明示（round-2 A-13, round-5 C）。

## ユーザーへの意思決定事項（エージェントが判断しない）

1. **案C継続 vs 案B（gpui-pre + gpui-component）移行** — エージェント推奨は「UI層のみ案Bへ移行、プロトコル層は案C成果を維持」のハイブリッド。gpui 0.2.2 は1年凍結、gpui-pre は週次、UI17ファイルが gpui 型に直結。前提4条件（DSH の ACP 継続、兄弟移行の実利益、残高回復、追従リソース）は未確認。
2. **モデル残高の補填 or `Channel::duplex()` オフライン検証の選択** — 課金・外部送信を伴うためエージェント判断ではない。
3. **`dsh` 本体のバージョン / ACP 対応方針 / 公式ロードマップ** — 一次情報では未確認。
4. **配布時の Apache-2.0 NOTICE 表示方針** — MIT 自体は判断済み（上書きしない）。
5. **gpui 0.3 が出た場合の再判断タイミング** — [zed#66275](https://github.com/zed-industries/zed/issues/66275) の状態監視。

## 採用・却下した候補（AGENTS.md §2 に基づく記録）

| 候補 | 判定 | 理由 |
| --- | --- | --- |
| `Channel::duplex()` | **採用**（呼び出し形式を修正） | 追加依存ゼロ。ただし `connect_with(2引数)` + `try_join!` 必須 |
| `tm-viewmodel::FilterInput` | **採用** | Apache-2.0・GPUI 非依存の pure logic。gpui 0.2.2 向けアダプタ60〜80行が必要 |
| `notify-rust` | **条件付き採用** | Wayland 可。zbus 6 要求で既存 zbus 5 との二重リンクに注意 |
| `tray-icon`（ksni） | **条件付き採用** | Plasma SNI 実動作は要確認 |
| `pulldown-cmark` / `comrak` | **条件付き採用** | ローカルキャッシュ済み。AST→Element は自作 |
| `gpui-component` / `gpui-kit` | **不採用** | gpui-pre =0.3.12 を要求し `gpui =0.2.2` と型衝突（バージョン非互換）。案B移行時に再評価 |
| `global-hotkey` | **不採用** | README が "Linux (X11 Only)"。Kubuntu Wayland と要件不一致 |
| `ashpd::global_shortcuts` | **不採用（即時）** | zbus ^5.13 が lock 5.12 と不整合＋オフライン解決不可＋portal 依存 |
| `agent-client-protocol-test::MockTransport` | **不採用** | crates.io 未公開で依存追加不可。Channel::duplex で代替可 |
| `tao` / `winit` / Zed checkout / `egui_commonmark` | **不採用** | GPUI Platform との二重化・依存過大・別フレームワーク |

## 並行チームとの統合

- opencode チームは cycle-001 の6ラウンドを完了し、`todo.md`（79行）に G1〜G8 を反映済み。**上書きせず訂正行を追記**した（AGENTS.md §12）。
- 主要な訂正追記: `acp-handlers-absent`（条件付き permission のみ・fs/terminal 対象外）、`stream-update-variants-dropped`（6種のうち5種・Plan 対象外）、`deps-and-docs-stale`（v0.67.0）、`external-research-retry`（調査済み）、`agent-ui-pipeline-no-gate`（真因は AgentLoop の pipeline バイパス）、`ux-pool-triage`（約30件未整理の明示）、plus round-5 新規 P0 3件（fs-read-errors / state-json / detached-poll）。
- `todo.md` への差分は G 由来のチェック項目を **1行も削除・改変していない**。git 上の削除3行は「優先度 高/中/低」の `（なし）` プレースホルダで、これは opencode チームが G1〜G8 を反映した時点で既に置換されていたもの（HEAD の todo.md は39行・チェック項目0行）。
- cross-review の独自発見（`MountReport::failed` ignored、`disabled` bypass、`.gitignore`、`no-msrv-pin`、CI ゲート確認）は採用し、逆輸入した。
- cross-review の frontmatter `generated` は実 mtime より未来（+5〜15分）だが、opencode 担当のため書き換えず記録のみ。

## 検証エビデンス（Runtime Proof 抜粋）

| コマンド /  observation | 結果 |
| --- | --- |
| `cargo fmt --check` / `cargo check --all-targets --locked` / `cargo clippy -- -D warnings` / `cargo test --all-targets --locked` | 全 EXIT=0（8 tests passed） |
| `cargo test --doc --locked` | EXIT=101（lib target 不在・既存 baseline） |
| `cargo audit` | EXIT=1、26 vulnerabilities / 23 warnings（うち11件非到達と判明） |
| `cargo machete` | EXIT=0（検出範囲で未使用依存0件） |
| `grep -rn 'unsafe' src/` | 一致0件（AGENTS.md §15 遵守） |
| `cargo build --example input -p gpui --offline` | EXIT=101（`unicode-segmentation` が dev-dependency） |
| `cargo check --target wasm32-unknown-unknown -p gpui` | EXIT=101（errno crate 非対応）→ wasm32 不可と確定 |
| `timeout 20 ./target/debug/acp_probe` | EXIT=0、INIT OK / SESSION OK、stdout 空・stderr に警告混在 |
| `timeout 8 ./target/debug/cordis_todo_ui` + `spectacle` + `read_image` | exit 124（timeout）、 Wayland ネイティブウィンドウの実描画を画像で直接確認、残存プロセスなし |
| `xwininfo -root -tree \| grep cordis` | 該当なし（Wayland ネイティブ）。ただし spectacle では写る |

## 結論

5ラウンドの発散調査（351件・重複含む）と6回目の吸収により、**過大・反証された主張を除いた約40件の実質アクション**を得た。最も重要なのは (1) stop_reason 本文化の修正、(2) ACP permission の条件付き扱いと fs/terminal の除外、(3) `AgentLoop` が `ToolPipeline` を通らない設計、(4) `ApprovalService` の read guard 再入、そして (5) spectacle による GUI 自己検証の成立 である。ユーザー判断が必要な5項目（特に案C vs 案B）を除き、残タスクは `todo.md` に反映済み。

本調査は読み取り専用で実施し、ソースコードは変更していない（`Cargo.toml` の cordis v0.67.0 更新は並行 opencode セッションによるもの）。

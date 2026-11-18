---
type: Reference
title: サイクル001 ラウンド4 — 外部リサーチ（依存の鮮度と上流の進展）
description: cordis-rs / gpui / ACP / 共通ワークフローの一次情報による鮮度確認
tags:
  - cross-review
  - cycle-001
  - round-4
  - external-research
generated: 2026-11-17T20:40:00+09:00
---

## 概要

- Web 検索は DeepSeek API の残高不足（HTTP 402）と rustxng の
  engine timeout で失敗したため、使用できる一次情報源
  （GitHub MCP / GitHub REST API / crates.io API / raw.githubusercontent.com）に限定した。
- 得られた事実のみを記録し、未取得の項目は「未確認」とする。

## A. cordis-rs の鮮度

出典: GitHub MCP `list_tags`（2026-11-17 取得）および
<https://api.github.com/repos/Seduce0068/cordis-rs/compare/v0.62.0...v0.66.0>

- 利用可能なタグ（新しい順）: `v0.66.0`, `v0.65.0`, `v0.64.0`,
  `v0.63.0`, `v0.62.0`, `v0.61.0`, `v0.60.0`, `v0.59.0`。
- 本リポジトリは `tag = "v0.62.0"`（commit `22290d9`）で、
  `v0.66.0` に対して **8コミット／4タグ遅れ**。
- `compare` の `ahead_by: 8`, `behind_by: 0` を確認。

### `upstream-new-ports-available`

- category: 機能追加案 / severity: info / confidence: high / known: no
- v0.63〜v0.66 で追加された主要機能:
  - `sessionTitles` サービス + `session-title-all-prompts-llm` port
    （タイトル生成を共有 llm / sessionTitles seam へ接続）
  - `ctx.schedule` サービス + `tool-schedule` port
    （`schedule_create` / `schedule_list` / `schedule_view` / `schedule_edit`）
  - `ctx.planMode` サービス + `exit_plan_mode` tool
  - Cargo プロファイル統一（本リポジトリの `6b6bb1e` と同趣旨）
- README 上の总量: コアサービス 26 → 28、再利用 port 14 → 16、
  テスト 83 → 88。
- value: ラウンド1/2 で「未 mount」と指摘した service の一部が
  上流で整備済み。アップデート判断の材料になる。

### `upstream-todo-md-convention`

- category: 仕様確認 / severity: info / confidence: high / known: no
- v0.66 の commit `fd2adbe` で cordis-rs 自身が
  `todo.md`（OKF frontmatter + `## 進行中` / `## 未着手` / `## 完了` / `## メモ`）を追加。
  共通指示 §12 の形式と一致しており、本リポジトリでも同じ形式を採用可能。

### `upstream-mit-version-note`

- category: 調査必要 / severity: info / confidence: high / known: no
- compare 内の cordis-rs README 差分にも
  「上流 `snapshots/sdk/text-turn/cordis.yml` の6エントリ」と記載があり、
  本リポジトリの fixture 6件（親が確認済み）と整合。
  **プールの「7エントリ」主張は再び却下**。

## B. gpui の鮮度

出典: <https://crates.io/api/v1/crates/gpui>（2026-11-17 取得）

- `max_version` / `newest_version` / `default_version` はいずれも **0.2.2**。
  本リポジトリの `gpui = "=0.2.2"` は**最新安定版**でアップグレード先なし。
- 0.2.2 の公開日は 2025-10-22、ライセンス Apache-2.0、edition 2024。
- ダウンロード数: 累計 510,180 / 直近 295,160。
  約1年経過しているが利用は活発（推測:  residential 利用の継続）。
- 過去バージョン 0.2.1 / 0.2.0 は yank → unyank の履歴があり、
  0.2.2 が安定版として固定されている。`=0.2.2` の厳密ピンは妥当。
- `test-support` feature が存在（leak-detection / test-support 等）。
  ラウンド2 の「Render が未検証」に対する既存手段候補だが、
  本リポジトリでは有効化されていない（未確認）。

## C. ACP の鮮度

- 取得試行: crates.io API（fetch 失敗）、Web 検索（HTTP 402）、
  rustxng（engine timeout）。**いずれも失敗**。
- 確認できること: 本リポジトリの `Cargo.lock` は
  `agent-client-protocol 0.11.1` と `agent-client-protocol-schema 0.12.0` を使用。
  ローカル crate ソースの一次情報はラウンド2 で確認済み。
- **未確認**: crates.io 上の最新安定版、0.12 系への移行有無、
  セキュリティアドバイザリ。
  推測で断定しない。

## D. 共通 CI ワークフロー

出典: <https://raw.githubusercontent.com/Seduce0068/workflows/v1/.github/workflows/rust-gates.yml>
（ラウンド2 D で確認済み）

- タグ `v1` の SHA は MCP `list_tags` で
  `b7675efd8a9a84ebc5b560da628d1a82f3fbf98f` と確認。
- ゲート: fmt → check(`--locked`) → test(`--locked`) →
  clippy(`-D warnings`) → audit(任意・既定 false) → extra_gates。
- `clippy_args` 既定空 → `undocumented_unsafe_blocks` ゲートなし。
- `audit` 既定 false → `cargo audit` 未実行。
- これらは「未確認」ではなく**確認済みのギャップ**として確定。

## E. 外部リサーチで得られなかった項目

| 項目 | 理由 |
| --- | --- |
| ACP の crates.io 最新版 | API fetch 失敗 / 検索 402 |
| DSH 本体（deepseek-harness）の公式ロードマップ上のネイティブクライアント位置づけ | Web 検索失敗。非公開設計資料も未読 |
| GPUI の Markdown レンダリング難度の定量評価 | 一次情報なし |
| RUSTSEC / GitHub Advisory との照合 | `cargo audit` 未実行（読み取り専用のため） |

## F. ラウンド4 集計

| 情報源 | 結果 | 主な貢献 |
| --- | --- | --- |
| GitHub MCP / REST | 成功 | cordis-rs の 4タグ遅れと新 port の具体 |
| crates.io API | 一部成功 | gpui 0.2.2 が最新版であることを確定 |
| Web 検索 | 失敗 | 残高不足（402）で代替手段に切り替え |
| 共通 workflow raw | 成功 | CI ゲートの実体を確定 |

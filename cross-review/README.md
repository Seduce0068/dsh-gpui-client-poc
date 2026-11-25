---
type: Reference
title: プロジェクト全体クロスレビューの追跡
description: 修正を伴わない多観点レビューを5回反復し、6回目で重複排除・妥当性評価を行って todo.md に反映するサイクルの管理方法
tags:
  - cross-review
  - read-only
  - cycle-tracking
generated: 2026-11-17T19:36:00+09:00
---

## 目的

プロジェクト全体を様々な観点でクロスレビューする。**ソースコードの修正は行わない**。
読み取り専用のエージェントチームをふんだんに使い、5回反復した後、6回目で
重複・妥当性などを吸収し、リポジトリルートの `todo.md` に追加する。

このサイクルは必要に応じて繰り返す（goal の最大ラウンド数 256）。

## ルール

- レビュアーエージェントは `read` / `glob` / `grep` のみ。ファイル書き込み・git 操作・ビルドは禁止。
- 統合（親エージェント）のみが `cross-review/` 配下と `todo.md` へ書き込む。
- エージェントの指摘は仮説として扱い、親の Runtime Proof（ローカル実行・ファイル確認）で事実確認する。
- 性能・スケールの指摘は測定根拠がない限り「機能バグ」と区別して記録する。
- 外部事実は出典を明記し、未確認は「未確認」と書く。
- failed 通知を無視せず、必ず status を確認して回収する。

## サイクル構成

| ラウンド | 役割 | 成果物 |
| --- | --- | --- |
| 1 | 全体横断レビュー（複数観点の並列レビュー） | `cycle-NNN/round-1.md` |
| 2 | プロトコル / データ検証戦略の深掘り | `cycle-NNN/round-2.md` |
| 3 | セキュリティ敵対レビュー + 運用 / 依存監査 | `cycle-NNN/round-3.md` |
| 4 | 外部リサーチ（上流プロジェクト / 仕様の進展） | `cycle-NNN/round-4.md` |
| 5 | 抜け漏れ・エッジケース・将来性の敵対レビュー | `cycle-NNN/round-5.md` |
| 6 | 重複排除・妥当性評価・優先順位付け → `todo.md` 反映 | `cycle-NNN/final.md` |

## サイクル 001 のラウンド割当て

| ラウンド | 観点 | 状態 |
| --- | --- | --- |
| 1 | arch / completeness / protocol / security / buildops / docs（6並列） | 完了（`round-1.md`） |
| 2 | ACP handler / 検証戦略 / cordis port | 完了（`round-2.md`） |
| 3 | 敵対的セキュリティ / 運用 / ライセンス | 完了（`round-3.md`） |
| 4 | 外部リサーチ（GitHub / crates.io / 共通 workflow） | 完了（`round-4.md`） |
| 5 | 抜け漏れ / エッジケース / メタ監査 | 完了（`round-5.md`） |
| 6 | 重複排除・妥当性評価 → `todo.md` 反映 | 完了（`final.md`） |

## 検出項目の統一フォーマット

各ラウンドの成果物では、各項目を次の構造で記録する。

- `id`: 短い英小文字 slug
- `title`: 見出し
- `category`: 残タスク / 機能追加案 / 改善 / 調査必要 / バグリスク
- `severity`: critical / high / medium / low
- `evidence`: `file:line` または出典 URL、あるいは明示した推論
- `description`: 何が問題 / 提案か
- `value`: 価値・根拠
- `confidence`: high / medium / low
- `known_in_readme`: README に記載済みか（yes / no）
- `source`: レビュアー名 または `parent-runtime-proof`

## 既存の入力プール

- `investigation/round-1.md`: 別タスクで既に実施された 5 観点並列調査（115 項目・重複あり）。
  新規発見の参考プールとして扱い、重複はラウンド 6 で吸収する。内容は無条件に正しいとは限らない。

## 対象リポジトリ

- ローカル: `<workspace>`
- GitHub: <https://github.com/Seduce0068/dsh-gpui-client-poc>
- 主要技術: Rust / GPUI `=0.2.2` / ACP `0.11.1` / cordis-rs `v0.62.0`

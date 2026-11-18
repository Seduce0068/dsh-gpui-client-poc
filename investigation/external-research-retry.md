---
type: Reference
title: 外部調査の再試行（ACP 5.x / DSH / tokio）
description: >-
  Web 検索が利用不可だったため、crates.io API・GitHub MCP・npm registry・
  ローカル DSH インストールで再調査した事実と未確認事項を記録する。
tags:
  - acp
  - dsh
  - tokio
  - investigation
generated: 2026-11-18T17:12:27+09:00
verified: 2026-11-18T17:12:27+09:00
status: completed
---

## 調査目的と範囲

既存調査（`round-2.md`, `round-5.md`）では Web 検索の失敗により
「tokio 最新版は未確認」「ACP 5.x と DSH の互換性は未確認」とされていた。
本レポートでは、利用可能な検索手段を改めて試し、確認できた事実と
未確認のまま残る事項、および追加確認手順を記録する。
既存ファイルは変更していない。

## 検索手段の可用性

- `web_search`: **利用不可**。DeepSeek API が
  `HTTP 402: Insufficient Balance`（request_id:
  `ee2c0794-e6b0-4d74-9d89-b5de1b27b189`）を返した。
  既存ラウンドと同じ障害。
- `web_fetch`: **利用可**。crates.io API が HTTP 200 で応答した。
  既存ラウンドの `TypeError: fetch failed` は今回は再現しなかった。
- `curl`: **利用可**。crates.io API と npm registry から JSON を取得できた。
  User-Agent を付与して実行（既存ラウンドの 403 の原因は未確認）。
- GitHub MCP: **利用可**。`list_releases`, `list_tags`,
  `search_repositories` が応答した。
- RustXNG `search`: **接続可だが本調査には不適**。
  ACP のクエリは該当結果 0 件、
  tokio のクエリは無関係な crates.io 結果（fp-bench 等）と
  wikipedia の timeout のみだった。版数の根拠には使わない。

## 確認できた事実

### tokio 最新版

- crates.io API の `max_version`, `newest_version`, `default_version` は
  すべて `1.54.1`。公開日時は `2026-10-21T21:05:43Z`。
  `rust_version` は `1.85`、ライセンスは MIT。
  <https://crates.io/api/v1/crates/tokio>
- GitHub `tokio-rs/tokio` の最新リリースも `tokio-1.54.1`
  （`2026-11-18` 時点で確認、`2026-10-21T21:07:05Z` 公開）。
  <https://github.com/tokio-rs/tokio/releases>
- `1.54.0` は crates.io API 上で `yanked: true`
  （`2026-10-21T21:16:13Z` に yank）。更新先は `1.54.1` を選ぶべき。
- 本リポジトリの `Cargo.toml:14` は
  `tokio = { version = "=1.48.0", features = ["full"] }`。
  最新版まで 6 マイナー遅れという既存調査（`round-3.md` C-18）と一致する。
- 既存調査が記録した `RUSTSEC-2026-0386` と修正版
  （`>=1.51.6` または `>=1.53.3`）は、本再調査では
  `cargo audit` を実行していないため未検証。

### ACP 5.x 最新版と版管理

- crates.io API の `agent-client-protocol` は
  `max_version`, `newest_version`, `default_version` がすべて `5.3.0`。
  公開日時は `2026-11-14T16:02:29Z`、`rust_version` は `1.88.0`、
  edition 2024、ライセンス Apache-2.0、
  `trustpub_only: true`（署名付きパブリッシュ専用）。
  リポジトリは `agentclientprotocol/rust-sdk`。
  <https://crates.io/api/v1/crates/agent-client-protocol>
- `5.3.0` の feature 一覧には `unstable_protocol_v2`、
  `unstable_session_archive`, `unstable_session_fork`,
  `unstable_session_status`, `unstable_session_set_title`,
  `unstable_plan_operations`, `unstable_agent_skills` 等がある。
  既存の `0.11.x` にあった `unstable_session_close` と
  `unstable_session_model` は一覧に存在しない。
  これは `round-5.md` の feature 名変更の指摘を裏付ける。
- GitHub `agentclientprotocol/rust-sdk` に `v5.3.0`
  （`agent-client-protocol-v5.3.0`、`2026-11-14T16:02:32Z`）がある。
  <https://github.com/agentclientprotocol/rust-sdk/releases>
- 一方、GitHub `agentclientprotocol/agent-client-protocol`（仕様リポジトリ）
  のリリースは `v1.18.0`（Rust Crate）、`schema-v1.32.0`、
  `schema-v2.0.0-alpha.16`（pre-release）で、まったく別のタグ体系。
  「ACP 5.3.0 = 仕様リポジトリの v5.3.0」と混同しないこと。
  <https://github.com/agentclientprotocol/agent-client-protocol/releases>
- npm の `@agentclientprotocol/sdk` 最新は `2.7.0`
  （リポジトリ `agentclientprotocol/typescript-sdk`）。
  Rust crate と npm パッケージでバージョン番号は独立している。
  <https://registry.npmjs.org/@agentclientprotocol/sdk/latest>

### DSH との互換性

- 事実: ローカル DSH は `0.2.2-rc.3` で、
  `package.json:126` が `@agentclientprotocol/sdk` を `1.4.0` に固定。
  インストール済み SDK の `package.json` の version も `1.4.0`。
- この SDK 1.4.0 は v1 と v2 の両方を保持している。
  v1 は `dist/schema/index.js:51` の `PROTOCOL_VERSION = 1`、
  v2 は `dist/v2/schema/index.js:42` の `PROTOCOL_VERSION = 2`。
  `dist/protocol-router.js` も両バージョンを扱う実装を持つ。
- ただし、DSH 同梱の `src/` と `lib/` を検索しても
  `agentclientprotocol` と `protocolVersion` の直接参照は 0 件だった。
  DSH コアが実際に v1 と v2 のどちらで ACP セッションを話すかは
  **未確認**。
- 既存の実 handshake 記録（`round-5.md`）は `agent_info=0.0.1` のみで、
  `protocolVersion` は記録していない。ローカルの
  `src/bin/acp_probe.rs:211` も `agent_info` を出力するだけで
  `protocolVersion` は出力しない。
- 推測（断定しない）: ACP crate 5.x の `unstable_protocol_v2` と
  SDK 1.4.0 の v2 サポートには理論上の親和性があるが、
  実際の wire 互換性は未検証であり「互換性あり」とは断定できない。

## 未確認事項と追加確認手順

- DSH が話す ACP プロトコル版の特定:
  `dsh --profile acp` を実起動し、`initialize` の
  `protocolVersion` をキャプチャする。既存 probe の出力に
  `protocolVersion` を追加する形が最小（本タスクでは変更しない）。
- ACP 5.3.0 への更新可否:
  `cargo update -p agent-client-protocol --precise 5.3.0` の後に
  `cargo check` と `cargo test` を実行し、feature 名変更
  （`unstable_session_close` / `unstable_session_model` の消滅）の
  影響を確認する。
- tokio 1.54.1 への更新可否:
  `cargo update -p tokio --precise 1.54.1` の後に
  `cargo check` と `cargo test` を実行する。
  ホスト側の間接依存との衝突リスクは `round-5.md` の指摘通り残る。
- 脆弱性の再評価: 更新後に `cargo audit` を実行し、
  `RUSTSEC-2026-0386` の状態を確認する。

## 既存調査との差分

- `round-5.md` の「tokio 最新版は未確認」は、今回 web_fetch が成功した
  ため `1.54.1` と確定した。
- ACP 最新版 `5.3.0` という数値自体は既存調査と一致した。
  ただし仕様リポジトリの `v1.18.0` は別系統のタグであり、
  今回その違いを明示した。
- DSH 互換性は、SDK 版とプロトコル定数という新たな手がかりが
  得られた一方、実通信の確認には至らず未確認のままである。

## 出典

- crates.io API（ACP）: <https://crates.io/api/v1/crates/agent-client-protocol>
- crates.io API（tokio）: <https://crates.io/api/v1/crates/tokio>
- GitHub releases（Rust SDK）: <https://github.com/agentclientprotocol/rust-sdk/releases>
- GitHub releases（仕様）: <https://github.com/agentclientprotocol/agent-client-protocol/releases>
- GitHub releases（tokio）: <https://github.com/tokio-rs/tokio/releases>
- npm registry: <https://registry.npmjs.org/@agentclientprotocol/sdk/latest>
- ローカル DSH: `/home/koppe/.npm-global/lib/node_modules/@deepseek-ai/dsh/package.json`
- ローカル SDK: `/home/koppe/.npm-global/lib/node_modules/@deepseek-ai/dsh/node_modules/@agentclientprotocol/sdk/package.json`

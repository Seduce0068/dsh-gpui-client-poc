---
type: Report
title: "Apache-2.0 NOTICE 充足状況の調査"
description: "gpui と agent-client-protocol の著作権表示・NOTICE をローカル成果物と照合した記録"
tags:
  - license
  - apache-2.0
  - notice
  - gpui
  - agent-client-protocol
generated:
  by: "dsh-licensecheck"
  at: "2026-11-18T16:50:42+09:00"
status: "review"
---

## 目的と結論の限定

THIRD_PARTY_NOTICES.md / README.md / LICENSE と、ローカルに展開された依存
クレートの LICENSE を照合し、Apache-2.0 の著作権表示・NOTICE 表示義務の
充足状況を事実として記録する。

本レポートは法的結論を断定しない。条項の適用可否（配布形態、派生著作物
該当性を含む）は法務または保守責任者の確認事項であり、ここでは根拠と
未確認事項を分離して列挙する。

## 調査対象と方法

対象は直接依存の `gpui =0.2.2` と `agent-client-protocol =0.11.1` とする。
補助として、ACP の推移依存 `agent-client-protocol-schema 0.12.0` を確認した。

方法は次のとおり。

- リポジトリ内ファイルの直接読取。
- crates.io ローカルキャッシュの `.crate` アーカイブに対する
  `tar tf` での同梱ファイル確認。
- GitHub MCP による上流リポジトリのルート NOTICE / LICENSE の有無確認。
- 既存調査 `investigation/round-3.md` の C-4 との照合。

## 根拠

### リポジトリ側

- `LICENSE` は AGPL-3.0 本文のみ（全 661 行）。Apache-2.0 本文は無い。
- `THIRD_PARTY_NOTICES.md` は旧 MIT 本文と
  `Copyright (c) 2026 Seduce0068`（10 行目）のみ。gpui / ACP /
  Zed Industries / Apache-2.0 の記載は無い。
- `README.md:174-178` は「上流 gpui は Apache-2.0」と述べるが、
  ACP への言及、著作権者名、ライセンス本文への導線は無い。
- `Cargo.toml:6,9,12` は PoC 自身を AGPL-3.0-or-later とし、
  直接依存に Apache-2.0 クレート 2 つを指定する。

### 依存側（ローカル）

- gpui 0.2.2（`Cargo.lock:2211-2214`）:
  `Cargo.toml.orig:9` が `license = "Apache-2.0"`、`:5` が
  `Nathan Sobo <nathan@zed.dev>`、`:7` が
  `https://github.com/zed-industries/zed`。
  `LICENSE-APACHE:1` に
  `Copyright 2022 - 2025 Zed Industries, Inc.` があり、
  同ファイルに Apache-2.0 本文（全 222 行）が含まれる。
  `.crate` アーカイブにも同ファイルのみで、NOTICE ファイルは無い。
- agent-client-protocol 0.11.1（`Cargo.lock:24-27`）:
  正規化 `Cargo.toml:16` が `Zed <hi@zed.dev>`、`:33` が
  `license = "Apache-2.0"`、`:34` が
  `https://github.com/agentclientprotocol/rust-sdk`。
  ただしローカル展開ディレクトリと `.crate` アーカイブの両方に
  LICENSE / NOTICE は存在しない（`tar tf` で確認）。
  `README.md:56` に Apache-2.0 への言及はある。
- agent-client-protocol-schema 0.12.0:
  ローカル `LICENSE:178` に
  `Copyright 2025 Zed Industries, Inc. and contributors` がある。

### 上流（外部情報、参考）

- <https://raw.githubusercontent.com/agentclientprotocol/rust-sdk/main/LICENSE>
  に Apache-2.0 本文と
  `Copyright 2025 Zed Industries, Inc. and contributors` を確認した。
- GitHub MCP では `zed-industries/zed` と `agentclientprotocol/rust-sdk` の
  既定ブランチ・ルートの `NOTICE` は 404 だった。
  一方、後者のルート `LICENSE` は存在した。

## 不足の具体的列挙

- **G-1 Apache-2.0 本文が未同梱**: リポジトリ内の LICENSE は AGPL-3.0、
  THIRD_PARTY_NOTICES.md は MIT のみ。受領者へ渡す Apache-2.0 写しが無い。
  関連条項は Apache-2.0 §4(a)。配布形態次第で要否は変わる。
- **G-2 gpui の著作権表示が未掲載**:
  `Copyright 2022 - 2025 Zed Industries, Inc.` が
  THIRD_PARTY_NOTICES.md / README.md のいずれにも無い。
  関連条項は §4(c) の帰属通知保持。
- **G-3 ACP の著作権表示が未掲載**:
  `Copyright 2025 Zed Industries, Inc. and contributors` が
  リポジトリ内に無い。ACP 0.11.1 の crate 自体に LICENSE が無いため、
  ローカルのみでは著作権者を確定できない点も併せて記録する。
- **G-4 THIRD_PARTY_NOTICES.md の実体と文件名の不一致**:
  内容は自リポジトリの旧 MIT のみで、gpui / ACP / schema の
  サードパーティエントリが 1 件も無い。
- **G-5 README ライセンス節の記載不足**:
  gpui にしか触れず、ACP のライセンスと著作権者、および
  G-1 のライセンス写しへの導線が欠落している。

## NOTICE ファイルについて

ローカルの gpui / ACP クレートと、上流既定ブランチのルートには
NOTICE ファイルが見つからなかった。この根拠だけでは Apache-2.0 §4(d) の
NOTICE 複製義務が発動するとは言えない。

ただし次は未確認であり、不足なしと断定しない。

- gpui 0.2.2 が公開された時点のリビジョンの状態。
- ルート以外のディレクトリや配布アーカイブの状態。
- 上流が将来 NOTICE を追加した場合の追従。

## 充足が確認できた点

- gpui / ACP の license メタデータは Apache-2.0 と正確に宣言されている。
- Cargo.lock の固定バージョンは Cargo.toml の指定と一致する。
- ACP schema 0.12.0 はローカルに Apache-2.0 本文と著作権表示を保持する。
- 本タスクでは既存ファイルを変更していない。

## 未確認事項

- 配布形態（ソース / バイナリ / `strip = true` のリリース）と
  Apache-2.0 上の「Derivative Work」該当性の法的判断。
- ACP 0.11.1 の crate に LICENSE が同梱されない理由。
- 推移依存を含む全パッケージの Apache-2.0 通知充足状況
  （本タスクは gpui / ACP に限定）。
- 既存調査 `investigation/round-3.md` の C-4
  「PoC に third-party notices が無い」という指摘の是正履歴。

## 提案（法的判断ではない）

- THIRD_PARTY_NOTICES.md に gpui / ACP / schema のエントリを追加し、
  著作権表示とライセンス本文の所在を明記する。
- Apache-2.0 本文を配布物へ同梱し、README から参照する。
- 公開対象リビジョンの上流 NOTICE を再確認し、存在すれば複製する。
- 上記の要否について法務レビューを経てから反映する。

なお既存ファイル変更は本タスクの禁止事項のため、ここでは未実施である。

## 検証

既定設定（MD013 は 80 文字）で検証し、2 ツールともエラー 0 を確認した。

- `markdownlint-cli2 v0.24.1`（markdownlint v0.42.1）:
  `0 issues`、exit code 0。
- `markdownlint 0.50.0`: 出力なし、exit code 0。

```bash
markdownlint-cli2 investigation/license-notice-check.md
markdownlint investigation/license-notice-check.md
```

## 参照

- ローカル: `THIRD_PARTY_NOTICES.md`, `README.md`, `LICENSE`, `Cargo.toml`
- ローカル: `Cargo.lock`, `investigation/round-3.md`
- 依存クレート: `$CARGO_HOME/registry/src/` 配下の
  `gpui-0.2.2`, `agent-client-protocol-0.11.1`,
  `agent-client-protocol-schema-0.12.0`
- 上流: <https://github.com/zed-industries/zed>
- 上流: <https://github.com/agentclientprotocol/rust-sdk>
- Apache-2.0 条文: <https://www.apache.org/licenses/LICENSE-2.0>

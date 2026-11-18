---
type: Reference
title: "依存脆弱性の到達性トリアージ（dsh-gpui-client-poc）"
description: >-
  cargo audit / cargo tree で 26 件の advisory の到達性を調査。
  到達 15 件・非到達 11 件を特定し、rmcp RUSTSEC-2026-0462 を
  最優先とする対処方針を記録する。
tags:
  - investigation
  - dependencies
  - security
  - rust
generated: 2026-11-18T15:00:00+09:00
verified: 2026-11-18T15:00:00+09:00
status: current
---

## 結論

- `cargo audit` の error 26 件のうち、**到達 15 件 / 非到達 11 件**。
- 最優先は到達する `rmcp 1.8.0` の `RUSTSEC-2026-0462`（6.5 medium）。
- ただし脆弱コードパス自体は feature 未有効（詳細は後述）。
- `Cargo.toml` / `Cargo.lock` は変更していない。

## 調査方法（Runtime Proof）

実行環境: 本リポジトリ、2026-11-18T14:52〜14:55+09:00。

```bash
cargo audit
cargo tree -i <crate>
cargo tree -i <crate> --target all
cargo tree -e features -i rmcp@1.8.0
```

- `cargo audit` 0.23.0: advisory DB 1527 件 / スキャン 733 crates。
  exit 1、**26 vulnerabilities / 23 allowed warnings**。
- advisory DB head:
  `233155b4025b854c68a4b199c0e32d7fd53291a2`
  （2026-11-17T11:14:35+01:00）。
- 判定基準: `cargo tree -i` で本クレート
  `dsh-gpui-client-poc v0.1.0` まで逆依存が繋がれば「到達」、
  `nothing to print` なら「非到達」。
- 非到達 3 クレートは `--target all` でも再確認した。
- **注意**: ここでの到達は「クレート単位」であり、
  「脆弱なコードパスが実行される」ことを意味しない。

## 内訳

| クレート | advisory 数 | 到達性 |
| --- | --- | --- |
| quinn-proto 0.11.13 | 8 | 非到達 |
| rustls-webpki 0.103.7 | 5 | 到達 |
| rustls 0.23.33 | 4 | 到達 |
| quick-xml 0.37.5 | 2 | 到達 |
| quick-xml 0.30.0 | 2 | 非到達 |
| rmcp 1.8.0 | 1 | 到達（最優先） |
| quinn-udp 0.5.14 | 1 | 非到達 |
| bytes 1.10.1 | 1 | 到達 |
| crossbeam-epoch 0.9.18 | 1 | 到達 |
| h2 0.4.12 | 1 | 到達 |

合計 26 件（到達 15 = 5+4+2+1+1+1+1、
非到達 11 = 8+2+1）。

## 最優先: rmcp RUSTSEC-2026-0462

- 内容: Streamable HTTP clients のレスポンスボディが無制限に
  バッファされる（DoS / メモリ枯渇）。CVSS 6.5 medium。
- advisory: <https://rustsec.org/advisories/RUSTSEC-2026-0462>
- 到達経路（Runtime Proof）:
  `cargo tree -i rmcp@1.8.0` →
  `agent-client-protocol v0.11.1` → `dsh-gpui-client-poc v0.1.0`。
- 影響を受けるコードは
  `transport-streamable-http-client-reqwest` と
  `transport-streamable-http-client-unix-socket`。
- 有効 feature（`cargo tree -e features -i rmcp@1.8.0`）:
  `base64`, `default`, `macros`, `schemars`, `server`,
  `transport-async-rw`, `transport-io`。
  上記 2 feature は **含まれない**（Runtime Proof）。
- rmcp 1.8.0 の source では当該モジュールが
  `#[cfg(feature = "transport-streamable-http-client")]`
  および
  `#[cfg(feature = "transport-streamable-http-client-reqwest")]`
  でゲートされている（ローカルレジストリ確認）。
- ACP 0.11.1 は `rmcp = "1.2.0"`
  （features: `server`, `transport-io`, `schemars`）を要求。
  修正版は `>=3.9.0` で major が異なる。

### 対処方針（P0）

1. ACP の新版が rmcp 3.x へ移行するかを最優先で追跡する。
2. 移行までは上記 feature を **有効化しない構成を維持**し、
   信頼できる MCP サーバのみに接続する（advisory の回避策）。
3. CI で `cargo audit` ゲートを有効化する際は、
   本 triage の結論を根拠に ignore/allow を判断する。
   ignore する場合も「feature 未有効」の根拠を残す。

## 到達 15 件の内訳と対処方針

### bytes / crossbeam-epoch / h2

- `bytes 1.10.1` — `RUSTSEC-2026-0007`
  （`BytesMut::reserve` の整数オーバーフロー）。
  - 経路: tokio / h2 / `gpui_http_client` → gpui → 本クレート。
  - 修正: `>=1.11.1`。
- `crossbeam-epoch 0.9.18` — `RUSTSEC-2026-0204`。
  - 経路: crossbeam-deque → rayon-core → image/EXR → gpui。
  - 修正: `>=0.9.20`。
- `h2 0.4.12` — `RUSTSEC-2026-0258`（空 DATA フレーム）。
  - 経路: hyper → zed-reqwest → `gpui_http_client` → gpui。
  - 修正: `>=0.4.16`。
- 方針: いずれも推移依存。`cargo update` での更新を
  別タスクで実施する（本タスクは lock を変更しない）。

### quick-xml 0.37.5（2 件）

- `RUSTSEC-2026-0195`（7.5 high, NsReader の名前空間確保）、
  `RUSTSEC-2026-0194`（7.5 high, 属性重複チェックの二次時間）。
- 経路: `wayland-scanner v0.31.7` (proc-macro) →
  `wayland-client` → gpui → 本クレート。
- 修正: `>=0.41.0`。Wayland のビルド時コード生成のみで
  実行時パスではない（推測: proc-macro である点からの判断）。
- 方針: 優先度は中。gpui / wayland-scanner の更新に追随する。

### rustls 0.23.33（4 件）/ rustls-webpki 0.103.7（5 件）

- rustls: `RUSTSEC-2026-0285` (5.3), `0348` (3.7),
  `0349` (3.7), `0346` (4.0)。
  修正: `>=0.23.45` / `>=0.23.46`。
- rustls-webpki: `RUSTSEC-2026-0098`, `0104`, `0049`,
  `0099`, `0382`。
  修正: `>=0.103.10` 〜 `>=0.103.16`（advisory ごとに異なる）。
- 経路: `hyper-rustls` / `zed-reqwest` →
  `gpui_http_client` → gpui → 本クレート。
- 方針: 推移依存の更新で解消可能。HTTPS 通信を行う
  `gpui_http_client` 経由のため、rmcp に次ぐ優先度で対応する。

## 非到達 11 件

- `quinn-proto 0.11.13` ×8:
  `RUSTSEC-2026-0524`, `0037` (8.7 high), `0338`, `0340`,
  `0185`, `0523`, `0342`, `0339`。
- `quinn-udp 0.5.14` ×1: `RUSTSEC-2026-0538`。
- `quick-xml 0.30.0` ×2: `RUSTSEC-2026-0195`, `0194`。
- いずれも `cargo tree -i` および `--target all` で
  `nothing to print`（Runtime Proof）。
- 最大 Severity 8.7 の `RUSTSEC-2026-0037` も非到達。
- 方針: 現状は対応不要。lock 更新で quinn 系が
  到達するようになった場合は再 triage する。

## 推測・未確認

- [推測] ACP の新版が rmcp 3.x に対応する時期は未確認。
- [推測] `cargo update` で各推移依存が更新可能かは未検証。
  Cargo.lock を変更しないため実行していない。
- [未確認] 脆弱コードが最終バイナリに含まれるかは、
  feature gate の source と `cargo tree` からの判断であり、
  バイナリ解析では未検証。
- [未確認] `warning` 23 件の到達性は今回の対象外
  （error 26 件のみを triage）。
- 補足: tokio 1.48.0 の `RUSTSEC-2026-0386` は `warning: unsound`
  で error 26 件には含まれない。`Cargo.toml:14` の `=` ピンが
  修正をブロックするが、PoC の `src/` に `AsyncFd` 参照は 0 件
  （grep Runtime Proof）。CI を赤にしないが、
  AsyncFd 導入時は修正必須。

## 検証結果

- `markdownlint investigation/dependency-triage.md`:
  exit 0、警告 0 件（2026-11-18 実測）。
- `git status --short -- Cargo.toml Cargo.lock`: 出力なし。
- `git diff -- Cargo.toml Cargo.lock`: 出力なし。
- 書き込みは本ファイル 1 ファイルのみ。

## 参考

- 先行調査: `investigation/round-4.md` C 節、
  `investigation/final.md`、`todo.md`。
- advisory: <https://rustsec.org/advisories/RUSTSEC-2026-0462>

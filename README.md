# DSH GPUI Client PoC

[![CI](https://github.com/Seduce0068/dsh-gpui-client-poc/actions/workflows/ci.yml/badge.svg)](https://github.com/Seduce0068/dsh-gpui-client-poc/actions/workflows/ci.yml)

リポジトリ: <https://github.com/Seduce0068/dsh-gpui-client-poc>

GPUI（`gpui = "=0.2.2"`）で DSH 風のチャットクライアント UI を描き、
ACP（Agent Client Protocol）経由で `dsh --profile acp` に接続する PoC です。

## バイナリ

| バイナリ | 内容 |
| --- | --- |
| `dsh-gpui-client-poc` | GPUI による 2 ペイン（セッション一覧 + 会話）の静的 UI |
| `acp_probe` | `initialize` と `session/new` のみを叩く接続確認 |
| `acp_prompt_probe` | 実際にプロンプトを送り、stop reason を表示する確認 |
| `cordis_bridge` | `cordis-rs` runtime 上で `command-compact` をマウントし、登録コマンドを表示するヘッドレス確認 |

## ビルドと実行

```bash
cargo build --release
./target/release/dsh-gpui-client-poc
./target/release/acp_probe
./target/release/acp_prompt_probe "1+1は？"
./target/release/cordis_bridge
```

## 確認済み（2026-11-16、Kubuntu 2026.04 / Wayland）

- GPUI の静的 UI：`cargo build --release` 成功。
- ACP 接続：`dsh --profile acp` を Rust から stdio 起動し、
  `agent_info = deepseek-harness-acp 0.0.1` を取得、`session/new` 成功。
- プロンプト発行：ACP レイヤまで成功。モデル側が残高不足を返したため
  回答取得は未確認（接続問題ではない）。

## 依存バージョンの注意

最新 crate をそのまま追加すると依存衝突でビルドが壊れる。
次のピン留めが必要（詳細は調査レポート §4.4）。

- `tokio = "=1.48.0"`, `tokio-util = "=0.7.16"`
- `libc = "=0.2.177"`, `mio = "=1.1.0"`
- `agent-client-protocol = "0.11.1"`

## 未実装

- GPUI の `Render` と ACP の Tokio セッションの統合。
- `session/update` のストリーミング表示、テキスト入力、Markdown 描画、
  テーマ切替、locale。

## ライセンス

MIT（LICENSE を参照）。上流 gpui は Apache-2.0 ですが、本 PoC のコードは MIT で公開します。

## 方針

詳細は `passkey-gateway/docs/dsh-client-gpui-migration-2026-11-16.md` を参照。
ユーザーにより案C（既存 Web 併存 + ACP 経由の GPUI ネイティブクライアント）
が選択済み。

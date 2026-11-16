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
| `cordis_todo_ui` | `dsh-tool-todo` port をマウントし、`todos` projection を GPUI ウィンドウに描画する。ボタンから `todo_write` を呼ぶ |
| `cordis_fs_ui` | `dsh-tool-fs` port でファイルを読み、GPUI ウィンドウに表示する（パスは第1引数） |
| `cordis_agent_ui` | scripted LLM で agent-loop を回し、tool-fs/bash の実行結果を GPUI ウィンドウに表示する |
| `cordis_prompt_ui` | キーボード入力でプロンプトを打ち、Enter で送信できる GPUI デモ |
| `cordis_prompt_agent_ui` | Enter で agent-loop を非同期実行し、応答を GUI に表示するデモ |
| `acp_llm_probe` | `dsh --profile acp` を実 LLM backend として cordis-rs に接続する probe |
| `acp_multi_turn_probe` | 1つの ACP session に複数 prompt を送り、session が維持されることを確認する probe |
| `cordis_composition_ui` | 実 DSH `cordis.yml` を読み込み、mount された fiber 一覧を GPUI に表示する |
| `cordis_composition_toggle_ui` | クリックでプラグイン fiber を unload / 再 mount するデモ |
| `cordis_entries_toggle_ui` | 実 composition の全エントリを個別に unload / 再 mount し、状態を `/tmp/cordis-entries-toggle.json` に永続化するデモ |

## ビルドと実行

```bash
cargo build --release
./target/release/dsh-gpui-client-poc
./target/release/acp_probe
./target/release/acp_prompt_probe "1+1は？"
./target/release/cordis_bridge
./target/release/cordis_todo_ui   # GUI ウィンドウが開く
./target/release/cordis_fs_ui ./README.md
./target/release/cordis_agent_ui   # agent-loop のステップを表示
./target/release/cordis_prompt_ui  # プロンプト入力（Enter で送信）
./target/release/cordis_prompt_agent_ui  # 入力 → agent-loop → 応答表示
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

## 確認済み（2026-11-17）

- `cordis_todo_ui`: `cargo test --bin cordis_todo_ui` で plugin → tools seam → todos projection の経路を検証。
- 実機では Wayland ネイティブでウィンドウが開く（Xwayland の `xwininfo` には出ないため、目視はユーザー確認が必要）。

### GUI の目視確認（Kubuntu / Wayland）

エージェント環境には `kdotool` が無く、Wayland ネイティブのウィンドウを
自動列挙できません。次の手順で確認してください。

```bash
echo '=== cordis_todo_ui を起動 ==='
cd /home/koppe/document/deepseek-harness/default-workspace/work/dsh-gpui-client-poc
./target/debug/cordis_todo_ui

echo '=== 標準エラー出力の確認（別ターミナル） ==='
# "cordis_todo_ui: GPUI window created" が出ていればウィンドウ作成成功
```

ウィンドウが見えたら `+ prepare step` / `start` / `finish` / `clear` を
押してください。押すと `todo_write` が cordis-rs の `tools` seam 経由で
呼ばれ、リストとステータス行数が更新されます。

## ACP バックエンド接続（2026-11-17）

`acp_llm_probe` で `dsh --profile acp` を `from_async_text` provider 経由で
agent-loop に接続できることを確認しました。ACP の initialize / session/new /
session/prompt は成功し、agent-loop まで到達しています。モデル側の
`Insufficient Balance` により回答本文は取得できませんでした（接続問題ではない）。

## ACP マルチターン（2026-11-17）

`acp_multi_turn_probe` で `initialize` / `session/new` を1回だけ行い、
2つの prompt を同じ session id に送れることを確認しました。モデル側の
`Insufficient Balance` で応答本文は取得できませんが、prompt が同じ session
に届くことは stop reason から検証できます。

## 実 composition 統合（2026-11-17）

`cordis_composition_ui` で `snapshots/sdk/text-turn/cordis.yml` を読み込み、
6つのエントリすべてを fiber として起動して一覧を GUI に表示できます。
cordis-rs 依存は `v0.29.0` に更新済みです。

## 未実装

- GPUI の `Render` と ACP の Tokio セッションの統合。
- `session/update` のストリーミング表示、テキスト入力、Markdown 描画、
  テーマ切替、locale。

## ライセンス

MIT（LICENSE を参照）。上流 gpui は Apache-2.0 ですが、本 PoC のコードは MIT で公開します。

## 方針

詳細はローカルの `passkey-gateway/docs/dsh-client-gpui-migration-2026-11-16.md` を参照（非公開資料のため GitHub には含まない）。
ユーザーにより案C（既存 Web 併存 + ACP 経由の GPUI ネイティブクライアント）
が選択済み。

# DSH GPUI Client PoC

[![CI](https://github.com/Seduce0068/dsh-gpui-client-poc/actions/workflows/ci.yml/badge.svg)](https://github.com/Seduce0068/dsh-gpui-client-poc/actions/workflows/ci.yml)

リポジトリ: <https://github.com/Seduce0068/dsh-gpui-client-poc>

GPUI（`gpui = "=0.2.2"`）で DSH 風のチャットクライアント UI を描き、
ACP（Agent Client Protocol）経由で `dsh --profile acp` に接続する PoC です。

## バイナリ

<!-- 表のセルは折り返せないため MD013 をこの範囲のみ無効化 -->
<!-- markdownlint-disable MD013 -->

| バイナリ | 内容 |
| --- | --- |
| `dsh-gpui-client-poc` | GPUI による 2 ペイン（セッション一覧 + 会話）の静的 UI |
| `acp_probe` | `initialize` と `session/new` のみを叩く接続確認 |
| `acp_prompt_probe` | 実際にプロンプトを送り、stop reason を表示する確認 |
| `cordis_bridge` | `cordis-rs` runtime 上で `command-compact` をマウントし、登録コマンドを表示するヘッドレス確認 |
| `cordis_todo_ui` | `dsh-tool-todo` port をマウントし、`todos` projection を GPUI ウィンドウに描画する。ボタンから `todo_write` を呼ぶ |
| `cordis_fs_ui` | `dsh-tool-fs` port でファイルを読み、GPUI ウィンドウに表示する（パスは第1引数） |
| `cordis_agent_ui` | scripted LLM で agent-loop を回し、tool-fs/bash の実行結果を GPUI ウィンドウに表示する |
| `cordis_prompt_ui` | キーボード入力と送信表示のみの GPUI デモ（AgentLoop は実行しない。agent-loop は `cordis_prompt_agent_ui`） |
| `cordis_prompt_agent_ui` | Enter で agent-loop を非同期実行し、応答を GUI に表示するデモ |
| `acp_llm_probe` | `dsh --profile acp` を実 LLM backend として cordis-rs に接続する probe |
| `acp_multi_turn_probe` | 1つの ACP session に複数 prompt を送り、session が維持されることを確認する probe |
| `acp_stream_probe` | ACP の `session/update` をリッスンして AgentMessageChunk を表示する probe |
| `acp_stream_ui` | ACP の chunk を共有バッファ経由で GPUI ウィンドウにリアルタイム描画するデモ |
| `cordis_permission_ui` | permission preset をクリック切替し、approval gate 付き tool の allow/deny を表示するデモ |
| `cordis_retry_ui` | ローカル stub の構造化 LLM エラー（HTTP 429）を cordis-rs agent-loop の retry で回復させるデモ。ACP / `dsh --profile acp` とは無関係 |
| `cordis_title_ui` | LLM 生成タイトルと first-message fallback を sessionTitles から表示するデモ |
| `cordis_composition_ui` | 実 DSH `cordis.yml` を読み込み、mount された fiber 一覧を GPUI に表示する |
| `cordis_composition_toggle_ui` | クリックでプラグイン fiber を unload / 再 mount するデモ |
| `cordis_entries_toggle_ui` | 実 composition の全エントリを個別に unload / 再 mount し、状態を XDG state home 配下の `cordis-entries-toggle/user-<user>` / `pid-<pid>` に永続化するデモ |

<!-- markdownlint-enable MD013 -->

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
./target/release/cordis_prompt_ui  # 入力と送信表示のみ（AgentLoop なし）
./target/release/cordis_prompt_agent_ui  # 入力 → agent-loop → 応答表示
```

## 確認済み（2026-11-16、Kubuntu 2026.04 / Wayland）

- GPUI の静的 UI：`cargo build --release` 成功。
- ACP 接続：`dsh --profile acp` を Rust から stdio 起動し、
  `agent_info = deepseek-harness-acp 0.0.1` を取得、`session/new` 成功。
- プロンプト発行：ACP レイヤまで成功。モデル側が残高不足を返したため
  回答取得は未確認（接続問題ではない）。

## 依存バージョンの注意

`Cargo.toml` の直接依存のうち、依存衝突を避けるため厳密に固定しているのは
`gpui = "=0.2.2"`, `tokio = "=1.48.0"`, `tokio-util = "=0.7.16"` と
`agent-client-protocol = "0.11.1"` です。`cordis-rs` は Git tag で固定し、
現行は `v0.426.0` です。

`libc = 0.2.177` と `mio = 1.1.0` は直接依存として宣言していません。
`cargo tree --offline -i libc` / `-i mio` で確認したところ、両者は gpui / tokio
などの推移依存であり、`Cargo.lock` で当該バージョンに固定されています。
直接依存として追加しても lock がある限り解決結果は変わらず、コードから
未使用の依存が増えるだけなので不要と判断しました（調査レポート §4.4 は
非公開資料でリポジトリ内には無いため、根拠は manifest と lock に置きます）。

## 依存監査（cargo-audit）の受容判断

CI の `audit` は無効化しています。`cargo audit` は GPUI 系の推移依存に
26 件の既知 advisory（主に `quick-xml` / `quinn-proto` / `h2` 等）を報告します。
これらは `gpui = "=0.2.2"` の推移依存であり、PoC の直接コードでは当該 API を
使用していません。また `tokio = "=1.48.0"` の RUSTSEC-2026-0386 は
`AsyncFd::*` の I/O safety 違反ですが、当リポジトリと cordis-rs の双方で
`AsyncFd` は未使用です。修正版 tokio は mio 1.2 / libc 0.2.183 の連鎖更新を
要求し、`zed-async-tar` の `xattr = "^0.2"` 制約と衝突してビルドが壊れます。
したがって pin を維持し、監査結果は受容した上で記録しています。
`AsyncFd` の使用を開始した場合、または gpui スタックを更新する場合は、
この判断を再評価して `audit: true` に戻してください。

`cargo update` で lock を再生成すると両クレートが更新され、
`xattr` / `ENOATTR` 周りでビルドが壊れる可能性があります。lock は勝手に
更新せず、更新時はビルドとテストで確認してください。

## 確認済み（2026-11-17）

- `cordis_todo_ui`: `cargo test --bin cordis_todo_ui` で plugin → tools seam →
  todos projection の経路を検証。
- 実機では Wayland ネイティブでウィンドウが開く。Xwayland の `xwininfo` には
  出ないが、`spectacle` の全画面撮影には写り、DSH の `read_image` で
  エージェント自身が実描画を確認できる。

### GUI の実描画確認（Kubuntu / Wayland）

旧手順の「`kdotool` が無いため目視はユーザー確認が必要」という記述と、
`stderr` の `GPUI window created` を grep する成功判定は **obsolete** である。
Wayland ネイティブウィンドウは `kdotool` や `xwininfo` で列挙できないが、
`spectacle 6.6.6` の非対話撮影なら写る（実証済み）。取得した PNG を
DSH の `read_image` ツールで開き、エージェント自身が実描画を確認する。

```bash
echo '=== ビルド ==='
cd /home/koppe/document/deepseek-harness/default-workspace/work/dsh-gpui-client-poc
cargo build --bins

echo '=== 証跡取得（各デモの全画面 PNG を EVIDENCE_DIR へ） ==='
bash scripts/smoke-gui.sh

echo '=== 証跡の確認（エージェントが read_image を呼ぶ） ==='
# スクリプト末尾に表示された EVIDENCE_DIR 配下の
# <bin>.png を 1 枚ずつ read_image で開く
```

`smoke-gui.sh` は外部依存のないデモのみを対象にする。以下は個別に起動する。

- `cordis_entries_toggle_ui`: `DEEPSEEK_API_KEY` または `--api-key` が必要。
- `acp_stream_ui`: 絶対パスの `dsh` バイナリ（`DSH_BIN`）が必要。

これらをスクリプトに含めると、認証や `dsh` が無い環境で `set -e` により
途中停止するため、手動起動に委ねる。

`read_image` で次を確認する（例: `cordis_todo_ui`）:

- タイトル `cordis-rs x GPUI — tool-todo`
- 本文 `No todos yet. Use the buttons below to call todo_write.`
- ボタン `+ prepare step` / `start` / `finish` / `clear`
- 下部ステータス `Ready`

ログの有無やプロセス生存は失敗時の診断材料にとどめ、
**成功判定には使わない**。描画が無い・期待文字列が無い場合は FAIL とする。
ボタン押下後の更新まで確認する場合は、ウィンドウを起動したまま
`+ prepare step` → `start` → `finish` を操作し、再度 `spectacle` で撮影して
`read_image` で差分を確認する。

#### 全画面 PNG のクロップ / マスキング方針

`spectacle -b -n -o` は全画面（Kubuntu 環境では 5120x1440）を撮るため、
対象ウィンドウ以外（ブラウザ、チャット、ファイル名など）が写り込む。
証跡を共有・添付する際は次の方針を守る。

1. 共有用画像は対象ウィンドウの領域へクロップする。
   `magick <input>.png -crop <WxH+X+Y> +repage <output>.png`
   座標は `read_image` で確認した画面から求める。
2. クロップが困難な場合は不要領域を単色矩形でマスクする。
   `magick <input>.png -fill black -draw 'rectangle X,Y X2,Y2' <output>.png`
3. 操作履歴・個人情報・トークン等が写る領域は必ずマスクし、
   元の全画面 PNG を報告へ添付しない。
4. クロップ / マスキング後も `read_image` で再確認し、
   対象 UI が判別できることと不要情報が消えていることを確かめる。
5. 元の全画面 PNG はローカルの `EVIDENCE_DIR` にのみ置き、
   確認後は必要に応じて削除する。共有物には加工済み画像だけを使う。

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

`cordis_composition_ui` で `fixtures/text-turn.cordis.yml` を読み込み、
port が存在するエントリのみを fiber として起動し、failed / skipped を一覧表示します。
cordis-rs 依存は `Cargo.toml` の Git tag で同期済みです（現行は `v0.377.0`）。

## 未実装 / 未確認

- `dsh-gpui-client-poc`（静的シェル）は ACP に接続していません。ACP 接続は
  `acp_*` probe と `acp_stream_ui` などの個別デモに分離されています。
- ACP `session/update` のうち `AgentMessageChunk` のテキストに加え、
  `ToolCall` / `ToolCallUpdate` / `AgentThoughtChunk` / `ConfigOptionUpdate`
  を描画します。`UsageUpdate` は未対応のため ignored 理由を表示します。
  ストリーム表示も共有バッファのポーリング経由であり、
  GPUI の `Render` と Tokio セッションを単一ランタイムで直接統合した
  わけではありません。
- `cordis_prompt_ui` はキーボード入力と送信表示のみで AgentLoop は
  実行しません。AgentLoop の実行は `cordis_prompt_agent_ui` を使います。
- Markdown 描画、テーマ切替、locale / i18n は未実装です（対応コードなし）。
- 実 LLM の応答本文は未確認です（モデル側の残高不足）。ACP の
  `initialize` / `session/new` / `session/prompt` までは到達済みです。

## ライセンス

`AGPL-3.0-or-later`（[LICENSE](LICENSE) を参照）。上流 gpui は Apache-2.0 ですが、
本 PoC のコードは `AGPL-3.0-or-later` で公開します。以前の MIT 本文は
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) に保持しています。

## 方針

詳細はローカルの
`passkey-gateway/docs/dsh-client-gpui-migration-2026-11-16.md` を参照
（非公開資料のため GitHub には含まない）。
ユーザーにより案C（既存 Web 併存 + ACP 経由の GPUI ネイティブクライアント）
が選択済み。

## ローカル検証

```bash
cargo build --bins

# 各 GPUI デモの実画面 PNG を取得（ディスプレイ必要）。
# 成功判定はログ grep ではなく、出力された各 PNG の
# read_image 確認による（上記「GUI の実描画確認」を参照）。
bash scripts/smoke-gui.sh
```

`acp_*` probe は外部の `dsh --profile acp` とモデル残高に依存するため、
このスクリプトには含めていません。

---
type: Reference
title: オンライン staged-B の実行可否確認
description: >-
  tests-stage-b-online について、実 LLM への課金送信を行わずに実行可否を確認した結果。
  認証情報の有無、残高確認手段、acp_probe のハンドシェイク結果と Root Cause、
  および実行不能の場合に必要なユーザー操作を記録する。
tags:
  - dsh
  - acp
  - stage-b
  - investigation
generated: 2026-11-18T17:28:41+09:00
verified: 2026-11-18T17:28:41+09:00
status: completed
---

## 目的と範囲

対象は `todo.md:109` の `tests-stage-b-online`（G5）である。「モデル残高回復後に
`acp_prompt_probe` を1回実行し、本文非空と stop reason を記録する」という作業を、
**実 LLM への課金送信なしで**開始できるかどうかを判定する。

実行したのは次の3項目のみである。

1. 認証情報の有無（値は読まず、キー名と設定長のみ）
2. 残高確認手段の特定と、読み取り専用エンドポイントによる参照
3. `acp_probe` のハンドシェイク（`initialize` + `session/new`、プロンプト送信なし）

既存ファイルは一切変更していない。書き込んだのは本レポート1ファイルのみである。

## 結論

**現状では `tests-stage-b-online` は実行不能**である。ただし ACP 接続層自体は健全で、
障害はモデル側の残高に限定されている。

- 認証情報: **有**（API キーと Platform アカウント grant が保存済み）
- API キーの有効性: **有**（`/models` が HTTP 200 で 2 モデルを返却）
- 残高: **ゼロ**（`is_available=false` / `total_balance=0.00 USD`）
- 推論リクエスト: **HTTP 402 Insufficient Balance**（既存記録）
- `acp_probe` ハンドシェイク: **成功**（exit 0、clean shutdown、課金送信なし）

## 実行した確認と結果

### 1. 認証情報の有無

`~/.dsh-agentrouter/.env` と `~/.dsh-agentrouter/.credentials.yaml` を確認した。
秘密値は読み取らず、キー名と空でないこと（長さ）のみを記録する。

| 保管先 | 項目 | 状態 |
| --- | --- | --- |
| `.env` | `DEEPSEEK_API_KEY` | SET（長さ 35、値は非表示） |
| `.env` | `AGENTROUTER_API_KEY` | SET（長さ 51、値は非表示） |
| `.credentials.yaml` | `kind: grant` レコード | 1件（Platform アカウント grant） |
| `.credentials.yaml` | `refs` | 3件（下記） |

`refs` の内容は `GOOGLE_API_KEY`、`LOGFARE_API_KEY`、`AMD_API_KEY` である。

`acp_probe` が起動する `dsh --profile acp` は
`@deepseek-ai/dsh-base` と `@deepseek-ai/dsh-acp-app` のみを束ね、
`cordis.patch.yml` は空配列である。起動ログに
`[llm-deepseek-api-key] model listing from
https://api.deepseek.com/models loaded 2 model(s)` が出たことから、
API キーが解決され DeepSeek 側で受け付けられていることも確認できた。
`/models` は認証を伴う読み取り専用エンドポイントであり、推論課金は発生しない。

### 2. 残高確認手段

`dsh --help`、`dsh headless --help`、`dsh web --help` を確認したが、
残高やアカウントを照会する CLI サブコマンド・オプションは存在しなかった。
利用可能な手段は次の3通りである。

| 手段 | 種別 | 備考 |
| --- | --- | --- |
| `GET /user/balance` | 読み取り専用 REST | API キー必須・推論ではないため課金なし |
| DSH Web UI のアカウントパネル | Remote API | `getBalance` が recharge / bonus を返す |
| DeepSeek Platform の残高画面 | ブラウザ UI | ユーザーが目視で確認する |

本タスクでは課金送信を避けるため、読み取り専用 REST のみを実行した。
結果は次の通りである。

```text
HTTP_CODE = 200
is_available = False
currency = USD  total = 0.00  granted = 0.00  purchased = None
```

API キーは有効なのに利用可能残高がゼロである。これは「認証失敗」ではなく
「残高枯渇」を直接示す Runtime Proof である。

### 3. acp_probe ハンドシェイク

`cargo build --locked --offline --bin acp_probe` でバイナリを生成し、
`timeout 90s ./target/debug/acp_probe` を実行した（プロンプト送信なし）。

<!-- markdownlint-disable MD013 -->
```text
EXIT_CODE = 0

--- STDOUT ---
INIT OK: agent_info=Some(Implementation { name: "deepseek-harness-acp", title: None, version: "0.0.1", meta: None })
SESSION OK: id=SessionId("f8a1f1c6-283f-427b-8e96-e2fcdc74249c")

--- STDERR ---
cwd = /home/koppe/document/deepseek-harness/default-workspace/work/dsh-gpui-client-poc
dsh: warning: 5 entries did not activate
[llm-deepseek-api-key] model listing from https://api.deepseek.com/models loaded 2 model(s), skipped 0 row(s)
SHUTDOWN = Exited
```
<!-- markdownlint-enable MD013 -->

`src/bin/acp_probe.rs` は `InitializeRequest` と `NewSessionRequest` のみを送り、
`PromptRequest` を送らない実装である。したがってこの実行ではモデル推論も課金も発生していない。
`SHUTDOWN = Exited` は子プロセスが EOF で正常終了したことを示し、
transport の確立・切断ともに問題ない。

`5 entries did not activate` はエージェント preset が
`agentPresets` サービスを待っているという起動時警告で、
ACP ハンドシェイクの成否とは無関係である。

## Root Cause

因果の連鎖は次の通りである。

1. 認証情報は存在し、API キーは有効である（`/models` が HTTP 200）。
   ネットワーク経路も到達している。
2. しかし DeepSeek API の推論リクエストは
   `HTTP 402: Insufficient Balance` を返す（request_id:
   `ee2c0794-e6b0-4d74-9d89-b5de1b27b189`）。
3. 読み取り専用の残高 API が `is_available=false` かつ
   `total_balance=0.00 USD` を返し、残高枯渇を裏付けた。
4. よって `acp_prompt_probe` が `session/prompt` を送っても
   非空の回答本文と妥当な stop reason を取得できない。
5. 結果として `tests-stage-b-online`（G5）の受入条件を満たせない。

これは接続不良や認証情報の欠落ではなく、**残高不足という外部状態**が原因である。
`README.md:139-151` の既存記録（ACP 接続は成功、本文だけが
`Insufficient Balance` で取得不可）と一致する。

## 必要なユーザー操作

stage B を実行可能にするには、エージェントでは行えない課金・認証の操作が必要である。

1. DeepSeek Platform（<https://platform.deepseek.com/>）にログインし、
   `DEEPSEEK_API_KEY` に紐づくアカウントへ残高をチャージする。
   プロモーション残高が付与された場合は `granted_balance` が正の値になる。
2. チャージ後、読み取り専用エンドポイントで `is_available=true` かつ
   `total_balance>0` になったことを確認する。以下は Kubuntu の bash で、
   キー値を画面に出さずに確認する手順である。

   ```bash
   echo "残高確認（Kubuntu / ローカル）"
   env_file="$HOME/.dsh-agentrouter/.env"
   header_file="$(mktemp)"
   umask 077
   key_line="$(grep '^DEEPSEEK_API_KEY=' "$env_file" | head -1)"
   key="$(printf '%s' "$key_line" | cut -d= -f2- | tr -d "\"'")"
   printf 'Authorization: Bearer %s\n' "$key" > "$header_file"
   curl -sS --max-time 25 -H @"$header_file" https://api.deepseek.com/user/balance
   rm -f "$header_file"
   ```

3. 残高回復を確認した後に限り、`tests-stage-b-online` を実行する。
   `acp_prompt_probe` は実プロンプトを送るため課金が発生する。
   実行はユーザーの承認後に親エージェントが行うこと。

なお DSH Web UI を利用できる場合は、アカウントパネルの残高表示
（`ctx.deepseekAccount.getBalance`）でも同じ状態を確認できる。
ただし本セッションのプロファイルは `agentrouter` で、起動した `acp` プロファイルは
API キー経由である点に注意する。

## 実行しなかった操作（安全上の境界）

- `acp_prompt_probe` / `acp_llm_probe` / `acp_multi_turn_probe` の実行
  （いずれも `session/prompt` を送り、推論課金が発生するため）
- `dsh headless "..."` へのタスク投入（推論を伴うため）
- 認証情報の値の読み取り・出力（有無と長さのみを確認）
- 既存の追跡ファイルの変更（本レポートの新規追加のみ）

## 出典とエビデンス

- ローカル Runtime Proof
  - `src/bin/acp_probe.rs`（`PromptRequest` を送らない実装）
  - `target/debug/acp_probe` の実行出力（本レポート「3. acp_probe ハンドシェイク」）
  - `~/.dsh-agentrouter/.env`、`~/.dsh-agentrouter/.credentials.yaml`
  - `~/.dsh-agentrouter/profiles/acp/package.json`
  - DSH インストール版 `docs/subsystems/credentials.md`
  - CLI: `/home/koppe/.npm-global/bin/dsh`（`0.2.2-rc.3`）
- 既存記録
  - `README.md:139-151`（ACP 接続成功・本文のみ取得不可）
  - `investigation/external-research-retry.md:27-30`（HTTP 402 と request_id）
  - `investigation/round-2.md:174`（web_search の 402）
  - `cross-review/cycle-001/round-4.md:15`（残高不足と代替手段）
- 外部エンドポイント
  - <https://api.deepseek.com/user/balance>（読み取り専用、HTTP 200）
  - <https://api.deepseek.com/models>（読み取り専用、2 モデル）
  - <https://platform.deepseek.com/>（チャージ操作はユーザーが実施）

## 補足

- `cargo build --locked --offline --bin acp_probe` により
  `target/debug/acp_probe` を生成した。`.gitignore` の `/target/` 配下であり
  追跡対象外で、既存の追跡ファイルは変更していない。
- 残高照会は推論を伴わない読み取り専用 GET であり、
  「実 LLM への課金送信」には当たらない。
- 残高は外部状態のため、`verified` は本レポート作成時点の値を示す。
  実行前に再度確認する必要がある。

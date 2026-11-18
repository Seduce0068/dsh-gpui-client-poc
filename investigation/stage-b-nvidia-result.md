---
type: Reference
title: "Stage B 実プロンプト検証（NVIDIA 無料モデル）"
description: "DeepSeek 残高待ちを回避し、NVIDIA の無料モデルで ACP 本文と stop reason の取得を実証した記録"
tags: [verification, acp, nvidia, runtime-proof, dsh-gpui-client-poc]
resource: work/dsh-gpui-client-poc
generated: 2026-11-18T19:15:44+09:00
verified: 2026-11-18T19:15:44+09:00
status: verified
---

## 結論

`tests-stage-b-online` は **条件付きで完了**。DeepSeek API への課金送信は行わず、
NVIDIA の無料モデル `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning` を使って
ACP 経由の本文取得と stop reason を実証した。

- `acp_llm_probe`: `steps: 1` / `acp body: 2`（非空） / exit 0
- `acp_prompt_probe`: `stop_reason = EndTurn` / exit 0

## やったこと

1. `https://integrate.api.nvidia.com/v1/models` を読み取り専用で確認し、
   `NVIDIA_API_KEY` が利用可能で `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning`
   が存在することを確認。キー値はレポートにも設定にも書いていない。
2. `fixtures/acp-nvidia-free.patch.yml` を追加。`dsh --profile acp` に
   `--patch` overlay として渡し、次を上書きした。
   - `llm-pi-ai`: `providers.nvidia`（`api: openai-completions`,
     `baseURL: https://integrate.api.nvidia.com/v1`, `apiKeyEnv: NVIDIA_API_KEY`）
   - `agent-default-model` と `acp`: `provider: nvidia` / 上記モデル
3. `acp_llm_probe` と `acp_prompt_probe` に `DSH_ACP_PATCH` 環境変数を追加。
   設定値がある場合だけ `--patch <path>` を渡し、未設定なら従来どおり。
   両ファイルに引数生成の回帰テストを追加済み。
4. 実行コマンド（作業ディレクトリ: `work/dsh-gpui-client-poc`）:

```bash
export DSH_ACP_PATCH="$PWD/fixtures/acp-nvidia-free.patch.yml"
./target/debug/acp_llm_probe "1+1は？"
./target/debug/acp_prompt_probe "1+1は？"
```

## 途中で判明したこと

- 最初に使った `google/gemma-3-12b-it` は NVIDIA API が
  `404 Function ... Not found for account ...` を返し、直接の chat completions
  でも同じだった。この ID はアカウント側で利用不可と判断して差し替えた。
- overlay が `agent-default-model` だけを上書きした最初の試行では、`acp`
  プラグイン自身が `deepseek-official / deepseek-v4-flash` を保持しており
  DeepSeek の 402 が残った。`acp` の設定も上書きして解消した。
- `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning` は `reasoning_content` を
  返す場合があるが、ACP 側の本文としては `2` が取得できた。

## 未確認

- 他モデル（`nvidia/cosmos-reason2-8b` など）の互換性は確認していない。
- DeepSeek 残高は依然 0 USD。`stage-b-readiness.md` の記述はそのまま有効。
- GitHub Actions 上の CI 実行はこのセッションでは確認していない。

## 検証

- `cargo fmt --all -- --check` / `cargo check --all-targets --locked` /
  `cargo test --bins --locked` / `cargo clippy --all-targets --locked -- -D warnings`
  すべて exit 0（Rust 1.88.0）。
- 実プロンプト実行は上記2バイナリで exit 0、本文非空と stop reason を確認。

---
type: Reference
title: サイクル001 ラウンド3 — 敵対的セキュリティ / 運用 / ライセンス
description: 悪用シナリオの切分け、エラー回復導線、ライセンス判断の履歴裏付け
tags:
  - cross-review
  - cycle-001
  - round-3
generated: 2026-11-17T20:35:00+09:00
---

## A. r3-adversarial（敵対的検証）

### `approval-gate-string-only-always-allow`

- category: 残タスク（**実 LLM 接続後の潜在リスク**） / severity: high /
  confidence: high / known: yes
- evidence: cordis-rs `src/services/tools.rs:311-315`,
  `cordis_permission_ui.rs:48-49`
- 実 LLM が `bash` に `{"command":"curl …|sh"}` を渡しても、
  gate が見るのは `"tool:bash"` という文字列のみ。
  説明文は「引数付き」と書くが `args` は request に渡らず、
  引数単位の deny ルールを構造的に書けない。answerer は常 Allow。

### `llm-to-tool-call-parser-absent`（`1` の抑制要因）

- category: 却下根拠（**今は発火しない**） / severity: info / confidence: high
- evidence: `cordis_agent_ui.rs:21-56,122-127`,
  `acp_llm_probe.rs:53,67-85`
- scripted provider が `WRITE`/`READ` 固定で parser も固定 JSON。
  ACP LLM probe は stop reason を `final_text` とし call は空。
  **現状の配線では悪用できない**。これが `1` を「潜在」に格下げする根拠。

### `agent-ui-pipeline-without-approval`

- category: 残タスク（**潜在リスク**） / severity: high / confidence: high / known: no
- evidence: `cordis_agent_ui.rs:110,116-121,125-127`
- `let (tools, _) = provide_tools(...)` で pipeline を破棄し、
  `with_approval` を呼ばず tool-fs / tool-bash を登録。
  parser を実 LLM 用に差し替えた瞬間、gate なしで
  任意パス read/write と任意 shell 実行が通る配線になる。

### `tmp-wipe-parallel-clobber`

- category: 改善（**今すぐ危険**） / severity: medium / confidence: high / known: no
- 2デモ並列起動で後発が先発の状態ディレクトリを消す。
  symlink 追跡手口は `remove_dir_all` が symlink 自体を削除するため
  成立しないと判断（**推測・未実行検証**）。

### `predictable-state-symlink-race`

- category: 改善（**今すぐ危険／同一ユーザー**） / severity: low /
  confidence: low / known: yes
- 予測可能パスへの保存と結果の握り潰し。
  `fs::write` の symlink 追跡は未検証（推測）。

### `dsh-path-hijack-and-stderr-inheritance`

- category: 調査必要（**PATH 汚染時は今すぐ危険**） / severity: medium /
  confidence: medium / known: no
- `Command::new("dsh")` は PATH 解決。引数インジェクションは却下済みだが、
  PATH 先頭に攻撃者の `dsh` を置くとプロンプトがその stdin へ流れる。
  stderr 継承のため GUI では認証エラーも攻撃痕跡も見えない。
  秘密が env 渡しされるかは未確認。

**区別**: 今すぐ危険 = tmp 状態汚染、PATH 汚染時、stderr 隠蔽。
潜在 = approval gate と agent-ui pipeline（実 LLM + parser 接続後に発火）。

## B. r3-ops（運用・保守）

### `acp-error-classification-absent`

- category: エラー分類 / severity: high / confidence: high / known: no
- evidence: `acp_llm_probe.rs:56`, `acp_prompt_probe.rs:48`,
  `acp_stream_ui.rs:71,86,136`
- JSON-RPC エラーと stop reason を区別せず全失敗が汎用文字列。
  実際に遭遇した `Insufficient Balance` に対して回復導線が UI に無く、
  429 / 認証切れ / 切断も同じ表示。

### `retry-ui-unrelated-to-acp`

- category: エラー回復 / severity: high / confidence: high / known: no
- evidence: `cordis_retry_ui.rs:1-5,35-48,52-60`
- この 429 は cordis agent-loop の LLM provider 層の固定文字列。
  ACP の `send_request(...).block_task().await` には retry/backoff が無く、
  「attempts=2 | final=recovered after retry」は実回線と無関係。
  **誤った安心感**を与える。

### `config-hardcoded-runtime-knobs`

- category: 設定外部化 / severity: medium / confidence: high / known: 一部
- プロンプト、パス、API キー、タイムアウト、preset、プロファイル名が全て直書き。
  別バックエンド/アカウント切替には再ビルドが必要。

### `stderr-and-session-trace-blindspot`

- category: ログ/診断 / severity: high / confidence: high / known: no
- stderr 未捕捉、`session_id` は `eprintln!` のみで相関 ID 化されず、
  `open_window` は無題でどのウィンドウがどの子プロセスか追えない。
  PID 保全・再接続手段なし。

### `kill-only-no-session-close`

- category: シャットダウン / severity: medium / confidence: medium / known: no
- `child.kill()` のみで `session/close` なし。
  一部 session 系は unstable feature gate で型自体が無い（ラウンド2 ACP-05）。

### `tmp-fixed-path-multi-instance`

- category: マルチインスタンス / severity: high / confidence: high / known: 一部
- 2つの GUI が同じ home/session/state パスを使い、
  片方の起動がもう片方の履歴を消し得る。
  state JSON は1ファイルで「最後の書き込みが勝つ」上に save 失敗が無視される。

## C. r3-license（ライセンス / ポリシー）

### `mit-consistency-confirmed`

- category: license-consistency / severity: info / confidence: high / known: yes
- `LICENSE:1`, `Cargo.toml:6`, `README.md:117` の3か所すべて MIT で矛盾なし。
  履歴: 初期コミット `bbdaddf` の `LICENSE` は MIT、`Cargo.toml` は当初
  Apache-2.0 で `d70bb23`（"align Cargo.toml license with the MIT LICENSE file"）
  で MIT に修正済み。

### `agpl-default-difference-is-decided-not-violation`

- category: policy-alignment / severity: low / confidence: high / known: yes
- 共通指示 §4 の「個人リポジトリのデフォルトは AGPL-3.0-or-later」に対し、
  本リポジトリは MIT。ただし明示的選択の痕跡（初期 LICENSE + d70bb23 +
  README:117）があるため、**是正指摘ではなく判断済み事項として記録**するのが妥当。
  利用者本人が直接選んだかは git 作者名だけでは判別不能（未確認）。

### `apache-2-deps-notice-unverified`

- category: license-compatibility / severity: low / confidence: high（メタデータ）/
  low（同梱状況） / known: 一部
- cordis-rs は MIT、gpui と `agent-client-protocol` は Apache-2.0
  （ローカル registry `Cargo.toml:33` で確認）。コピーレフト型でなく
  MIT 本体と衝突するメタデータはない。
  ただし Apache-2.0 の著作権・NOTICE 表示義務の充足は未確認。
  README は gpui にのみ言及し ACP の Apache-2.0 は未記載。

### `license-version-note-stale`

- category: documentation-accuracy / severity: low / confidence: high / known: yes
- README:107 の v0.29.0 対 実タグ v0.62.0。
  ライセンス監査の再現性を損なう（ラウンド1 と同一趣旨）。

### `private-design-doc-boundary`

- category: disclosure-governance / severity: low / confidence: high / known: yes
- 資料の存在・非公開性・案C選択済みは README で公開。
  資料本体はリポジトリ外で未読。
  公開 README にローカル相対パスを載せるのは内部構成の開示に当たる。

### `no-ad-payment-code`

- category: policy-alignment / severity: info / confidence: medium / known: yes
- 広告・課金処理コードは確認範囲でなし。
  外部送信は ACP 経由で実在しモデル残高を consume するが
  README:54-55,93-94 が開示しており隠蔽ではない。
  `sk-demo` はダミー値で実秘密ではない。全ファイル全文は未読（未確認）。

## D. ラウンド3 集計

| 出典 | 件数 | 主な貢献 |
| --- | --- | --- |
| r3-adversarial | 6 | 潜在/即時の区別、parser 不在による抑制要因の発見 |
| r3-ops | 6 | エラー分類不在、retry デモの無関係性、診断 브라인드スポット |
| r3-license | 6 | MIT 判断の履歴裏付け、AGPL 差異の正しい扱い、NOTICE 未確認 |

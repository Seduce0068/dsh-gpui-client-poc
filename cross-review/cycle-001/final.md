---
type: Reference
title: サイクル001 ラウンド6 — 重複排除・妥当性評価・最終取りまとめ
description: 5回のクロスレビューで得た検出事項を統合し、妥当性を判定して todo.md へ反映した最終成果物
tags:
  - cross-review
  - cycle-001
  - final
  - consolidation
generated: 2026-11-17T20:50:00+09:00
---

## 実施内容

- 5回のレビュー（`round-1.md` 〜 `round-5.md`）で得た約 120 件の検出事項を統合。
- 根拠元: `investigation/round-1.md`（115件・別タスクのプール）、
  本サイクルのラウンド1〜5、親の Runtime Proof、
  外部一次情報（GitHub REST / crates.io API / raw.githubusercontent.com）。
- **ソースコードの修正は一切行っていない**。成果物は
  `cross-review/` と `todo.md` への追記のみ。
- `todo.md` は既存の雛形（2026-11-17T20:11:47+09:00 作成）を保持し、
  `（なし）` プレースホルダを置き換える形で追記した。

## 統合グループと最終項目

### G1: 検証の偽陽性とストリーミング描画

| id | title | severity | 統合元 | 妥当性 |
| --- | --- | --- | --- | --- |
| `verify-smoke-false-positive` | smoke がログ1行で成功判定し、Render/操作/クラッシュを検出できない | high | A `smoke-log-grep-false-positive`, E `docs-smoke-script-overclaims`, D-3, D-1 | Runtime Proof: `smoke-gui.sh:24-32` の `\|\| true` + grep を確認 |
| `verify-window-not-observed` | 「ウィンドウが開く」は `eprintln!` のみで目視/列挙は未実施 | high | E `docs-window-verification-contradiction`, D-1, B-32 | README:69 対 73-75 の自己矛盾を確認 |
| `stream-update-variants-dropped` | `SessionUpdate` 11種のうち10種、`ContentBlock` 5種のうち4種をドロップ | high | F `session-update-variants-dropped`, E-7, B-18 | crate 一次情報 `client.rs:84` / `content.rs:36` で確認 |
| `stream-polling-and-detach-leak` | 100ms 無限ポーリング + `.detach()` が strong 参照を保持 | medium | D `acp-100ms-polling-via-mutex`, C `stream-ui-fixed-prompt-and-dead-poll`, E-15, R5-05, A-7 | `acp_stream_ui.rs:157-170` を直接確認。実残存は未確認 |
| `stream-fixed-prompt-no-input` | ストリーム UI の送信プロンプトが固定 | medium | C, A-6 | `:77` を確認 |
| `ux-ime-enter` | 日本語 IME の変換確定 Enter が途中送信される | high | R5-01, B-19, B-24 | `cordis_prompt_ui.rs:144-155` を確認。IME 状態 API は未確認 |

**判定**: G1 は「最大の重複グループ」だが像が3つに分かれる。
(a) 検証手法の偽陽性、(b) ストリーミング処理の設計、
(c) 入力基盤（IME）。まとめて1件にすると修正が破綻するため、
todo では別項目とした。

### G2: 承認 / 権限 / ACP agent→client request（2原因を分離）

| id | title | severity | 統合元 | 妥当性 |
| --- | --- | --- | --- | --- |
| `acp-handlers-absent` | ACP 側に permission/fs/terminal の handler が無く `method_not_found` になる | high | F, ACP-01, ACP-03, E-5, E-6 | crate `role/acp.rs:24-25`, `incoming_actor.rs:349` で確認 |
| `cordis-approval-always-allow` | cordis 側 answerer が常 Allow で deny 不能、引数も gate に渡らない | high | B, C, security, r3-1, A-2, F-6, B-34 | `cordis_permission_ui.rs:48-49`, `tools.rs:311-315` で確認 |
| `agent-ui-pipeline-no-gate` | `cordis_agent_ui` が `with_approval` を破棄 | high（潜在） | r3-3 | `cordis_agent_ui.rs:110,116-121` を確認 |
| `tool-fs-unvalidated-path` | パス検証が非空文字列のみで read/write/edit 全権限 | medium | security, F-6 | cordis-rs `tool_fs.rs:29-54` を確認 |
| `tool-call-parser-absent`（抑制要因） | 現在は scripted/stop-reason のため tool call が生成されず悪用不可 | info | r3-2 | **現状のリスク格下げ根拠として採用** |

**判定**: 「permission が動かない」は **ACP 側 handler 不在** と
**cordis 側 answerer 固定** という別原因が混在していた。
単純マージすると誤修正を生むため todo では2項目に分離。
`known_in_readme` の yes/no 反転は
README:27 に「デモの説明」はあるが「欠陥」は無いことを採用し、
最終値は `known_in_readme = yes（欠陥自体は no）` に統一。

### G3: 依存 / バージョン / ドキュメント数値

| id | title | severity | 統合元 | 妥当性 |
| --- | --- | --- | --- | --- |
| `deps-pins-not-in-manifest` | README が必須とする libc/mio ピンが lock のみに存在 | high | A, docs, security, D-5 | `Cargo.toml` に無く `Cargo.lock:3142,3401` にあることを確認 |
| `deps-readme-version-stale`（解消済み） | 当時 README `v0.29.0` 対 pin `v0.62.0` で5タグ差。現行は README:67,157 と Cargo.toml:10 がともに `v0.133.0` | info | docs, D-7, L-04 | 現行 README / Cargo.toml を確認 |
| `deps-upstream-4-tags-behind`（陳腐化） | 当時 pin `v0.62.0` は上流 `v0.66.0` より4タグ/8コミット遅れ。現行 pin は `v0.133.0` | info | round-4 | 現行 Cargo.toml:10 と investigation/round-5.md を確認 |
| `docs-fixture-path-wrong` | `snapshots/sdk/text-turn/cordis.yml` は不存在、実体は `fixtures/` | high | docs, D-13 | 両パスの存在確認済み |
| `docs-unimplemented-list-stale` | README:112 の「未実装」が README:21,26 と自己矛盾 | high | C, D, docs, D-10 | 直接確認 |
| `docs-run-examples-incomplete` | 実行例が9件のみ、release/debug パス混在 | medium | docs, D-4 | 直接確認 |
| `ci-gates-missing` | 共通 workflow に unsafe lint / cargo audit が無い | medium | A `unsafe-gate-unverified`, security, round-2 D | **未確認→確認済みに格上げ**（raw 取得） |
| `license-mit-decided` | MIT は判断済み。AGPL デフォルト違反ではない | info | L-01, L-02 | `bbdaddf` / `d70bb23` の履歴で確認 |
| `license-apache-notice-unverified` | Apache-2.0 依存（gpui, ACP）の NOTICE 同梱は未確認 | low | L-03 | メタデータのみ確認 |

**判定**: 「5タグ遅れ」と「4タグ遅れ」は比較対象が違った
（当時 README 記述 v0.29.0 → pin v0.62.0 が5タグ、
pin v0.62.0 → 上流 v0.66.0 が4タグ）。両方を正しく記録し、
片方を却下しない。2026-11-18 現在は README / Cargo.toml ともに
`v0.133.0` に同期済みで、いずれの「遅れ」も解消している。

### G4: /tmp 状態・設定外部化

| id | title | severity | 統合元 | 妥当性 |
| --- | --- | --- | --- | --- |
| `state-tmp-fixed-paths` | 予測可能な /tmp パスへ状態分散 + 起動時 `remove_dir_all` | high（即時） | A, B, C, D, r3-4, r3-6, r3-ops, A-16, F-9 | 全パスと `composition_ui.rs:203` を確認 |
| `state-multi-instance-clobber` | 並起動で履歴が消え state JSON が最後書き込み勝ち | high | r3-ops, r3-4 | 同上の帰結 |
| `config-hardcoded` | プロンプト/パス/API キー/タイムアウト/preset が直書き | medium | A, security, r3-ops, R-06 | 各 file:line を確認 |
| `cwd-fallback-root` | `current_dir()` 失敗時に cwd が `/` になる | medium | security | 全 `acp_*` を確認 |
| `dsh-path-hijack` | `Command::new("dsh")` が PATH 依存（インジェクションはなし） | low | security, r3-6 | 引数固定を確認 |

### G5: テストカバレッジ

| id | title | severity | 統合元 | 妥当性 |
| --- | --- | --- | --- | --- |
| `tests-18-of-19` | 当時テスト6バイナリのみ。現行は18 bin すべてに `#[cfg(test)]` / `#[test]`、残りは `src/main.rs` のみ | info（解消） | A, r2-test, C-1, D-15, E-18, F-15 | `src/bin/*.rs` 18 ファイルすべてに `#[test]` を確認（2026-11-18） |
| `tests-no-mock-provider` | ACP 本文の検証経路が無い（残高依存） | high | A, r2-02, D-2, A-18 | 既知 |
| `tests-existing-solutions` | `Channel::duplex()` と cordis `echo_provider` が既存 | info | r2-03, r2-04 | crate 一次情報で確認 |
| `tests-isolation` | 実 FS / `/tmp` 固定に依存し並列フレーク要因 | medium | r2-05 | 直接確認 |

**分母の確定**: 18 bin + main = 19 バイナリ。現行は 18 bin すべてにテストが
あり、テストが無いのは `src/main.rs` のみ。
当時の「テスト6・残り13」は obsolete。

### G6: cordis-rs 統合の実質

| id | title | severity | 統合元 | 妥当性 |
| --- | --- | --- | --- | --- |
| `cordis-config-discarded` | 全 factory が `_config` を無視し `Default()` で mount | high | r2-2, F-10 | 14箇所の `move \|_ctx, _config\|` を確認。**原因は本リポジトリ側**と切り分け済み |
| `cordis-services-not-provided` | sessions / systemPrompt / goals / compaction が未 provide | high | r2-4, r2-5, F-11 | cordis-rs `agent_loop.rs` で参照を確認 |
| `cordis-failed-report-ignored` | `MountReport::failed` が両デモで捨てられる | high | r5-gaps R5-01 | cordis-rs `loader.rs:370-375` で確認 |
| `cordis-disabled-bypassed` | `enabled_with()` を使わず YAML の disabled を上書き | high | r5-gaps R5-02 | `with_env_value("DSH_SNAPSHOT_FEEDBACK","1")` を確認 |
| `cordis-entry-state-collapse` | disabled/unknown/明示 false が「unloaded」1つに潰れる | medium | r5-gaps R5-03, A-10 | 直接確認 |
| `cordis-registry-duplication` | PluginRegistry 登録が8対6で乖離 | medium | D R-02, A-3, F-13 | 14箇所を確認 |
| `cordis-execution-models-mixed` | `#[tokio::main]` / 一時 runtime / 別スレッドが混在 | medium | D R-03, A-17, F-14 | 直接確認 |

### G7: 正確性・回復導線

| id | title | severity | 統合元 | 妥当性 |
| --- | --- | --- | --- | --- |
| `llm-probe-returns-stop-reason` | `acp_llm_probe` が本文ではなく stop reason を返す | high | C-02, A-14, E-14, F-3 | `acp_llm_probe.rs:53` を確認 |
| `prompt-ui-replies-never-filled` | `cordis_prompt_ui` が送信せず `replies` が永久に空 | high | C-01, A-1 | `submit()` と `replies.get()` のみを確認 |
| `errors-not-classified` | 残高/429/認証/切断が同一文字列、回復導線なし | high | r3-ops, E-11, B-37 | 直接確認 |
| `retry-ui-unrelated-to-acp` | retry デモが ACP と無関係で誤解を招く | high | r3-ops | `cordis_retry_ui.rs:35-60` を確認 |
| `errors-swallowed` | save / build_rows / open_window の結果を破棄 | medium | A-8, C-07, C-08, security | 複数 file:line を確認 |
| `assert-in-main-path` | `cordis_composition_toggle_ui.rs:98` に本番 `assert_eq!` | medium | A, C-08, A-9 | 直接確認 |
| `session-close-absent` | `session/close` / cancel が無く kill のみ（型自体が unstable gate） | medium | F, r3-ops, D-9, E-2/E-3 | `src/` 全体で0件を確認 |

### G8: UX / アクセシビリティ（プールから回収）

`investigation/round-1.md` B セクション43件のうち、重複を除いた主要項目を
todo の「中」へまとめて登録した。
- セッション CRUD / データモデル / 検索（B-1〜B-3, E-1, E-17）
- 会話のスクロール・コードコピー・メッセージ操作（B-8, B-10, B-11）
- 入力履歴 / ショートカット / キャンセル（B-21〜B-23）
- キーボードナビゲーション / スクリーンリーダー / コントラスト（B-40〜B-42）
- GUI 検証サイクル確立（B-32）← **G1 の検証偽陽性と同一なので高へ統合**

## 却下 / 修正した仮説（妥当性の記録）

| 主張 | 判定 | 根拠 |
| --- | --- | --- |
| fixture が7エントリ（プール D-11, r2-6） | **却下** | id は6件で README:105 と一致。3回確認 |
| README バイナリ表に不整合 | **却下** | 19行で実体19バイナリと1:1 |
| `sk-demo` が process env を汚染 / 実キー漏洩 | **却下** | `resolve_api_key` は渡された map のみ参照 |
| `provider: "spawn"` = OS プロセス起動 | **却下** | `SubagentRegistry` のプロバイダ名 |
| `Command::new("dsh")` の引数インジェクション | **却下** | 引数固定 + ACP JSON |
| symlink による `remove_dir_all` 誘導 | **却下（推測）** | Rust は symlink 自体を削除。未実行検証 |
| 「factory の第二引数」 | **用語補正** | クロージャ仮引数 `_config: &Value` が正しい |
| 「19バイナリ中13にテストが無い」 | **現行訂正** | 18 bin + main = 19、現行は18 bin 全てにテスト |
| 「5タグ遅れ」vs「4タグ遅れ」 | **両方正しい** | 比較対象が異なる（README 記述 → pin / pin → 最新） |
| 「全て confirmed」（プール A） | **格下げ** | 機能案・typo 推測を含むため `confidence` 定義を統一 |
| MIT が AGPL デフォルトに反する | **却下** | 初期 LICENSE + d70bb23 の明示的判断の痕跡あり |

## known_in_readme の統一ルール

- README に**機能の説明**があるが**欠陥の記載が無い**場合:
  `known_in_readme = yes（欠陥自体は no）` と表記する。
- これによりラウンド1 で yes/no が反転していた
  README 未実装リスト / approval / tmp パスの3件を統一した。

## 未確認として残す項目

- ACP の crates.io 最新版・セキュリティアドバイザリは調査済み
  （`investigation/round-3.md` C-17: 最新 5.3.0、B-1: `cargo audit`
  EXIT=1 / 26 件実測、`investigation/dependency-triage.md`: 到達15件・
  非到達11件）。**「cargo audit 未実行」は誤記**。残る未確認は
  ACP 5.x と DSH の互換性、tokio 最新版のみ。
- GPUI `Keystroke` の IME 状態フラグの有無。
- WAYLAND ネイティブウィンドウの実列挙と目視確認
  （`kdotool` 不在）。
- 性能仮説（`chunks.join("")`、バッファ無制限）の実測。
- Apache-2.0 依存の NOTICE 同梱状況。
- 共通 workflow の `@v1` タグ付け替え履歴。

## エージェントチーム利用の要約

- 起動: ラウンド1 で6名（うち3名が途中終了）、ラウンド2 で3名、
  ラウンド3 で3名、ラウンド4 で親が一次情報取得、
  ラウンド5 で3名。**合計 15 エージェント**（再起動8名を含む）。
- 途中終了は無視せず `list_agents` で status を確認し、
  スコープを絞った再起動（`*-r2` / `*-r3`）で回収した。
- 採用: 各レビュアーの指摘は仮説として扱い、
  親の Runtime Proof（直接 grep / read / `cargo test --bins` /
  ローカル crate ソース / GitHub・crates.io 一次情報）で
  事実確認したもののみ todo へ反映。
- 却下: 上記「却下 / 修正した仮説」11項目。
- 修正: 成果物は `cross-review/` と既存 `todo.md` のみ。
  **ソースコードは未修正**。

## 次のサイクル

サイクル2では、本 final の「未確認」項目を優先し、
特に GUI 検証サイクル確立（G1）と ACP 外部リサーチの代替経路を扱う。

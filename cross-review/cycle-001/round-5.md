---
type: Reference
title: サイクル001 ラウンド5 — 敵対的抜け漏れ / エッジケース / メタ監査
description: ラウンド1〜4の盲点、エッジケース、レビュー成果物自体の矛盾を検出
tags:
  - cross-review
  - cycle-001
  - round-5
generated: 2026-11-17T20:45:00+09:00
---

## A. r5-edge（エッジケース / 将来性）

### `ime-enter-submits-during-conversion`

- category: エッジケース（日本語入力）/ severity: 中 / confidence: 高（コード事実）/
  known: no
- evidence: `cordis_prompt_ui.rs:144-155`（特に `:146-147`）、`:63-69`、`:150-154`
- `observe_keystrokes` がキー種別のみを見て IME 変換中 / 確定キーを区別しない。
  日本語 IME では変換確定の Enter も捕捉され、
  確定前に `submit()` が走って `draft` がクリアされる。
- gpui 0.2.2 の `Keystroke` に IME 状態フラグがあるかはローカル未確認。

### `multi-session-buffer-collision`

- category: 将来性 / severity: 中（現状は単一セッション）/ confidence: 高（構造）/
  known: no
- evidence: `acp_stream_ui.rs:100-113,119-122,36`
- `notification.session_id` を参照せず全 chunk を同一 `Vec<String>` へ追記。
  複数セッション化で出所不明の混線が発生する設計課題。
  **測定根拠なし**（機能バグではない）。

### `unbounded-buffer-joined-per-frame`

- category: 性能仮説（**測定根拠なし**）/ severity: 低〜中 / confidence: 低（性能影響）/
  known: no
- evidence: `acp_stream_ui.rs:24,36,54`, `cordis_prompt_ui.rs:40-41`
- `chunks` に上限/trim が無く、Render ごとに `join("")` と
  `SharedString::from(...)` を実行。長大な会話でコストが伸びる可能性。
  あくまで仮説で機能バグではない。

### `platform-build-unverified`

- category: 将来性（推測）/ severity: 低 / confidence: 低〜中 / known: 一部
- evidence: `Cargo.toml:12`, `.github/workflows/ci.yml:18-25`, `README.md:49-50`
- gpui default features（wayland/x11/windows-manifest）と
  Ubuntu 用 apt 3パッケージの組み合わせで、
  Windows/macOS ビルドは未検証。成否は未ビルド確認。

### `detached-loop-holds-strong-refs`

- category: リソースリーク / severity: 中 / confidence: 高（構造）/
  known: no
- evidence: `acp_stream_ui.rs:157-170,164-165,63-64,141`
- 100ms ループに終了条件が無く `.detach()` でハンドルを失う。
  `panel` / バッファの strong 参照を解放せず、
  ACP スレッドもウィンドウ寿命と無関係に残る。
  GPUI が最終ウィンドウ閉鎖で自動 quit するかは未確認。

## B. r5-meta（レビュー自体のメタ監査）

### `known-flags-contradict-across-sections`

- category: 矛盾 / severity: high / confidence: high / known: no
- 同一項目なのに `known_in_readme` が yes/no で反転
  （README 未実装リスト、approval、tmp パス）。
  「同一（独立再検出）」と明記しているため統合の手掛かりはあるが、
  フラグ単体では最終値が不定。**ラウンド6で統一する**。

### `fixture-count-and-path-confusion`

- category: 矛盾 / severity: high / confidence: high / known: 一部
- `investigation/round-1.md:181` の「実数7」は未訂正のまま。
  さらに上流 `snapshots/sdk/text-turn/cordis.yml`（cordis-rs 側）と
  本リポジトリの `fixtures/text-turn.cordis.yml` の
  同一パス文字列がすり替わっている。
  同型: プール「5タグ遅れ」対 round-4「4タグ遅れ」は
  比較対象が異なる（README 記述 vs 実際の pin → 最新版）ため、
  両方を正しく説明する必要がある。

### `test-denominator-three-ways`

- category: 矛盾 / severity: medium / confidence: high / known: no
- 分子「テスト6」は一致するが分母が
  `18 bin` / `19 バイナリ` / `12 cordis bin` の3通り。
  正しくは **18 bin + main = 19 バイナリ、テスト6、残り13**。

### `confidence-definition-inconsistent`

- category: 証拠不足 / severity: medium / confidence: high / known: no
- プールの「20項目・全て confirmed」見出しに
  機能案・「typo の可能性」・マジックナンバー詰め合わせが混入。
  `confidence: high` が「静的読み取りの確度」か
  「Runtime Proof 済み」かで成果物間で定義が異なる。

### `duplicate-groups-top-five`

- category: 重複 / severity: high / confidence: high / known: 混在
- 上位5グループ:
  1. ストリーミング / smoke / Render 未検証（最大）
  2. permission / approval / ACP agent→client request
     （cordis 側 answerer と ACP 側 handler の**別原因が混在**）
  3. 依存・バージョン・ドキュメント数値
  4. /tmp + `sk-demo`
  5. テストカバレッジ
- グループ2 は単純マージすると誤修正を生むため原因分離が必須。

### `ux-a11y-items-dropped`

- category: 抜落 / severity: medium / confidence: medium / known: no
- プールの UX / アクセシビリティ 43件がラウンド1〜4に引き継がれていない。
  特に `B-32`（GUI 検証サイクル確立）は
  「全 GUI 機能の検証が止まるため優先度は機能より高い」と自ら書いている。
  兄弟 PoC（`gpui-task-manager` 等）の再利用調査もドロップしている。

### ラウンド6で最初に統合すべきグループ

1. ストリーミング / smoke / Render 未検証グループ
2. permission / approval / ACP agent→client request グループ（2原因を分離）
3. 依存・バージョン・fixture / ドキュメント数値グループ（一度の Real Proof で確定）

## C. r5-gaps（抜け漏れハンティング）

### `mount-failed-report-ignored`

- category: バグリスク / severity: high / confidence: high / known: no / 新規
- evidence: `cordis_entries_toggle_ui.rs:250-272`,
  `cordis_composition_ui.rs:176-197`, cordis-rs `loader.rs:344-375`
- `mount_available()` は `mounted` / `skipped_unknown` / `failed` を返すが、
  両デモは `failed` を一切描画しない。entries_toggle では
  失敗行が `entries` にも入らず、クリックによる再試行すら不能。

### `disabled-default-bypassed`

- category: バグリスク / severity: high / confidence: high / known: no / 新規
- evidence: `cordis_entries_toggle_ui.rs:238,260`,
  `fixtures/text-turn.cordis.yml:32,37`, loader.rs:177-178,296-308
- cordis-rs の `enabled_with(&state)` を使わず
  `saved.is_enabled(&id, true)` をハードコード。
  さらに `with_env_value("DSH_SNAPSHOT_FEEDBACK","1")` で
  `disabled: !!js "..."` を常に false 化し、disabled 分岐は一回も通らない。

### `entry-state-collapse`

- category: 改善 / severity: medium / confidence: high / known: no / 新規
- `EntrySlot` は `Option<Fiber>` のみで、
  YAML disabled / `skipped_unknown` / 明示 false が「unloaded」1つに潰れる。
  未知 ID も state ファイルへ保存され、将来の実装追加時に古い false を配り得る。

### `main-and-bridge-isolated`

- category: 改善 / severity: medium / confidence: high / known: 一部
- `main.rs` は静的会話とプレースホルダのみで他18バイナリと接続しない。
  `cordis_bridge.rs` は独自 `Commands` を provide するが、
  GPUI 側は `core_services::provide_commands` を使うため
  `/compact` 登録状態を表示する UI が存在しない。

### `prompt-ui-is-lower-compat-demo`

- category: 改善 / severity: medium / confidence: high / known: no / 新規
- `cordis_prompt_ui` は `replies` が永久に空で、
  `cordis_prompt_agent_ui` の下位互換。echo provider とキー処理も重複。
  残すなら後者一択。

### `review-artifacts-not-ignored`

- category: 改善 / severity: low / confidence: medium / known: 一部 / 新規
- `.gitignore:1` は `/target/` のみで、
  `cross-review/` / `investigation/` が `git add -A` で混入し得る。
  README:60,105,121 のローカル限定参照も残る。

**上位3件**: disabled 既定のバイパス → failed のサイレントドロップ →
3状態の混同と状態ファイル汚染。いずれも cordis-rs 側 API との整合性问题で、
修正量に対しデモの信頼性向上が大きい（整合性の問題）。

## D. 親の Runtime Proof（ラウンド5）

- `cordis_prompt_ui.rs:144-155` の IME 判定不在を直接確認。
- `acp_stream_ui.rs:157-170` の 100ms ループ + `.detach()`、
  `:175-179` の `refresh_from(&_buffer, cx)` が `cx.notify()` のみを実行することを確認。
- `fixtures/text-turn.cordis.yml` の `disabled: !!js "..."` は
  `cordis_entries_toggle_ui.rs:270` の `skipped_unknown` 表示と結びつく。
- `git status`: `cross-review/` と `investigation/` が未追跡。
  `.gitignore` は `/target/` のみ（ソース変更なし）。

## E. ラウンド5 集計

| 出典 | 件数 | 主な貢献 |
| --- | --- | --- |
| r5-edge | 5 | IME、マルチセッション、detach リーク、性能仮説の区別 |
| r5-meta | 6 | known 反転、数値矛盾、重複グループ、UX43件のドロップ |
| r5-gaps | 6 | failed ドロップ、disabled バイパス、entry 状態混同 |

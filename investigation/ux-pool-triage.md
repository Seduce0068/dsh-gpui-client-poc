---
type: Report
title: "UX 候補43件のトリアージ（round-1 B セクション）"
description: >-
  investigation/round-1.md B セクションの43件を現行実装と照合し、
  対応済み／却下／次フェーズ候補へ分類した結果を記録する。
tags:
  - investigation
  - ux
  - triage
  - dsh-gpui-client-poc
generated: 2026-11-18T16:39:26+09:00
verified: 2026-11-18T16:39:26+09:00
status: current
---

## 結論

- 対象 43 件を分類した。内訳は対応済み 6 件、
  却下 2 件、次フェーズ候補 35 件。
- 根拠は現行コード（`src/`）と既存 investigation
  （round-1〜5 / `final.md` / README / `todo.md`）のみ。
- 推測を含む項目は本文で明示した。
- 既存ファイルは変更していない。新規は本レポート 1 ファイルのみ。

## 分類の定義

- 対応済み: 現行コードに実装がある、または調査で
  「解消・確定」と明記された項目。
- 却下: 要件不一致やデータソース不在などにより、
  現状では採用しないと判断した項目（根拠を明記）。
- 次フェーズ候補: 妥当だが未実装の項目。部分対応もここに
  含め、残る内容を明記した。

## サマリ

| 分類 | 件数 | ID |
| --- | --- | --- |
| 対応済み | 6 | B-19, B-32, B-34, B-35, B-36, B-37 |
| 却下 | 2 | B-14, B-27 |
| 次フェーズ候補 | 35 | 上記以外の 35 件（詳細節を参照） |

### 優先して扱う次フェーズ候補

- 入力基盤: B-20 multiline-input、B-23 input-cancel、
  B-24 clipboard-cursor。
- データモデル: B-2 session-data-model、
  B-1 session-crud、B-18 streaming-partial-markdown。
- 検証・透明性: B-38 connection-status（再接続）、
  B-40 keyboard-navigation、B-32 は対応済み。
- 消滅リスクへの回復: B-5 session-folders、
  B-29 deep-link、B-31 background-startup。

## 詳細

### B-セッション一覧

- **B-1 `session-crud`** — 次フェーズ候補
  - 根拠: `src/main.rs:59-68` に `on_click` なし。
    新規/改名/削除/選択の実装なし。
  - 補足: ACP `session/new` は接続確認済み
    （README:57-58）。実装経路は短い。

- **B-2 `session-data-model`** — 次フェーズ候補
  - 根拠: `src/main.rs:10-19` は `Vec<SharedString>`
    のみ。`session_id` / `updated_at` / 状態 / 永続化なし。
  - 他のセッション系 UX の前提となる最優先項目。

- **B-3 `session-search`** — 次フェーズ候補
  - 根拠: `src/main.rs:47-70` に検索 UI なし。
    `Ctrl+K` のバインドもなし。

- **B-4 `session-sort-pin`** — 次フェーズ候補
  - 根拠: 並び替え・ピン留めのコードなし。
    B-2 のデータモデル依存。優先度は低。

- **B-5 `session-folders`** — 次フェーズ候補（推測）
  - 根拠: フォルダ/タグ分類の実装なし。
    **round-1 自身が「推測ベース」と明記**。
  - `investigation/round-5.md` AUDIT-05 で完全消滅を指摘。
    round-3 の別 id `B-5` と衝突するため id 注意。

- **B-6 `unread-badge`** — 次フェーズ候補
  - 根拠: `src/main.rs` に未読ドット/スピナーなし。
    `src/bin/acp_stream_ui.rs` の status と未連動。

- **B-7 `session-title-manual-override`** — 次フェーズ候補
  - 根拠: `src/bin/cordis_title_ui.rs:36-80` は
    LLM 生成と first-message fallback のみ。
    手動上書き API / UI なし（E-17 / F-16 と重複）。

### B-会話ビュー

- **B-8 `conversation-scrollback`** — 次フェーズ候補
  - 根拠: `src/main.rs:72-117` にスクロール領域と
    「最新へ追従」制御なし。

- **B-9 `conversation-virtualization`** — 次フェーズ候補
  - 根拠: 仮想化の実装なし。`investigation/round-2.md`
    で gpui 0.2.2 の `uniform_list` / `list` による
    可視部分描画 API の存在を確認（調査自体は解消）。
  - `investigation/round-5.md` E5B-12 で 1000 件時に
    1 フレーム 1000 超要素との指摘。未実装・実測待ち。

- **B-10 `code-copy`** — 次フェーズ候補
  - 根拠: fenced code block の Copy ボタンと言語ラベルなし。
    gpui 側クリップボード API は存在（round-2 B-F1）。

- **B-11 `message-actions`** — 次フェーズ候補
  - 根拠: `src/main.rs:81-105` に編集/再生成/削除の
    ハンドラなし。

- **B-12 `diff-view`** — 次フェーズ候補（推測含む）
  - 根拠: `src/bin/cordis_agent_ui.rs` に tool 結果表示は
    あるが before/after diff と差し戻しはなし。
  - 補足（推測）: DSH は `fs/*` を発行しない
    （`final.md` 妥当性吸収表 E-6）ため、ACP 経由の
    write 結果は届かない可能性がある。

- **B-13 `attachments`** — 次フェーズ候補
  - 根拠: `src/bin/acp_stream_ui.rs:189-192` は
    `ContentBlock::Text` のみ。他は ignore 理由を表示。
    ファイル添付の経路なし。

- **B-14 `image-display`** — 却下
  - 根拠: DSH の実送信は 6 種（thought / message /
    tool_call / tool_call_update / usage / config_option）
    で画像 ContentBlock は含まれない
    （`final.md` 妥当性吸収表 E-7）。
  - 判定: gpui 側に Image API があっても PoC 未接続で
    入力データが無いため、現状は採用しない。
  - 補足（推測）: 将来 ACP 側が画像送信に対応したら再評価。

- **B-15 `link-handling`** — 次フェーズ候補
  - 根拠: URL の自動リンク化と危険スキーム確認は未実装。
    セキュリティ上も確認が必要。

- **B-16 `timestamps`** — 次フェーズ候補
  - 根拠: `src/main.rs:10-13` の `Message` に時刻・
    モデル名・プロファイル名のフィールドなし。

- **B-17 `token-usage`** — 次フェーズ候補
  - 根拠: `src/bin/acp_stream_ui.rs:184-186` で
    `usage_update` は feature 無効のため明示的に無視。
    DSH は usage を送信するため表示対象。

- **B-18 `streaming-partial-markdown`** — 次フェーズ候補
  - 根拠（一部対応）: 100ms ポーリングは `Notify` +
    `WeakEntity` + `CancellationToken` へ置換済み
    （`todo.md` の `stream-polling-and-detach-leak` 完了）。
  - 残項目: Markdown 描画は未実装（README:170）。
    `chunks.concat()` の生文字列 join も残存。

### B-入力 UX

- **B-19 `ime-japanese`** — 対応済み
  - 根拠: `src/bin/cordis_prompt_ui.rs:76-147` に
    `ImePhase` と `is_ime_in_progress()` による変換中判定。
  - 同 `:345-404` で `EntityInputHandler` /
    `marked_text_range` / preedit 別保持を実装。
    変換中の Enter は送信しない（テストあり）。

- **B-20 `multiline-input`** — 次フェーズ候補
  - 根拠: `src/bin/cordis_prompt_ui.rs:105-108` の
    `modified` は ctrl / alt / platform / function のみで
    shift を含まない。非変換時の Enter は常に Submit。
    Shift+Enter 改行は未実装。

- **B-21 `input-history`** — 次フェーズ候補
  - 根拠: Up/Down による履歴呼び出しの実装なし。

- **B-22 `input-shortcuts`** — 次フェーズ候補
  - 根拠（一部対応）: 修飾キーの一部判定は存在するが
    （B-20 参照）、ショートカットの一元定義はなし。

- **B-23 `input-cancel`** — 次フェーズ候補
  - 根拠: UI 側に stop ボタンも `session/cancel` /
    `StopReason::Cancelled` の配線もなし（UI の grep 0 件）。
    round-1 E-2 と統合。

- **B-24 `clipboard-cursor`** — 次フェーズ候補
  - 根拠（一部対応）: `EntityInputHandler` の
    `selected_text_range` は常に末尾 `end..end` を返し、
    `marked` / `replace` は IME 用途（B-19）。
  - 残項目: 任意のカーソル移動・選択・Undo は未実装。

- **B-25 `paste-attachment`** — 次フェーズ候補
  - 根拠: クリップボード画像のペースト添付なし。
    gpui 側の `ClipboardItem` / Image 生成 API は存在
    （round-2 B-F1）。

### B-デスクトップ統合

- **B-26 `system-tray`** — 次フェーズ候補
  - 根拠: トレイ常駐と「隠す/終了」分離は未実装。
    `tray-icon`（ksni feature）は条件付き採用候補
    （round-2 C-15 / `final.md`）。
  - `investigation/round-5.md` AUDIT-05 で
    タスク化漏れを指摘。

- **B-27 `global-hotkey`** — 却下
  - 根拠: `global-hotkey` 0.8.0 は
    "Linux (X11 Only)" で Kubuntu Wayland と要件不一致
    （round-2 C-16 / `final.md` 採用・却下表）。
  - 補足: `ashpd::global_shortcuts` も zbus 5.13
    不一致で即時却下（`final.md` 同表）。

- **B-28 `notifications`** — 次フェーズ候補
  - 根拠: OS 通知は未実装。gpui 0.2.2 に通知 API なし
    （round-2 B-F5）。`notify-rust` は条件付き採用、
    `dsh-ntfy-notify` の設計転移が候補（`final.md`）。

- **B-29 `deep-link`** — 次フェーズ候補（低）
  - 根拠: `dsh://session/...` のスキーム処理なし。
    AUDIT-05 で完全消滅を指摘。

- **B-30 `multi-window`** — 次フェーズ候補
  - 根拠: `src/main.rs:133` は `open_window` 1 回のみ。
    セッションのポップアウトは未実装。

- **B-31 `background-startup`** — 次フェーズ候補（低）
  - 根拠: XDG autostart / 自動更新なし。
    round-1 自身が「後回し候補」と明記。
    AUDIT-05 で完全消滅を指摘。

- **B-32 `wayland-x11-verification`** — 対応済み
  - 根拠: README:87-137 に `spectacle` +
    `read_image` による実描画確認手順を確立。
    `final.md` P1-8 / `todo.md` の
    `verify-smoke-false-positive` 完了。

- **B-33 `wm-class-desktop-file`** — 次フェーズ候補
  - 根拠: `.desktop` と `StartupWMClass` の整合設定なし。

### B-信頼性/透明性

- **B-34 `permission-explanation`** — 対応済み
  - 根拠: `src/bin/cordis_permission_ui.rs:7-13,31-76`
    で `DSH_GPUI_APPROVAL_ANSWER` により
    allow / deny / ask を切替。
  - 同 `:209-228` で description と tool 引数を
    trace / UI に表示。常に Allow ではなくなった。
  - 補足: ACP `session/request_permission` handler は
    条件付きで未登録（`todo.md` の
    `acp-handlers-absent` 未完了）だが、本項目は
    cordis デモ側の指摘。

- **B-35 `tool-call-visualization`** — 対応済み
  - 根拠: `src/bin/acp_stream_ui.rs:200-214` で
    `ToolCall` / `ToolCallUpdate` を id / title / status
    付きのラベル行として描画。
  - 残項目（次フェーズ）: 折りたたみカード・所要時間。

- **B-36 `reasoning-display`** — 対応済み
  - 根拠: `src/bin/acp_stream_ui.rs:193-199` で
    `AgentThoughtChunk` を `[thought]` 行として描画。
  - 残項目（次フェーズ）: 折りたたみ表示。

- **B-37 `error-presentation`** — 対応済み
  - 根拠: `src/bin/cordis_retry_ui.rs:37-131` で
    Balance / RateLimited / AuthExpired / Disconnected の
    4 分類と回復導線を表示。
  - 同 `:7-11` で ACP 非接続の local stub である旨を
    常時明示（`todo.md` の `errors-not-classified` 完了）。

- **B-38 `connection-status`** — 次フェーズ候補
  - 根拠（一部対応）: `src/bin/acp_stream_ui.rs:264-277`
    で connection status を GPUI ウィンドウに表示。
  - 残項目: 再接続ボタンは未実装。

- **B-39 `cost-display`** — 次フェーズ候補（要調査）
  - 根拠: 推定コスト表示なし。単価出所は未調査
    （round-1 の指摘のまま）。推測で数値を出さない。

### B-アクセシビリティ

- **B-40 `keyboard-navigation`** — 次フェーズ候補
  - 根拠: `src/main.rs:120-128` に Tab / F6 / 矢印 /
    Enter / Esc のフォーカス移動ハンドラなし。

- **B-41 `screen-reader`** — 次フェーズ候補（調査）
  - 根拠: AT-SPI 対応範囲の調査は未実施。
    round-2 の集計で gpui 0.2.2 に AccessKit なし。

- **B-42 `contrast-fontsize`** — 次フェーズ候補
  - 根拠: `src/main.rs:52-66` は固定色リテラル。
    テーマ切替自体が未実装（README:170）。
    `gpui-task-manager` の Apache テーマが再利用候補。

- **B-43 `locale-formatting`** — 次フェーズ候補
  - 根拠: 日付/数字書式と IME 連動なし。
    locale / i18n は未実装（README:170）。

## 推測・未確認の明示

- B-5: round-1 の段階で既に「推測ベース」とされる。
  必要性の評価は次フェーズで利用者確認が必要。
- B-12: DSH が `fs/*` を発行しないことから ACP 経由の
  write 結果が届かない可能性は推測。cordis 側の
  tool-fs デモでは結果が存在する。
- B-14: 将来の ACP 側画像対応は推測。現状の却下理由は
  DSH の実送信 6 種に画像が無いという事実に基づく。
- B-39: 単価出所は未調査。コスト値は記載していない。
- 本レポート自体はコードと既存文書の静的照合であり、
  新たな GUI 実操作・実実行検証は実施していない。

## 検証

- markdownlint（CLI v0.50.0、デフォルト規則）で
  本ファイルを検証し、error / warning 0 件を確認した。
- 既存ファイル（round-1.md ほか）は変更していない。
- git の commit / checkout / reset / stash / clean は
  実行していない。
- 検証時点の `git status` で `src/bin/acp_probe.rs` に
  本タスク以外の変更（+146 / -7）を確認。本タスクは
  同ファイルを編集していない。

## 参照

- `investigation/round-1.md` B セクション（分類元）
- `investigation/round-2.md` / `round-5.md` / `final.md`
- `README.md` / `todo.md`
- `src/main.rs` / `src/bin/*.rs`（現行実装）

# ラウンド1: コードベース監査 + ドキュメント照合 + UX発想

日時: 2026-11-17T19:17+09:00 頃
起動エージェント: `r1-code-audit`, `r1-docs-audit`, `r1-acp-analysis`, `r1-cordis-analysis`, `r1-ux-ideation`（5並列）

## 原点（ которым 全エージェントで共有した既知項目 = README:109-113）

- GPUI の `Render` と ACP の Tokio セッションの統合
- `session/update` のストリーミング表示、テキスト入力、Markdown 描画、テーマ切替、locale

以下は **既知6項目以外** を中心にした検出項目。`known: yes` は既知項目の具体的不備・深掘り。

---

## A. `r1-code-audit`（コードベース監査、20項目・全て confirmed）

### A-1 `prompt-ui-replies-never-filled` [残タスク / high / known:no]
`cordis_prompt_ui.rs:1-5,41,50,63-69,90-100,128-139`。モジュール doc は "wired to the cordis-rs agent loop" と書くが `submit()` は `submitted.push()` と `draft.clear()` のみ。`replies` は定義・初期化・参照のみで書き込み無し（dead code）。`:128-129` のコメント自身が "a real host would keep the AgentLoop entity alive here and call `run(prompt)`" と認めている。`cordis_prompt_agent_ui.rs` が正しく実装済み。**コードコメントと README の乖離がユーザーを誤導**。

### A-2 `permission-ui-deny-unreachable-and-bash-stub` [残タスク / high / known:yes]
`cordis_permission_ui.rs:48-57,73-76,112-117`。answerer が常に `Allow` を返すため deny 経路が到達不能なのに、表示側には "DENIED" 分岐がある。README:27 は「allow/deny を表示するデモ」と記載。`bash` tool はコマンドを実行せず固定文字列を返すだけ。

### A-3 `plugin-registry-registration-duplicated` [改善 / high / known:no]
`cordis_composition_ui.rs:68-174` と `cordis_entries_toggle_ui.rs:152-231` で同一の `PluginFactory` 登録が延べ14回重複。差分は tool-fs / tool-bash の有無のみ。`<placeholder-key>` ダミーキーも重複。共通 `build_registry()` 抽出候補。

### A-4 `text-input-keyboard-logic-duplicated` [改善 / high / known:no]
`cordis_prompt_ui.rs:38-69,144-156` と `cordis_prompt_agent_ui.rs:39-89,172-201` で `draft/push_char/backspace/observe_keystrokes` がほぼ同一。共通 `TextInput` 化で既知「テキスト入力」改善の同期コストを削減。

### A-5 `acp-subprocess-boilerplate-duplicated` [改善 / high / known:no]
`acp_probe.rs:11-38`, `acp_prompt_probe.rs:13-39`, `acp_multi_turn_probe.rs:20-43`, `acp_stream_probe.rs:21-64`, `acp_llm_probe.rs:24-45`, `acp_stream_ui.rs:76-122` の6箇所で `dsh --profile acp` 起動〜initialize/session/new が同一。エラー型も不統一。

### A-6 `acp-stream-ui-hardcoded-prompt-no-input` [機能追加案 / high / known:no]
`acp_stream_ui.rs:77,114-131`。送信プロンプトが `"Explain this repository in one sentence."` 固定、UI に入力 widget 無し。ストリーム表示自体は README:26 既知だが「入力してストリームを見る」経路が欠落。

### A-7 `acp-stream-ui-infinite-poll-leak` [改善 / high / known:no]
`acp_stream_ui.rs:63-75,160-170,176-181`。`cx.spawn` の `loop { 100ms timer; app.update() }` に終了条件が無く `.detach()`。ウィンドウ閉鎖後も ACP 接続・タイマが残る。`cx.notify()` 駆動への置換候補。

### A-8 `swallowed-errors-and-ignored-results` [改善 / high / known:no]
`let _ = state.saved.save(...)`（entries:118）、`fiber.unload()`（entries:86, toggle:40）、ACP `child.kill()`（各 probe）、`build_rows().unwrap_or_default()`（title:100）、各 `cx.open_window` の結果破棄。保存失敗やウィンドウ生成失敗が見えず「成功表示」が偽陽性になり得る。

### A-9 `assert-in-production-paths` [改善 / high / known:no]
`cordis_composition_toggle_ui.rs:98` と `acp_multi_turn_probe.rs:71` の main パスに `assert_eq!`。mount 状態変更時に panic し得る。

### A-10 `skipped-unknown-entries-unremountable` [残タスク / high / known:no]
`cordis_entries_toggle_ui.rs:270-272,71-79,88-97`。factory が無い skipped エントリが "unloaded" と表示されるが toggle は no-op（`(ctx, None)` → 何も起きない）。

### A-11 `todo-content-hardcoded` [機能追加案 / high / known:no]
`cordis_todo_ui.rs:117-119,129-131,141-143`。3ボタンすべてが同一内容 "Prepare workspace"、自由入力・複数 todo 追加不可。

### A-12 `main-ui-static-hardcoded-data` [残タスク / high / known:yes]
`src/main.rs:24-43,47-69,81-116,115`。セッション3件・メッセージ3件・全色がベタ書き、サイドバー行に `on_click` なし、入力欄はプレースホルダ。既知「静的 UI」の具体内訳。

### A-13 `bridge-headless-out-of-scope` [機能追加案 / high / known:no]
`cordis_bridge.rs:3-6,66-86`。コメントが「最終目標は GPUI ウィンドウ表示。このバイナリは意図的にそこで止まる」と明言。`commands` は将来 GPUI view が消費する状態。**コマンド一覧 UI** が明確な追加候補。

### A-14 `acp-llm-session-persistent-future-work` [残タスク / high / known:yes]
`acp_llm_probe.rs:3-5,23-58`。doc が "a full session-persistent ACP client is future work" と明記。毎回 spawn + session/new。マルチターン probe は再利用するが agent-loop 未接続。既知「セッション統合」の具体的不良箇所。

### A-15 `fs-ui-host-specific-default-path` [改善 / high / known:no]
`cordis_fs_ui.rs:56-59`。既定パスが開発者固有の絶対パス。別環境で即 "read failed"。可搬性の問題。

### A-16 `hardcoded-temp-paths-and-credentials` [改善 / high / known:no]
`/tmp/cordis-gpui`, `/tmp/cordis-entries`, `/tmp/cordis-entries-toggle.json`, `/tmp/cordis-agent-ui.txt`, `<PROVIDER_API_KEY_ENV>=<placeholder-key>` が分散。`composition_ui.rs:203` は起動直後に `remove_dir_all`。並列実行時の衝突・状態汚染。

### A-17 `permission-ui-recreates-runtime-per-click` [改善 / high / known:no]
`cordis_permission_ui.rs:33-38,129-133`。クリック毎に Tokio runtime を生成し `block_on`。他デモは起動時1回のみでパターン不統一。

### A-18 `prompt-probes-stop-reason-only` [調査必要 / high / known:yes]
`acp_prompt_probe.rs:41-48` 等。stop_reason のみで応答本文を検証せず。残高問題解消時に「本文が正しいか」を検証するテストが無い。

### A-19 `default-prompt-language-inconsistent` [改善 / high / known:no]
既定プロンプトが日本語/韓国語/英語に散在（`acp_stream_probe.rs:24` の "1+1은?" は typo の可能性）。

### A-20 `assert-and-magic-numbers-minor` [改善 / high / known:no]
actor 名 "gpui"、fallback タイトル長20、"HTTP 429 rate limited" スタブ、`max_retries:2`、`initial_delay_ms:1`、100ms poll、無題ウィンドウ等。

---

## B. `r1-ux-ideation`（UX 機能案、43項目）

### B-セッション一覧
- **B-1 `session-crud`** [機能追加案/high/no] `main.rs:59-68,16-19`。新規/改名/削除/選択が皆無。ACP `session/new` は接続確認済み（README:52-54）で実装経路は短い。
- **B-2 `session-data-model`** [残タスク/high/no] `main.rs:17,9-13`。`Vec<SharedString>` しか無く、session_id / updated_at / 状態 / 永続化が欠落。全 UX の前提。
- **B-3 `session-search`** [機能追加案/high/no] `main.rs:47-70`。インクリメンタル検索と `Ctrl+K`。
- **B-4 `session-sort-pin`** [機能追加案/medium/no] 並び替え・ピン留め。
- **B-5 `session-folders`** [機能追加案/medium/no] フォルダ/タグ分類（推測ベース）。
- **B-6 `unread-badge`** [機能追加案/medium/no] 未読ドット・生成中スピナー。`acp_stream_ui.rs:148` のステータスと未連動。
- **B-7 `session-title-manual-override`** [改善/high/yes] `cordis_title_ui.rs:24-69`。自動タイトルの手動上書き（自動生成自体は既知として除外）。

### B-会話ビュー
- **B-8 `conversation-scrollback`** [改善/high/no] `main.rs:72-117`。スクロール領域と「最新へ追従」制御。ストリーミングと組み合わせると必須。
- **B-9 `conversation-virtualization`** [調査必要/medium/no] `main.rs:81-105`。数百〜数千件での仮想化。実測してから対応（性能仮説）。
- **B-10 `code-copy`** [機能追加案/high/no] `main.rs:103`。fenced code block の Copy ボタンと言語ラベル。
- **B-11 `message-actions`** [機能追加案/high/no] `main.rs:81-105`, `cordis_prompt_agent_ui.rs:66-76`。編集/再生成/削除。
- **B-12 `diff-view`** [機能追加案/medium/no] `cordis_agent_ui.rs:45-53,73-77`。write ツール結果の before/after diff と差し戻し。
- **B-13 `attachments`** [機能追加案/high/no] `acp_stream_ui.rs:124-127`。`ContentBlock::Text` のみ → ファイル添付。
- **B-14 `image-display`** [機能追加案/medium/no] `acp_stream_ui.rs:104-107`。画像 ContentBlock のインライン表示。
- **B-15 `link-handling`** [機能追加案/high/no] URL 自動リンク化と危険スキーム確認。
- **B-16 `timestamps`** [改善/high/no] `main.rs:9-13`。時刻・モデル名・プロファイル名。
- **B-17 `token-usage`** [機能追加案/medium/no] `acp_stream_ui.rs:104-107`。トークン使用量・コンテキスト残量。
- **B-18 `streaming-partial-markdown`** [残タスク/high/yes] `acp_stream_ui.rs:36,159-168`。生文字列 join + 100ms ポーリング。既知2項目が重なる真の難所。**Render/Tokio 統合の本来の形（Entity 駆動）に直すポイント**。

### B-入力 UX
- **B-19 `ime-japanese`** [残タスク/high/yes] `cordis_prompt_ui.rs:144-155,5`。Enter が常に送信 → 日本語変換確定が送信される。日本語ユーザー必須。
- **B-20 `multiline-input`** [改善/high/yes] `cordis_prompt_ui.rs:146`。Shift+Enter 改行。
- **B-21 `input-history`** [機能追加案/high/no] Up/Down で履歴呼び出し。
- **B-22 `input-shortcuts`** [改善/high/no] `cordis_prompt_ui.rs:144-155`。modifiers 未参照。ショートカット一元定義。
- **B-23 `input-cancel`** [残タスク/high/no] `cordis_prompt_agent_ui.rs:43,66-76,183-190`。busy 中に stop ボタン + `session/cancel`。
- **B-24 `clipboard-cursor`** [残タスク/high/yes] `cordis_prompt_ui.rs:55-69`。push_char/pop のみでカーソル・選択・Undo が無い。既知「テキスト入力」の最重要サブ課題。
- **B-25 `paste-attachment`** [機能追加案/medium/no] クリップボード画像のペースト添付。

### B-デスクトップ統合
- **B-26 `system-tray`** [機能追加案/medium/no] `main.rs:131-136`。トレイ常駐と「隠す/終了」分離。
- **B-27 `global-hotkey`** [機能追加案/medium/no] Wayland では portal ベース要調査（README:69,73-75）。
- **B-28 `notifications`** [機能追加案/medium/no] `acp_stream_ui.rs:134-140`。応答完了/エラー通知。
- **B-29 `deep-link`** [機能追加案/low/no] `dsh://session/...`。Web↔ネイティブの受け渡し。
- **B-30 `multi-window`** [機能追加案/medium/no] `main.rs:133`。セッションのポップアウト。
- **B-31 `background-startup`** [機能追加案/low/no] XDG autostart・自動更新（後回し候補）。
- **B-32 `wayland-x11-verification`** [調査必要/high/no] README:69,73-75。`kdotool` 導入等でエージェント自身の GUI 検証を確立。**全 GUI 機能の検証サイクルが止まるため優先度は機能より高い**。
- **B-33 `wm-class-desktop-file`** [改善/medium/no] `.desktop` と `StartupWMClass` の整合（AGENTS.md §10 の Edge 拡張の例を参照）。

### B-信頼性/透明性
- **B-34 `permission-explanation`** [改善/high/no] `cordis_permission_ui.rs:48-49,106-118`。常に Allow の answerer、説明カード欠落。セキュリティ上も重要。
- **B-35 `tool-call-visualization`** [改善/high/no] `cordis_agent_ui.rs:65-80`。tool call の折りたたみカード（引数/実行中/結果/所要時間）。
- **B-36 `reasoning-display`** [機能追加案/medium/no] `acp_stream_ui.rs:104-107`。thinking/reasoning の折りたたみ表示。
- **B-37 `error-presentation`** [改善/high/no] `cordis_prompt_agent_ui.rs:85-88`, `cordis_retry_ui.rs:82-88`。残高/レート/切断/認証の分類と回復導線。
- **B-38 `connection-status`** [改善/high/no] `acp_stream_ui.rs:141,148`。接続状態と再接続ボタン。
- **B-39 `cost-display`** [機能追加案/medium/no] トークン使用量からの推定コスト（単価出所は要調査）。

### B-アクセシビリティ
- **B-40 `keyboard-navigation`** [改善/high/no] `main.rs:120-128`。Tab/F6/矢印/Enter/Esc のフォーカス移動。
- **B-41 `screen-reader`** [調査必要/medium/no] `main.rs:60-68`。AT-SPI 対応範囲の調査。
- **B-42 `contrast-fontsize`** [改善/medium/yes] `main.rs:52-66`。ハイコントラスト・文字サイズ倍率（既知テーマの深掘り）。
- **B-43 `locale-formatting`** [改善/medium/yes] `main.rs:103`。日付/数字書式、IME 連動（既知 locale の深掘り）。

UX エージェントの優先度: (1) 入力基盤 `ime-japanese`+`clipboard-cursor`+`multiline-input`、(2) `streaming-partial-markdown`（+session-data-model, connection-status）、(3) `session-crud`+`session-data-model`。

---

## C. 親エージェントの独自確認（Runtime Proof）

- **C-1 baseline-tests-pass-but-sparse** [改善/high/no]
  `cargo test --bins` は EXIT=0（全テスト成功）。分母は
  18 bin + main = 19 バイナリ（2026-11-18 実測）。当時は
  `#[test]` / `mod tests` が 6 バイナリのみだったが、現行実装では
  18 bin すべてに `#[cfg(test)]` / `#[test]` が存在。
  テストが無いのは `src/main.rs` のみ。
  「19バイナリ中13バイナリにテストが無い」は obsolete。
- **C-2 private-design-doc** [調査必要/high/no] 非公開設計資料 `passkey-gateway/docs/dsh-client-gpui-migration-2026-11-16.md` の §5「未確認事項」に、wasm32 ビルド可否、Markdown レンダリング難度の定量評価、Render×Tokio 統合（既知）、ファイル D&D/画像プレビュー/シンタックスハイライトの難度、DSH 公式ロードマップ上のネイティブクライアント想定の有無が列挙されている。§3.1 にはスロット機構再現、`dsh.client` マニフェスト、`cordis-client-runner` のサンドボックス再設計、接続/認証レイヤ再実装（経路3のみ）等のアーキテクチャ課題。§4.1 の「案A（Web 維持 + GPUI 独立ツール）推奨」方針と整合するか検討が必要。
- **C-3 related-pocs** [調査必要/medium/no] 同一 workspace に `work/gpui-task-manager`, `work/gpui-file-explorer`, `work/dsh-tui`, `work/bitwarden-gpui-poc` が存在。案A（独立 GPUI ツール群）の実績/再利用候補として後 rounds で調査する価値あり。

---

## D. `r1-docs-audit`（ドキュメント/git 履歴照合、15項目）

### D-1 `wayland-window-only-stderr-confirmed` [調査必要/high/yes]
README:49-55,66-83 + `cordis_todo_ui.rs:197`。「実機でウィンドウが開く」根拠は `eprintln!("... GPUI window created")` のみ。自動列挙も目視も未実施。README 自身が「kdotool が無く…目視はユーザー確認が必要」と矛盾を認めている。C-2/B-32 と重複（統合候補）。

### D-2 `acp-verified-handshake-only` [残タスク/high/yes]
README:52-55,89-101 + git 57dbc01/2a60bc7。ACP「確認済み」は全てハンドシェイクまで。実レスポンス本文は残高問題で一度も未検証。**テスト用モック LLM provider の缺失**がアンカー（A-18 と重複）。

### D-3 `smoke-script-grep-only` [改善/high/no]
`scripts/smoke-gui.sh:2-4,26`。`"Not part of CI"` かつ `grep -q 'GPUI window created'` のみ。Render 結果・ボタン操作・実ウィンドウは未検証。12デモの "ok" が誤った安心感を与え得る。

### D-4 `run-examples-missing-for-10-bins` [改善/high/no]
README:12-32 の19バイナリに対し、README:36-47 の実行例は10件欠落（acp_llm_probe, acp_multi_turn_probe, acp_stream_probe, acp_stream_ui, cordis_composition_ui, cordis_composition_toggle_ui, cordis_entries_toggle_ui, cordis_permission_ui, cordis_retry_ui, cordis_title_ui）。

### D-5 `libc-mio-pins-not-in-manifest` [残タスク/high/no]
README:57-64 が「ピン留めが必要」とする `libc=0.2.177`, `mio=1.1.0` が Cargo.toml:8-15 に無く Cargo.lock:3142,3401 の推移依存のみ。`cargo update` で `xattr`/`ENOATTR` 問題が再発し得る。**ビルド再現性の実害**。

### D-6 `dep-note-stale-tokio-helper` [改善/high/no]
commit 09453cb で削除済みの `agent-client-protocol-tokio` を、参照先の非公開レポート §4.4 が今も前提にしている。README:60 が「詳細」として誘導している。

### D-7 `cordis-rs-version-notes-stale` [改善/high/no]
README:103-107 は `v0.29.0` と記載していたが、当時 Cargo.toml:10 は
`v0.62.0`（5タグ遅れ、28aa8a0→aacbf9a）。**2026-11-18 現在は
README:67,157 と Cargo.toml:10 がともに `v0.133.0` に同期済み**。

### D-8 `lld-linker-config-removed` [残タスク/medium/no]
commit d70bb23 が `.cargo/config.toml`（lld + libshim）を削除。非公開レポート §2.3:88 は lld 必須と明記するが CI apt_packages に lld なし。クリーン環境でのビルド再現性に影響し得る（推測含む）。

### D-9 `session-close-never-called` [残タスク/medium/no]
非公開レポート §4.2:159 が ACP README 引用で `session/close` を手順に含めるが、`grep -rn 'session_close|SessionClose' src` → 0件。子プロセスは timeout/kill のみ。リソースリーク・正常シャットダウン欠落。

### D-10 `streaming-docs-contradict-code` [改善/high/yes]
README:109-113 + 非公開レポート §4.3/§5 は「ストリーミング未着手」とするが、commit 57dbc01/2a60bc7 で listen + GPUI 共有バッファ描画が既に動く。**未実装リストの信頼性低下**。残るのは実データでの検証のみ（D-2 と表裏）。

### D-11 `fixture-entries-mounted-not-exercised` [機能追加案/high/no]
`fixtures/text-turn.cordis.yml:16-37` の message-feedback / feedback-producer /
session-persistence / tool-subagent は mount 表示のみ。フィードバック送信・
永続化ラウンドトリップ・subagent 呼び出しのデモが無い。
README:156 の「6エントリ」は実数 6 件と一致（2026-11-18 再確認）。
**次の機能デモ候補が明確**。

### D-12 `feedback-producer-js-absent` [調査必要/medium/no]
fixture:36 が参照する `./feedback-producer.mjs` は repo に存在せず、Rust `mount_feedback_producer` に差し替え済み。実プラグインとの挙動互換性は未検証。

### D-13 `readme-fixture-path-wrong` [改善/high/no]
README:105 は `snapshots/sdk/text-turn/cordis.yml` と記載するが実は `fixtures/text-turn.cordis.yml`（composition_ui.rs:54 の include_str!）。

### D-14 `private-doc-unmirrored-decisions-and-unknowns` [調査必要/high/no]
非公開レポート（実在確認済み、223行）の §3.1:115-125（テーマ/Markdown/i18n/アクセシビリティ/DnD）、§4.1-4.3、§5:204-210（wasm32 可否、Markdown 難度、DnD/画像/ハイライト、DSH ロードマップ）が GitHub に未反映。README「未実装」は §3.1 のごく一部のみ。C-2 と同一（統合）。

### D-15 `automated-test-coverage-thin` [改善/medium/no]
`#[cfg(test)]` は現行 18 bin すべてに存在（C-1 と同一。当時は 6 のみ）。
`[dev-dependencies]` なし、smoke は CI 非対象（GUI 回帰検知は弱いまま）。

docs エージェントの優先度: (1) D-14 非公開資料の未反映判断、(2) D-5 libc/mio ピン不在、(3) D-11 fixture エントリの意味的未活用。

---

## E. `r1-acp-analysis`（ACP 統合深掘り、18項目）

一次情報: ローカルの `agent-client-protocol 0.11.1` crate source（`~/.cargo/registry/src/.../agent-client-protocol-0.11.1`）と `agent-client-protocol-schema 0.12.0`（Cargo.lock:58-61）。Web 検索は失敗したため未使用（参照試行 URL: <https://github.com/agentclientprotocol/agent-client-protocol>）。

### E-1 `session-load-list-missing` [残タスク/high/no]
全 probe が `initialize → session/new → session/prompt` のみ。crate には `LoadSessionRequest` (`session/load`) と `ListSessionsRequest` (`session/list`) が定義されているが未使用。session id は `println!` するだけで保存せず、再起動後に復元不可。静的セッション一覧と実 ACP セッションが結びつかない。

### E-2 `session-cancel-missing` [残タスク/high/no]
`CancelNotification` (`session/cancel`) と `StopReason::Cancelled` の表示処理が無い（crate schema `agent.rs:3171-3177` は cancel 時に Cancelled を返す MUST 規定）。生成中停止ボタンを置いても ACP 側へ届かない。B-23 と統合。

### E-3 `unstable-session-methods-and-feature-flags` [残タスク/high/no]
`session/fork`, `session/resume`, `session/close`, `session/set_model`, `logout` は crate 側で `unstable_*` feature gate され、Cargo.toml:9 は features 未指定のため**呼び出す型自体が存在しない**。実装以前に依存設定変更が必要（破壊的変更リスクの判断材料）。D-9 と重複。

### E-4 `initialize-capabilities-default` [改善/high/no]
全 probe が `InitializeRequest::new` = `ClientCapabilities::default()` / `client_info: None`。`fs.read_text_file` / `fs.write_text_file` / `terminal` が false の見込みで、レスポンスの `agent_capabilities` / `auth_methods` / modes も破棄。エージェントが「ファイルを読めない」と判断し得る（dsh 側の実際の挙動は未検証=推測）。

### E-5 `permission-request-unhandled` [残タスク/high/no] ★重要
crate `src/role/acp.rs:20-29`（Client 既定 handler = `Handled::No`）+ `src/jsonrpc/incoming_actor.rs:344-352`（未処理 request = `method_not_found()`）。`session/request_permission` のハンドラが無く、ファイル編集/コマンド実行の承認要求が自動拒否/エラーになる。README:27 の `cordis_permission_ui` は cordis-rs 側の別デモで ACP 経由ではない。**ACP ツール実行を実用的にする上で最も重要な欠落**。

### E-6 `fs-terminal-requests-unhandled` [残タスク/high/no] ★重要
`fs/read_text_file`, `fs/write_text_file`, `terminal/create`, `terminal/output`, `terminal/release`, `terminal/wait_for_exit`, `terminal/kill` の agent→client request も未処理 → 同じく `method_not_found`。「チャットはできるが作業はできない」状態の直接原因。E-5 と同じく未処理リクエスト全般。

### E-7 `dropped-session-update-variants` [残タスク/high/no] ★重要
`acp_stream_probe.rs:41-55` と `acp_stream_ui.rs:100-113` は `SessionUpdate::AgentMessageChunk` + `ContentBlock::Text` のみ。crate の 10+ バリアントのうち **9 つをドロップ**: `UserMessageChunk`, `AgentThoughtChunk`, `ToolCall`, `ToolCallUpdate`, `Plan`, `AvailableCommandsUpdate`, `CurrentModeUpdate`, `ConfigOptionUpdate`, `SessionInfoUpdate`（+ unstable `UsageUpdate`）。画像等の非 Text ContentBlock も落とす。ツール呼び出し・計画が UI に出ず、信頼性/デバッグ性が損なわれる。B-17/B-35/B-36 と統合。

### E-8 `notification-session-id-meta-ignored` [改善/high→影響medium/no]
`SessionNotification.session_id` / `_meta` を無視し `notification.update` のみ参照。共有バッファへ追記するため複数セッション化時にストリームが混線し得る（現状1セッションのみなので未発生=影響は推測）。

### E-9 `stderr-not-captured` [改善/high/no]
全 probe が stdin/stdout のみ `Stdio::piped()`（`acp_probe.rs:14-17` 等）。stderr は親に継承され、GPUI アプリでは UI に反映されない。dsh 側の起動失敗・認証エラー・ログがユーザーに見えない。GUI の診断性に直結。

### E-10 `process-lifecycle-no-graceful-shutdown-timeout-reconnect` [残タスク/high/no]
`child.kill()` のみ、graceful shutdown / timeout / 再接続なし。`send_request(...).block_task().await` にタイムアウトが無くハングすると固まる。spawn 失敗時もリトライなし。長時間デーモン化の直接要因。A-7/D-9 と重複。

### E-11 `model-insufficient-balance-handling` [改善/high/no]
全 error を `format!("error: {error}")` または `prompt error (model-side)` と決めつけ、JSON-RPC エラーとモデル応答の stop reason を区別していない。残高/レート/認証切れの分類とユーザー案内（残高チャージ等）が無い。README:54-55,93-101 が実際に遭遇したシナリオ。B-37 と重複。

### E-12 `auth-required-unhandled` [残タスク/high/no]
crate `error.rs` の `ErrorCode::AuthRequired` (-32000) / `ResourceNotFound` (-32002) に対する match が無く、`AuthenticateRequest` (`authenticate`) も送らない。認証必須バックエンド/マルチアカウントの前提。セキュリティ上も誤分類でないことの確認が必要。

### E-13 `no-retry-policy` [改善/high/no]
全 `acp_*.rs` に retry/backoff ループなし。`cordis_retry_ui` は HTTP 429 の agent-loop retry の別デモで ACP とは無関係。最適方針は未決定=推測。

### E-14 `llm-probe-per-prompt-spawn` [改善/high/no]
`acp_llm_probe.rs:1-5,22-59`。prompt 毎に dsh を再 spawn + initialize + session/new + kill しセッションを破棄。ツール結果を含む会話履歴が次呼び出しに引き継がれない。A-14 と同一。

### E-15 `stream-ui-polling-and-lock-panic` [改善/high/no]
`acp_stream_ui.rs:160-169` の 100ms 無限ポーリング + `:35` の `expect("stream buffer poisoned")`。poison で UI/ACP 双方のスレッドが panic し得る。`chunks.join("")` で毎フレーム全結合。実性能影響は未測定。A-7 と同一。

### E-16 `modes-config-commands-unhandled` [機能追加案/high/no]
`session/set_mode`, `session/set_config_option` と対応 update（mode/config/available_commands）が未対応。モデルのモード切替（例: plan mode）やスラッシュコマンド一覧の基盤。

### E-17 `session-info-title-not-linked` [機能追加案/medium/no]
`SessionInfoUpdate`（タイトル/タイムスタンプ）未対応。`cordis_title_ui` のタイトル生成デモと ACP セッションが分離。セッション一覧を実データで埋める低コスト接続点。B-7 と重複。

### E-18 `acp-probes-no-automated-tests` [改善/high/no]
全 `acp_*` に `#[cfg(test)]` なし、smoke からも除外（README:125-133）。外部プロセスと残高に依存する probe をモック ACP サーバ（in-memory transport）へ置換する等の分離が必要（方針は未決定=推測）。C-1/D-15 と同一。

ACP エージェントの優先度: (1) E-7 9バリアントのドロップ、(2) E-5+E-6 agent→client request が `method_not_found`、(3) E-1 セッション復元不能。

---

## F. `r1-cordis-analysis`（cordis-rs 統合深掘り、16項目）

一次情報: 当時 cordis-rs v0.62.0（commit 22290d9、ローカル checkout
`~/.cargo/git/checkouts/cordis-rs-.../22290d9/`）。現行 Cargo.toml:10 は
`v0.133.0`（2026-11-18）。全12 cordis バイナリ通読 + cordis-rs 実ソース照合。
Web 検索不要と判断し未実施。

### F-1 `cordis-command-ports-unmounted` [残タスク/high/no]
14再利用 port のうち command 系4種（compact/feedback/goal）+ tool 系2種（session_query, working_directory）が一切マウントされず。`provide_commands` は呼ばれるが登録が無く空。cordis-rs 側に実装済みで低コスト。

### F-2 `bridge-fake-command-compact` [残タスク/high/no]
`cordis_bridge.rs:34-36,44-57,87-91`。空 struct Compaction を inject し `/compact` を登録するだけ。実 port は `['commands','compaction','sessions']` を inject し SessionStore を要求。README:17 の「command-compact をマウント」は誤導。

### F-3 `scripted-llm-no-real-connection` [残タスク/high/no]
全 cordis デモが scripted/echo/stub provider。HTTP でモデルを呼ばない。実接続は `acp_llm_probe` のみで残高不足で本文未検証（README:89-94）。composition の DeepSeek provider は `<placeholder-key>` で mount のみ、リクエスト内容も無視。**PoC の本命が未配線**。

### F-4 `no-streaming-chunks-or-toolcall-stream` [残タスク/high/no]
cordis-rs の `StreamChunk` は Text/Error/Done のみ。`stream::once` で Text 1個、GUI は AgentRun 完了後にまとめて描画。function-call delta を表す型自体が無い（事実）。ACP 側ストリーミングとは別レイヤ。

### F-5 `tool-calls-serial-parallel-disabled` [調査必要/high/no]
agent-loop は `for` ループで await 直列（agent_loop.rs:231-235）。todo mount は `allow_parallel=false` で active>1 をエラー化。意図的か既定値継承かは未確認。実 LLM の複数 function call の並列性検証が必要。

### F-6 `permission-sandbox-not-enforced` [残タスク/high/no] ★重要
`SandboxMode` は cordis-rs 全体で permission_presets.rs と再 export にしか現れず、tool pipeline（tools.rs:305-328）は参照しない。read-only でも bash stub は ALLOWED。deny 分岐は死code。approval request は `"tool:bash"` だけで引数検査なし。**セキュリティ重要**。A-2/E-5/B-34 と統合。

### F-7 `retry-only-429-string` [残タスク/high/no]
cordis 側は [408,429,500,502,503,504] 対応だがデモは 429 のみ。未検証: 401/403, 408, 5xx, タイムアウト, Retry-After, retry 使い切り, jitter。`ToolPipeline::with_retry` は cordis-* から未呼出。本ファイルにテスト無し。

### F-8 `unload-remount-loses-plugin-state` [残タスク/high/no]
cordis-rs plugin.rs:87-105,153-164。unload は dispose_effects、再 mount は新規 Plugin → 内部状態（todo/titles/feedback/session イベント）復元されない。保存は `BTreeMap<String,bool>` の有効/無効フラグのみ。doc:4-5 の謳いと乖離。仕様意図は推測。

### F-9 `tmp-state-volatile-and-wiped` [改善/high/no]
`/tmp/cordis-gpui` を GUI 起動直前に `remove_dir_all` し自己矛盾。再起動で消え並行実行で衝突。A-16 と同一。

### F-10 `composition-config-ignored` [残タスク/high/no] ★重要
全 factory が `move |_ctx,_config|` で Default mount。`maxDepth:1`, `includeDefaultRoots:false`, `maxNoteBytes:1024`, `compression:zstd/none` 等 fixture 設定が**一切効いていない**。README:103-107 の「実 composition を読み込んだ」に対し config 無視の部分 mount。

### F-11 `core-services-unprovided` [残タスク/high/no]
26コアサービスのうち11しか provide されず。未使用: sessions, compaction, goals, systemPrompt, toolResultPruner, tokenMeter, sessionQuery, workspace, storage, agents, otel, retry。AgentLoop の session/compaction/goals/system_prompt/checkpoint オプションも未接続。

### F-12 `feedback-persistence-subagent-mounted-not-exercised` [機能追加案/high/no]
message-feedback / session-persistence / subagent / skill-fs は Fiber Active だが、feedback 送信・永続化ラウンドトリップ・subagent 起動・skill 検出描画が無い。D-11 と同一（cordis 側 mount 証拠を補強）。

### F-13 `gui-boilerplate-no-shared-abstraction` [改善/high/no]
11バイナリが runtime build→Context::root→Application::run→open_window(Default)→eprintln を反復。色リテラルと div 骨格も同一。echo_provider 逐語複製、PluginRegistry 登録14回重複。**既知のテーマ切替時に11ファイルの色を個別修正する必要**があり、UiTheme/WindowRunner/TextInput 抽出が先決。A-3/A-4/A-17 と統合。

### F-14 `toggle-click-mixed-executors` [改善/high/no]
3デモがクリックハンドラで `futures::executor::block_on` と Tokio runtime 生成を混在させ UI をブロック。`#[tokio::main]` 下で futures::block_on は将来 Fiber が Tokio 依存 future を返すとデッドロック化し得る（推測）。`cordis_prompt_agent_ui` は正しく `cx.spawn`。A-17 と同一。

### F-15 `cordis-tests-only-six-of-twelve` [改善/high/no]
当時 12 cordis バイナリ中6にしかテストが無かったが、現行は 12 すべてに
テストが存在（C-1/D-15 と同一）。README:68 は1本（todo_ui）しか記載。
テストは data-model 層が中心で Render/ボタン/window 生成は未検証。

### F-16 `title-titles-in-memory-only` [改善/high/no]
`cordis_title_ui.rs:24-70,100`。sessionTitles → Vec<TitleRow> に写像するだけ。永続化 service と初回発言で確定する実フローへ未接続、再起動で消える。B-7/E-17 と重複。

cordis エージェントの優先度: (1) F-1+F-2 port 未活用と /compact スタブ、(2) F-6 sandbox 未強制/deny 到達不能、(3) F-3+F-4 全 stub とストリーム1チャンク/直列 tool。

---

## ラウンド1 集計（親エージェント取りまとめ）

| 観点 | 検出数 | 主な貢献 |
| --- | --- | --- |
| A: コード監査 | 20 | doc/実装乖離、everything ハードコード、重複3系統 |
| B: UX 発想 | 43 | セッション CRUD/データモデル、IME、入力基盤、デスクトップ統合 |
| C: 独自確認 | 3 | baseline tests、非公開設計資料、周辺 PoC |
| D: ドキュメント/git | 15 | 依存ピン不在、version 遅れ、session/close 欠落、fixture 未活用 |
| E: ACP 深掘り | 18 | 9バリアント ドロップ、permission/fs/terminal `method_not_found`、capabilities 既定 |
| F: cordis 深掘り | 16 | config 無視、sandbox 未強制、scripted LLM、core service 11/26 |

**合計 115項目**（重複含む）。重複の例: テストカバレッジ（C-1/D-15/E-18/F-15/ACP-no-tests）、`cordis` 重複（A-3/A-4/F-13）、permission（A-2/E-5/F-6/B-34）、session 永続化（B-2/E-1/F-16）、/tmp（A-16/F-9）、ストリーミング polling（A-7/E-15）、fixture 未活用（D-11/F-12）、タイトル（B-7/E-17/F-16）。

**ラウンド1 の三大事実（他ラウンドへ引き継ぐ）**:
1. **ACP agent→client request が全て `method_not_found`**（E-5/E-6）— permission / fs / terminal が未実装で、「チャットはできるが作業はできない」。
2. **cordis-rs の fixture config が factory で全て捨てられている**（F-10）— 「実 composition 統合」の実質が限定的。
3. **入力基盤の日本語対応欠落 + 9つの session/update バリアントのドロップ**（B-19/E-7）— 日本語ユーザー向けクライアントとしてのボトルネック。

**エージェントチーム利用の反省**: `r1-acp-analysis` が一回 failed 通知を受けたが、再メッセージで回収できた（教訓: failed を無視せず status 確認＋再促す）。Web 検索は ACP エージェントで失敗、他はローカル crate ソースを一次情報として代替（AGENTS.md §1/§16 の「ローカル情報優先」に適合）。


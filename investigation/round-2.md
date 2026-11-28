# ラウンド2: 外部/上流リサーチ + 検証戦略

日時: 2026-11-17T19:51+09:00 頃
起動エージェント: `r2-dsh-upstream`, `r2-gpui-caps`, `r2-existing-solutions`, `r2-runtime-verification`, `r2-cross-project`（5並列）
方針: ラウンド1が「リポジトリ内部の静的解析」だったのに対し、本ラウンドは **リポジトリ外の一次情報**（上流 DSH、ローカル crate ソース、兄弟プロジェクト、実行環境）から未知の残タスク/機能案を掘り起こす。

## E. `r2-cross-project`（兄弟プロジェクト横断調査、15項目）

### E-01 `brother-projects-inventory` [調査必要/high/yes]
8プロジェクト棚卸し。要点: `work/gpui-task-manager`（Apache-2.0、201 tests、XDG 設定、Catppuccin テーマ、**IME 対応**、HEAD b7cba1a）、`work/gpui-file-explorer`（AGPL-3.0、仮想リスト/タブ/検索/プレビュー、167 tests）、`work/bitwarden-gpui-poc`（AGPL-3.0、設定/TOTP/Zeroizing）、`work/dsh-tui`（git なし、TypeScript TUI）、`dsh-ntfy-notify`（git なし、JS 通知プラグイン）、`dsh-resilient-retry`（Cordis patch）、`dsh-config-export`、`security-review/Argus`（外部 OSS・Windows 専用）。

### E-02 `gpui-stack-fragmentation` [調査必要/high/yes] ★重要
本 PoC は生 `gpui =0.2.2`、兄弟3プロジェクトは `gpui-kit =0.9.1`（内部 gpui-pre 0.3.12 + gpui-component）。API が異なるため **UI コードの直接共有は不可**。共通クレート化の前提はバージョン統一。現実的のは pure logic（IME 状態機械等）の移植のみ。

### E-03 `reuse-theme-palette` [改善/high/yes] ★重要
`work/gpui-task-manager/app/src/theme.rs:12-95` の `Palette` / `DARK` / `LIGHT` / `set_theme` / `palette()` / `usage_color()`（AtomicU8 + widget 側は `rgb(palette().mantle)`）。本 PoC は11バイナリ・80+箇所のハードコード `rgb(0x…)`（round-1 F-13）。**Apache-2.0 → MIT PoC へ帰属明記で移植可**。既知「テーマ切替」の前提条件。

### E-04 `reuse-ime-filter-input` [機能追加案/high/yes] ★重要
`work/gpui-task-manager/crates/tm-viewmodel/src/filter_input.rs:1-14,29-106,108-120` の UTF-16 marked range + `EditOutcome::{Unchanged, Composing, Committed}` + `display_parts()` / `committed_text()`。PoC の `cordis_prompt_ui.rs:144-155` は Enter 常に送信で日本語確定が送信される（round-1 B-19）。**pure logic で GPUI バージョン非依存、Apache-2.0、移植可能**。B-24 のカーソル/選択もカバー。

### E-05 `reuse-widget-viewmodel-pattern` [改善/medium/yes]
`gpui-task-manager/app/src/ui.rs:1-142` の stateless widget + `on_click` 注入、viewmodel reducer が状態所有、シェルは Command を投げる構成。PoC は各デモが state と Render を直書き（F-13 と表裏）。仮想リストの具体的描画コードは未精読（confidence medium）。

### E-06 `reuse-atomic-xdg-settings` [改善/high/yes]
`gpui-task-manager/app/src/settings.rs:31-64,88-131`。XDG/APPDATA/macOS + PID 付き tmp + rename の原子的書き込み + `.mode(0o600)` + `SETTINGS_VERSION`。PoC は `/tmp/cordis-gpui` を起動直前に `remove_dir_all`（F-9）。Apache-2.0。

### E-07 `reuse-lucide-icons` [機能追加案/high/yes]
`gpui-file-explorer/app/src/icons.rs:1-92` の拡張子→種別→Lucide `IconName` 決定的マッピング＋テスト。ただし **AGPL-3.0 で MIT PoC へ直接コピー不可**。設計参考のみ。

### E-08 `reuse-window-options` [改善/high/partial]
`gpui-task-manager/app/src/main.rs:30-65`。`application().with_assets`, `gpui_kit::init`, `on_window_closed`→quit, `Bounds::centered`, `WindowOptions{titlebar, app_id, focus, show}`, `activate(true)`。PoC は `cx.open_window(Default::default(), …)` のみ（main.rs:131-136）。トレイ/グローバルホットキーの実装は兄弟にも見当たらず（B-26/B-27 未解决）。

### E-09 `acp-layer-is-poc-first` [調査必要/medium/partial]
兄弟の Cargo.toml に `agent-client-protocol` 不在。兄弟は HTTP ループバック (`tm-agent`) または in-process (dsh-tui) で接続。**「共通 ACP 接続レイヤ」は兄弟から再利用できず、むしろ PoC が先発**。レポート §3.1 の接続/認証再実装課題は PoC 固有。

### E-10 `reuse-ntfy-notification-design` [機能追加案/high/yes] ★重要
`dsh-ntfy-notify/lib/index.js:54-63,169-236,238-277,317-415`。「見ているセッションは通知しない」「30分メモリで dedupe」「子セッション完了は親に集約」「通知失敗で本体の waterfall を止めない (`next()` 必ず)」「本文600字 clip・非 ASCII は `X-Title` RFC 2047」。B-28 へ転移可能。**送信自体は既存プラグインへ委譲すれば新規依存不要**。

### E-11 `reuse-retry-policy-config` [改善/high/yes] ★重要
`dsh-resilient-retry/cordis.patch.yml:9-29,490-507` の `retryPolicy.mode: normal`, `maxRetries: 10`, `retryableCodes` に AUTH/QUOTA/MISSING_CREDENTIAL/INVALID_CREDENTIAL 追加, backoff 1000/30000/0.2。**自作ループを書かず DSH 標準 `@deepseek-ai/dsh-llm-retry` を provider route 単位で設定**する好事例（AGENTS.md §2 適合）。ただし README:32 の「agent ループ経由のみ、直接 `ctx.llm.stream()` は single-attempt」制約は PoC probe に直接効く。課金あり。

### E-12 `plan-a-vs-plan-c-integration` [調査必要/high/partial]
レポートは案A（独立ツール群）推奨、ユーザーは案C選択。両者は排他ではなく「案C クライアントが ACP で DSH 接続 + 案A ツール群が独立バイナリ併存」が整合解。共通クレート化・共有テーマ・共通 ACP レイヤの具体案はレポートに無く（D-14）、意思決定が必要。

### E-13 `license-boundaries` [調査必要/high/yes]
Apache-2.0 (task-manager) / AGPL-3.0 (file-explorer, bitwarden-poc, Argus) / MIT (PoC) の混在。AGPL の `icons.rs` / `settings.rs` を PoC へ直接持ち込み不可。task-manager は「AGPL からコードをコピーせず調査ノートのみ参照」と明記しており、これを踏襲するのが安全。

### E-14 `cargo-profile-already-cross-repo` [改善/medium/no]
兄弟4リポジトリ + PoC で同一の GPUI 向けプロファイル（`debug="line-tables-only"`, `debug=false` for deps, `strip=true`）が統一済み（各 "chore: Cargo プロファイル設定を統一"）。共通化の前例になる。

### E-15 `argus-and-dsh-tui-excluded` [調査必要/low-medium/yes]
Argus（外部 OSS・Windows 専用・レビュー対象）、dsh-tui（GPUI でない TypeScript TUI）は共通基盤候補から除外。設計参考のみ。

**再利用推奨リスト**: ① `gpui-task-manager/app/src/theme.rs`（Apache）② `crates/tm-viewmodel/src/filter_input.rs`（Apache・IME）③ `app/src/settings.rs` + viewmodel settings（Apache・原子的保存）④ `app/src/main.rs:30-65`（WindowOptions）⑤ `app/src/ui.rs`（widget パターン）⑥ `dsh-ntfy-notify/lib/index.js`（通知設計・委譲）⑦ `dsh-resilient-retry/cordis.patch.yml`（retry 設定）⑧/⑨ file-explorer の icons/settings（**AGPL・設計参考のみ**）。

---

## D. `r2-runtime-verification`（検証戦略の具体案、18項目・ Runtime Proof 付き）

### D-1 `rv-verified-is-handshake-only` [残タスク/high/yes]
README:49-55,66-69,89-101。ACP「確認済み」は stop reason まで、本文未検証。GUI も `eprintln!("GPUI window created")` のみ。round-1 D-1/D-2 と同一。

### D-2 `rv-tests-6-of-19` [改善/high/no]
`cargo test --bins --quiet` → `PIPE_RC=0` だがテストは6ファイルのみ。全 `acp_*`, `acp_stream_ui`, permission/retry/title, `main.rs` にテスト無し。

### D-3 `rv-smoke-grep-only` [改善/high/yes]
`scripts/smoke-gui.sh:2-4,21-36`。12デモを `timeout 20` 起動し stderr grep のみ。Render 結果・ボタン操作・ウィンドウ実在は未検証。CI 外。

### D-4 `rv-tests-data-model-only` [改善/high/no]
既存テストは plugin seam / tool invoke / 状態遷移の確認に留まり、`cx.open_window` 結果や Render 出力を検証するテストが無い。`cordis_todo_ui.rs:197` は `Ok(_window)` を捨てる。

### D-5 `rv-docs-stale-vs-code` [改善/high/yes]
README:109-113 はストリーミング未実装とするが commit 57dbc01/2a60bc7 で実装済み（round-1 D-10 と同一）。残るのは実データ検証1点に絞れる。

### D-6 `rv-no-wayland-cli` [調査必要/high/yes]
`command -v` 実測: kdotool/ydotool/wtype/grim/slurp/swaymsg/xdotool/wmctrl = 全て`(none)`。`gdbus`, `qdbus6`, `xwininfo`, `xprop`, `spectacle 6.6.6`, `kscreen-doctor`, `busctl`, `dbus-send` は存在。環境: `DISPLAY=:1`, `WAYLAND_DISPLAY=wayland-0`, `XDG_SESSION_TYPE=wayland`, KWin 6.6.6。**「ディスプレイが無い」のでなく「Wayland 操作 CLI が無い」だけ**。

### D-7 `rv-xwayland-enumeration-only` [残タスク/high/yes]
`xwininfo -root -tree` は Xwayland クライアント30個（Bitwarden 等）のみ、GPUI 不在。`xprop -root _NET_CLIENT_LIST` も同様。GPUI は Wayland ネイティブ。**「xwininfo で見えない=起動失敗」と誤判定しない境界线**。

### D-8 `rv-kwin-query-interactive` [調査必要/high/no]（否定結果）
`gdbus call ... org.kde.KWin.queryWindowInfo` → `GDBus.Error:org.kde.KWin.Error.UserCancel: User cancelled the query`。メソッドは存在するがユーザーがウィンドウをクリック選択する方式で**無人化不可**。`getWindowInfo` も window id 別途必要。

### D-9 `rv-spectacle-screenshot-works` [改善/high/no] ★Runtime Proof
`spectacle -b -n -o /tmp/r2-verify-$$.png` → rc=0、全画面（環境依存）の PNG（8-bit/color RGBA、寸法・サイズは環境依存、複数モニタ構成と整合）。裏側は `org.kde.KWin.ScreenShot2` の `CaptureActiveScreen/CaptureWindow/CaptureArea`。**Wayland ネイティブ画面の非対話スクリーンショット証跡が今日から可能**。撮影後ファイルは削除済み。

### D-10 `rv-portal-input-injection` [調査必要/medium/no]
`gdbus introspect ... /org/freedesktop/portal/desktop` に `NotifyPointerMotion`, `NotifyPointerButton`, `NotifyKeyboardKeysym`, `NotifyTouchDown/Motion/Up`, `CreateSession`, `SelectDevices`, `Start`, `Screenshot` が存在。実注入は未試行。初回にユーザー承認ダイアログを伴う可能性（推測・要検証）。ボタン押下自動化の唯一のローカル経路候補。

### D-11 `rv-kwin-scripting-enumeration` [調査必要/medium/no]
`gdbus introspect ... /Scripting` に `loadScript(filePath, pluginName)` / `loadDeclarativeScript` / `unloadScript` が存在（未実行）。JS/QML で `workspace.clientList()` を列挙し結果を D-Bus/ファイルへ。リポジトリ外へのスクリプト配置と KWin 再起動リスクがあり今回は未実行。

### D-12 `rv-cordis-mock-provider-exists` [改善/high/yes]
pinned checkout `22290d9/src/services/llm.rs:192`（`from_async_text`）`:219`（`echo_provider`）、`lib.rs:125` 再 export。`acp_llm_probe.rs:17-19,67-76` が既に使用。**モック LLM は「無い」のでなく「使われ方が誤っている」**。新規依存なしで agent-loop をオフライン化可能。

### D-13 `rv-stop-reason-as-answer-body` [残タスク/high/yes] ★重要
`acp_llm_probe.rs:46-55`。`Ok(format!("{:?}", response.stop_reason))` を LLM 回答本文として agent-loop に渡し `:67-85` の provider がそれを返す。**残高が復旧しても「prompt → session/update → GUI 表示」の本文検証は構造的に不可能**。E2E テストより先に修正すべき。

### D-14 `rv-acp-in-memory-channel` [改善/high/no] ★重要
`agent-client-protocol-0.11.1/src/jsonrpc.rs:3330-3385` の `pub struct Channel` / `Channel::duplex()`（mpsc unbounded ペア）、`src/lib.rs:114` で crate root 再 export。`ByteStreams` の代わりに `Client.builder().connect_with(Channel::duplex())` すれば **`dsh --profile acp` も残高も不要**で ACP プロトコルを往復できる。ドック内の `MockTransport` はローカル registry に存在せず（grep 0件）が、Channel で代替可。**新規依存ゼロ**。

### D-15 `rv-mock-e2e-design` [改善/medium/no]
4段構成（未実装・未コンパイル検証）: ① `Channel::duplex()` で mock Agent を接続し `initialize`/`session/new` に応答、`session/prompt` 受領時に `SessionNotification{AgentMessageChunk(TextContent::new("MOCK_ANSWER"))}` を送出 ② prompt 本文と session_id を `Arc<Mutex<Vec<_>>>` に記録 ③ `acp_stream_ui` の chunk 追記を純粋関数 `append_chunk(state, &notification)` へ切り出し ④ cordis 側は `echo_provider`/`from_async_text` で `run.final_text == "MOCK_ANSWER"` を assert。`#[tokio::test]` で `cargo test --bins` に載る。残高0円・外部プロセス0・CI 対応。

### D-16 `rv-gates-covered` [改善/high/no]
`.github/workflows/ci.yml:18-25` → `Seduce0068/workflows/rust-gates.yml@v1` を web_fetch で確認（HTTP 200）。ステップ: fmt → check → test → clippy → audit(任意) → extra_gates。`locked` 既定 true、`audit` 既定 false、runner ubuntu-24.04（ディスプレイ無し）。

### D-17 `rv-gates-gaps` [改善/high/no]
`cargo test --all-targets` は doctest を含まない（`cargo test --doc` が必要）、GUI smoke は原理的に CI 外、残高依存 probe も CI 外、`cargo audit` 無効。**「CI green = 検証済み」と思い込むリスク**。

### D-18 `rv-extra-gates-proposal` [改善/high/no]
呼び出し側 `.github/workflows/ci.yml` の `with:` に `audit: true` と `extra_gates: |`（`cargo test --doc --locked` 等）を追加する案。**共通ワークフローを変更せず呼び出し側だけでゲート強化**（AGENTS.md §5 に整合）。

**検証ロードマップ（コスト対効果順）**: ① RV-15+RV-13（mock E2E + stop_reason 誤用修正）② RV-09+RV-10（spectacle 証跡 runbook 化 + portal 実検証）③ RV-02+RV-18（テスト拡充 + extra_gates）④ RV-11（KWin Scripting）⑤ RV-01/03/05（README/smoke 記述修正）。

---

## B. `r2-gpui-caps`（gpui 0.2.2 ローカルソース実力調査、17項目）

一次情報: `{cargo-registry}/src/.../gpui-0.2.2`（Zed 製、vcs sha `69e2130295...`）。外部検索なし。

### B-A1 `gpui-no-builtin-text-widget` [high/yes]
`src/gpui.rs:10-46`, `src/input.rs:4-9`, `examples/input.rs:29-40`。完成済み TextInput widget は無い。ただし「入力基盤が無い」とは同義でない（B-A2/A3）。

### B-A2 `gpui-ime-handler-exists` [high/no] ★重要
`src/input.rs:10` (`EntityInputHandler`), `:77` (`ElementInputHandler<V>`)、`src/platform.rs:991-1075` (`InputHandler` =「Zed's interface for handling text input from the platform's IME system」「1:1 exposure of the NSTextInputClient API」; `:1043 replace_and_mark_text_in_range` = setMarkedText, `:1054 unmark_text`, `:1011 marked_text_range`, `:1060 bounds_for_range`)、`src/window.rs:3400 handle_input`, `:4126 update_ime_position`。**日本語 IME は「gpui の欠落」でなく「PoC の未接続」**。PoC は全バイナリで `open_window(Default::default(), ...)` のみ。

### B-A3 `gpui-input-example-complete` [high/no]
`examples/input.rs:1-40,29-90`。`TextInput` に focus_handle, content, placeholder, selected_range, selection_reversed, marked_range, last_layout。Left/Right/SelectLeft/SelectRight/SelectAll/Home/End/Backspace/Delete actions とハンドラ、UTF-16 変換、クリップボード、composition。**round-1 B-24 の直接移植元**。

### B-A4 `gpui-keystroke-fields` [high/partial]
`src/platform/keystroke.rs:18-33` (`Keystroke { modifiers, key, key_char }`), `:447-470` (`Modifiers` 5 booleans: control/alt/shift/platform/function), `:229 is_ime_in_progress()`, `:82 should_match`。IME 状態は `InputHandler`/`marked_range` 側で管理する設計。`KeystrokeEvent` に composition フィールドは無い。

### B-A5 `gpui-x11-xim` [medium/no]
`src/platform/linux/x11/xim_handler.rs:122-124`, x11 feature の `xim = "0.4.0-zed"`。ファイル存在と依存宣言のみ、実動作・Wayland 側は未確認。

### B-B1 `gpui-no-markdown-core` [high/partial]
core 全体を `markdown|syntect|pulldown|comrak|highlight` で grep → markdown は doc コメント2件のみ。`src/style.rs:493-`, `:950 combine_highlights`, `src/elements/text.rs:188 with_highlights` の範囲装飾プリミティブはある。**Range→HighlightStyle を生成するパーサは PoC 側で用意が必要**。

### B-B2 `markdown-crates-in-local-cache` [medium/no]
`{cargo-registry}/src/.../` に `comrak-0.28.0/0.59.0`, `pulldown-cmark-0.12.2/0.13.4/0.13.5/0.13.6`, `markdown-1.0.0`, `syntect-5.3.0`, `tree-sitter-0.24.7/0.27.1`。PoC Cargo.lock には0件。**オフライン追加可能、未選定**。

### B-B3 `gpui-component-incompatible` [high/partial] ★戦略判断
`gpui-component-0.9.1/Cargo.toml:278-280`: `[dependencies.gpui] version = "=0.3.12" package = "gpui-pre"`。gpui-pre は「zed@d2a4fb6 のスナップショット」、本 PoC の gpui 0.2.2 は `69e2130...` で**別 crate**。Markdown/Input/仮想リストを揃えるが直接混用不可。再利用できるのは pure logic のみ（round-2 E-02 と同結論）。

### B-C1 `gpui-uniform-list-exists` [high/partial]
`src/elements/uniform_list.rs:1-7,22`（「only render the visible subset of items」）、`src/elements/list.rs:1-7,24`（可変高アイテム、`ListState::splice`/`reset`）、examples/uniform_list.rs, data_table.rs, tree.rs。**round-1 B-9「仮想化は調査必要」を解消: API は既に利用可能**。

### B-D1 `gpui-no-accesskit-atspi` [high/partial]
gpui 0.2.2 の src/Cargo.toml/Cargo.lock を `\baccesskit\b|\batspi\b|\borca\b` 厳密 grep → **0件**。近傍は `Focusable`/`FocusHandle`/`src/tab_stop.rs`（キーボードフォーカスのみ）。**「何が公開されるか」=「何も公開されない」**。必要なら外部 crate/GTK bridge 自前統合（推測）。非公開 §3.1 の未確認を解消。

### B-E1 `wasm32-build-fails` [high/no] ★Runtime Proof
`rustup target list --installed` に wasm32-unknown-unknown あり。`cargo check --target wasm32-unknown-unknown -p gpui` → **exit 101**、`error: The target OS is "unknown" or "none", so it's unsupported by the errno crate.`（`errno-0.3.14/src/sys.rs:8` の compile_error）。`cargo tree -i errno -p gpui --target wasm32-unknown-unknown`: `errno 0.3.14 → rustix 1.1.2 → async-io 2.6.0 → smol 2.0.2 → gpui 0.2.2`（`gpui.rs:94 pub use smol::Timer` で smol は非 optional）。**非公開 §5「wasm32 可否」を「現状不可」へ確定**。feature 回避は `--no-default-features` が Cargo 制約で未検証（smol 非 optional なので困難と推測）。

### B-F1 `gpui-clipboard-api` [high/partial]
`src/app.rs:1041 write_to_clipboard`, `:1053 read_from_clipboard`, `:1036 write_to_primary`, `:1048 read_from_primary`; `src/platform.rs:1504 ClipboardItem`（`:1544` 画像から生成, `:1621 From<Image>`, `:1742` clipboard image → ImageData）。round-1 B-24 は PoC 側の未接続。

### B-F2 `gpui-drag-drop-api` [medium-high/partial]
`src/elements/div.rs:499 on_drag`, `:282 on_drag_move`, `:62 DragMoveEvent`, `src/window.rs:3626` / `div.rs:2166` `active_drag = Some(AnyDrag {...})`, `examples/drag_drop.rs`。非公開 §5 の「D&D 難度」は受け取り口が既に存在するため PoC 側の接続作業に落ちる（推測含む）。

### B-F3 `gpui-image-api` [medium-high/partial]
`src/elements/img.rs:198 img()`, `:227 image_cache()`, `src/elements/image_cache.rs:15`, `src/elements/svg.rs:18 svg()`, examples/image/, image_gallery.rs, image_loading.rs, gif_viewer.rs。画像プレビューは gpui 標準機能（§5 難度事項を解消）。

### B-F4 `gpui-window-options-14-fields` [high/partial]
`src/app.rs:943 open_window`, `src/platform.rs:1089-1135 WindowOptions`（window_bounds, titlebar, focus, show, kind, is_movable, is_resizable, is_minimizable, display_id, window_background, **app_id**, window_min_size, window_decorations, tabbing_identifier）, `src/window.rs:1779 set_window_title`, `:4389 window_title`。PoC 全11バイナリが Default::default()。round-2 E-08 と同結論。

### B-F5 `gpui-no-os-notification` [medium/no]
`src/window.rs:4141 prompt(level, message, detail, answers, cx)`（プラットフォーム確認ダイアログ）, `src/app.rs:1116 prompt_for_paths`, `:1129 prompt_for_new_path`。`src/platform.rs` に notification 系 API 0件。`cx.notify()` は Entity 再描画通知で OS 通知ではない。**デスクトップ通知は外部委譲が必要**（E-10 の ntfy 案と整合）。

---

## C. `r2-existing-solutions`（既存ソリューション調査、24項目: 採用3 / 条件付き5 / 不採用15 / 補足1）

証拠の優先順: ローカル crate ソース > crates.io API > 兄弟プロジェクト > 外部検索。web_search は DeepSeek API 残高不足 (HTTP 402) で不能使用だったため、外部情報は crates.io API と crate 同梱 README/Cargo.toml を出典とする。

### C-1 `cand-acp-channel-duplex` [改善・**採用**/high/no]
`agent-client-protocol-0.11.1/src/jsonrpc.rs:3330-3424`（Channel 定義・duplex()・ConnectTo<R> for Channel）、`src/component.rs:37-56`。`Channel::duplex()` が mpsc unbounded ペアを返し外部プロセス/ソケット/stdio 不要。モック agent は `Agent.builder().on_receive_request(...)`（examples/simple_agent.rs:7-19）で作れる。**追加依存ゼロ、ライセンス Apache-2.0（MIT PoC と互換）**。

### C-2 `cand-acp-mocktransport` [調査必要・**不採用**/high/no]
`agent-client-protocol-test::MockTransport` は dev-dependency / workspace 内テスト用 crate。ローカルキャッシュ・PoC Cargo.lock・registry index に存在せず依存追加不可。代替は C-1。

### C-3 `cand-acp-tokio-duplex` [改善・**条件付き**/high/no]
`tokio::io::duplex` + ByteStreams で実バイト列の newline-delimited JSON を模擬。ワイヤーフォーマット自体を検証したい場合のみ。

### C-4 `cand-gpui-component` [機能追加案・**不採用（バージョン非互換）**/high/yes]
crates.io API: 0.10.0 published 2026-11-16T06:45Z、累計 628,242 DL、Apache-2.0、39 versions、週次リリース。**しかし `gpui-component-0.9.1/Cargo.toml:278-280` が `[dependencies.gpui] version = "=0.3.12" package = "gpui-pre"`**。本 PoC の `gpui = "=0.2.2"` と型レベルで別 crate で、同時依存すると Window/Context/Element が衝突しビルド不能。60+ コンポーネント（Input, Markdown TextView, syntect ハイライト, notification）は十分だが届かない。条件付き例外: POC 全体を gpui-pre 0.3.12 + gpui-kit 0.9.1 へ移行するなら採用可（兄弟3プロジェクトは移行済み）。

### C-5 `cand-gpui-kit` [不採用（バージョン非互換）/high/no]
内包する gpui-component が gpui-pre 0.3.12 依存。兄弟3プロジェクトの共通 UI スタックだが PoC と共存不可。

### C-6/C-7/C-8 `gpui-rsx / adabraka-ui / ChatGPUI` [不採用（未確認）/low/no]
ローカルキャッシュになく crates.io API / GitHub MCP で詳細取得不可。エコシステムが gpui-pre 側である点から非互換の可能性が高い（推測）。時間制約で深掘りせず。

### C-9 `cand-gpui-022-input-handler` [調査必要・**採用（既存 API の利用）**/high/yes]
`gpui-0.2.2/src/input.rs:1-180`（`EntityInputHandler`, `ElementInputHandler<V>`, `impl<V: EntityInputHandler> InputHandler for ElementInputHandler<V>`）。UTF-16 selection・IME marked range・paste/replace を含む低レベル API は 0.2.2 にも存在。**「テキスト入力が一切無い」わけでなく「高レベル Widget が無い」のが正確**。追加依存なし（round-2 B-A2/A3 と同結論）。

### C-10 `cand-pulldown-cmark` [機能追加案・**条件付き採用**/high(存在)/medium(レンダラ無確認)/yes]
ローカルキャッシュに 0.12.2 / 0.13.4 / 0.13.5 / 0.13.6。Markdown → イベント/AST の定番パーサ（MIT・依存小）。AST→GPUI Element 部分は自作が必要。

### C-11 `cand-comrak` [条件付き採用/high/no]
ローカルキャッシュ 0.28.0 / 0.59.0。GFM 対応の富んだ AST（テーブル・脚注）。拡張要件が出た場合のみ。

### C-12 `cand-markdown` [条件付き採用/high/no]
ローカル 1.0.0、gpui-component-0.9.1/Cargo.toml:315-317 が使用。優先度低。

### C-13 `cand-egui-commonmark` [不採用（要件不一致）/medium/no]
egui 専用で GPUI では使えない。設計参考のみ。

### C-14 `cand-gpui-component-markdown` [不採用（バージョン非互換）/high/yes]
完成度は極めて高いが C-4 と同じ理由で届かない。

### C-15 `cand-tray-icon` [機能追加案・**条件付き採用**/medium/no]
`tray-icon-0.25.1/README.md:7,12,17-19`。`ksni` feature で SNI/D-Bus backend、KDE Plasma は SNI ネイティブサポートで Wayland でも動作見込み。デフォルトの libappindicator は X11 依存で要件不一致。GNOME は SNI 非対応（拡張必要）だが本件は Kubuntu/Plasma に限定。

### C-16 `cand-global-hotkey` [不採用（要件不一致）/high/no]
`global-hotkey-0.8.0/README.md:3-7` が **"Linux (X11 Only)"** と明記。Kubuntu Wayland 環境と不一致。

### C-17 `cand-ashpd-global-shortcuts` [機能追加案・**条件付き採用**/medium/no]
`ashpd-0.13.13/src/desktop/global_shortcuts.rs`。XDG portal `org.freedesktop.portal.GlobalShortcuts` 経由で Wayland ネイティブのショートカット登録が可能。ただし compositor 依存で GPUI イベントループとの統合（D-Bus signal poll）が新規作業。まず Plasma の portal 応答を PoC で確認すべき。

### C-18 `cand-notify-rust` [機能追加案・**採用**/medium-high/no]
`notify-rust-4.19.0/README.md:67`（zbus 使用、XDG 向け）。freedesktop Notifications の D-Bus 実装、Wayland/X11 問わず通知可、依存比较小、MIT/Apache-2.0。注意: `gpui-file-explorer/app/Cargo.toml:16` の `notify = "8.2"` はファイルシステム監視用で別 crate。

### C-19 `cand-tao` / C-20 `cand-winit` [不採用（依存過大・要件不一致）/medium/no]
GPUI が独自 Platform を持つため第二のイベントループを追加する形になり Wayland でフォーカス/イベント競合リスク。tray は C-15、hotkey は C-17 で十分。

### C-21 `cand-sibling-filterinput` [改善・**採用**/high/no] ★重要
`work/gpui-task-manager/crates/tm-viewmodel/src/filter_input.rs:1-45`（依存は tm-core, serde のみ = **GPUI 非依存**）、参照実装 `app/src/app.rs:1956-2070` の `impl EntityInputHandler`。UTF-16 オフセット・IME 未確定 marked range・選択範囲・確定文字列のみ反映する純粋状態機械、単体テスト多数。**Apache-2.0 で MIT PoC と互換**。gpui 0.2.2 の `EntityInputHandler` と組み合わせれば IME 対応入力を短期実装可能。

### C-22 `cand-sibling-gpui-ui` [不採用（バージョン非互換）/high/no]
兄弟3プロジェクトの UI コードは全て `gpui-kit = "=0.9.1"`（gpui-pre 0.3.12 系）。API 設計の参考にはなるが混入不可。

### C-23 `cand-zed-checkout` [不採用（依存過大）/high(存在)/low(移植性)/no]
ローカル `work/zed/_ref/` に markdown/agent_panel の完全な参照実装があるが Zed 本体のクレート群に深く結合。設計参考のみ。

### C-24 `cand-dsh-tui` [不採用（未確認）/low/no]
TUI 実装で再利用要素を検出できず。

**採用推奨トップ3**: ① C-1 `Channel::duplex()`（追加依存ゼロで残高/外部プロセス依存を排除）② C-21 `FilterInput` + gpui 0.2.2 `EntityInputHandler`（IME 対応入力を短期実装）③ C-18 `notify-rust`（Wayland 対応・数行で実現、ROI 高い）。次点 C-15 tray-icon(ksni), C-17 ashpd。

**自作時の記録義務（AGENTS.md §2）**: Markdown→GPUI 変換で pulldown-cmark を採用し AST→Element を自作する場合、「gpui-component を不採用とした理由は gpui-pre =0.3.12 によるバージョン非互換」を ADR またはコードコメントに残すこと。

---

## A. `r2-dsh-upstream`（上流 DSH が実際に送る/受け取るもの、14項目）

`dsh-doc` スキルを読み込み、npm インストール `${HOME}/.npm-global/lib/node_modules/@deepseek-ai/dsh`（v0.2.2-rc.3）の同梱 `src/*.ts`、README、ACP プロファイル設定、CLI ヘルプ、公式 GitHub README を一次情報とした。**静的ソースに基づき、実 JSON-RPC トレースは未実施**。

### A-01 `dsh-emits-six-update-variants` [改善/high/yes]
`dsh-acp/src/updates.ts:26,36,54,80,98`, `session.ts:197`, `index.ts:124-126`。実送信は `agent_thought_chunk` / `agent_message_chunk` / `tool_call` / `tool_call_update` / `usage_update`（usage・contextWindow・tokenMeter 揃う時のみ）/ `config_option_update`（`llm/adapters-updated` 時）の6種のみ。PoC が実装すべき範囲が確定。

### A-02 `dsh-excludes-four-variants-intentionally` [改善/high/yes] ★ラウンド1訂正
`dsh-acp/README.md:82,177` に「modes, commands, plans, terminals, client filesystem operations ... unsupported」と明記、全 `@deepseek-ai/*/src` 横断 grep で該当 `sessionUpdate:` 0件。**round-1 E-7「9バリアントを落としている」は要訂正**: `Plan` / `AvailableCommandsUpdate` / `CurrentModeUpdate` / `SessionInfoUpdate` は相互運用上不要。残る実装対象は `ToolCall` / `ToolCallUpdate` / `AgentThoughtChunk` / `UsageUpdate` の4種に絞られる。

### A-03 `dsh-sends-permission-conditionally` [残タスク/high/yes] ★最優先
`dsh-acp/src/index.ts:158-176`, `dsh-user-approval/src/index.ts:68,171,293-310`。`approval/request` イベントで ACP 所有 Agent かつ `callId !== undefined` のとき `conn.request(methods.client.session.requestPermission, ...)`。選択肢は `allow-once` / `reject-once` のみ。承認ポリシー `'ask' | 'never'`（既定 `'ask'`）、**`'never'` では answerer 到達前に即 `'rejected'` で ACP へ送らない**。PoC のハンドラ未実装なら実運用で確実に `method_not_found`（round-1 E-5 は confirmed、ただし条件付き）。

### A-04 `dsh-never-sends-fs-terminal` [改善/high/yes] ★ラウンド1訂正
`dsh-acp/README.md:82,177`、`dsh-acp/src` で `methods.client.fs` / `methods.client.terminal` 0件（`methods.client` 使用は index.ts:126,171 の2箇所のみ）。**round-1 E-6「fs/terminal が method_not_found」は DSH 接続では不要**。SDK に定義があってもブリッジが発行しない。

### A-05 `dsh-ignores-client-capabilities` [改善/high/yes] ★ラウンド1訂正
`dsh-acp/src/index.ts:179-192` の `initialize(_params)` は未使用。自ら `agentCapabilities` を返すのみで `clientCapabilities` を読まない。**round-1 E-4「false ならツールをスキップし得る」は DSH 側では成立しない（推測だったので却下）**。

### A-06 `multimodal-is-provider-driven` [調査必要/high/yes]
`dsh-acp/src/index.ts:182,188`, `content.ts:89-104`。`promptCapabilities: { image: supportsAcpImagePrompts(...), audio: false, embeddedContext: false }`。image は durable attachment store とモデル `inputModalities` 依存。既定プロバイダは `deepseek-official` / `deepseek-v4-flash`。PoC の promptCapabilities は DSH の image 判定に影響しない。

### A-07 `dsh-supports-list-resume-close` [残タスク/high/yes]
`dsh-acp/src/index.ts:189,385-387,241-359`, README:64,67-69。`sessionCapabilities: { close: {}, list: {}, resume: {} }`。list はカーソル付き、resume は cwd 実体検証、`origin: subagent` 除外。**round-1 E-1「session/load/list 未実装」のうち list は正式対応**で、PoC が3メソッドを実装すればマルチセッションが動く。

### A-08 `dsh-supports-set-config-option` [残タスク/high/yes]
`dsh-acp/src/index.ts:333-345,388`, `session.ts:170-210`, README:70。`model` / `reasoning_effort`（ACP v1 標準）。`newSession`/`resume` の `configOptions` とトポロジー変更通知（`config_option_update`）の両方を扱う必要。round-1 E-16 の一部を具体化。

### A-09 `dsh-rejects-session-load` [残タスク/high/yes]
`dsh-acp/src/index.ts:379-391`（登録なし）、README:82,177。履歴再生なし、`session/resume` が代替。**PoC が session/load を送ると method_not_found。送らない設計が正しい**（round-1 E-1 の load 部分は「PoC 側の未実装」ではない）。

### A-10 `dsh-rejects-session-fork` [残タスク/high/yes]
README:76,82,177。メソッド自体は登録なし。README は「list/resume admit ordinary forks, including historical logs with `parentSession`」と区別。**fork 要求を送らず resume で代替可能**（round-1 E-3 の fork 部分も不要）。

### A-11 `dsh-rejects-set-mode` [残タスク/high/yes]
`dsh-acp/src/index.ts:186-190`, README:82,177。`current_mode_update` も送信しない（A-02）。ACP の mode と DSH の承認ポリシーは別概念。round-1 E-16 の set_mode は DSH 接続では不要。

### A-12 `additional-directories-rejected` [改善/high/yes]
`dsh-acp/src/index.ts:509-518`, README:82,174。`session/new` / `resume` で渡されると `invalidParams`。PoC は cwd のみ送信する設計で問題ない。

### A-13 `no-official-roadmap-found` [調査必要/medium/no]
公式 README <https://github.com/deepseek-ai/deepseek-harness/blob/master/README.md>（developer preview / breaking changes のみ、roadmap 節なし）、同梱 docs 全ファイル grep で `roadmap|multimodal|GPUI` 該当なし（"not a roadmap" という否定的文言のみ）。**確認した一次情報の範囲では公式ロードマップ記述なし。不存在の証明ではない**。非公開レポート §5 を「公式一次情報では未確認」と結論付ける根拠。

### A-14 `acp-profile-is-automation-only` [調査必要/high/no]
`${HOME}/.dsh-agentrouter/profiles/acp/package.json`（bundles: `dsh-base` + `dsh-acp-app`）、`dsh-acp-app/cordis.patch.yml`（`acp` を `inject: [acpAppStartup]`）、`dsh --profile acp --help`、`dsh-acp-app/src/index.ts:1-48`。PoC が接続するのは「プレゼンテーション/対話機能を意図的に省いた標準 ACP v1 サーバ」。PoC の期待値をこの制約に合わせる判断材料。

**未確認事項**: 公式 docs サイトへの fetch 失敗（`TypeError: fetch failed`）でサイト内の roadmap 有無は未確認。web_search は HTTP 402 残高不足で失敗。`approval/request` を実発火するツール（推測: sandbox 系）は未追跡。実 JSON-RPC キャプチャは未実施（読み取り専用順守）。

---

## ラウンド2 集計

| 観点 | 検出数 | 主な貢献 |
| --- | --- | --- |
| A: 上流 DSH | 14 | **ラウンド1の4項目を訂正**。permission は条件付きで確実送信、update 6種に確定、fs/terminal/capabilities は不要 |
| B: gpui 0.2.2 ソース | 17 | IME/入力基盤・uniform_list・画像・D&D は既存、wasm32 は失敗、AccessKit 無し |
| C: 既存ソリューション | 24 | 採用3（Channel::duplex / FilterInput / notify-rust）、不採用15（gpui-component は version 非互換） |
| D: 検証戦略 | 18 | Channel::duplex モック E2E、spectacle 証跡、stop_reason 誤用が構造的ブロッカー |
| E: 兄弟プロジェクト | 15 | Apache-2.0 のテーマ/IME/XDG保存/WindowOptions が再利用可。ライセンス境界に注意 |

**合計 88項目**（重複含む）。

### ラウンド2 の最重要結論（後続ラウンドへの引き継ぎ）

1. **ラウンド1の過大評価を訂正する证的発見**: DSH は `fs/*` / `terminal/*` を送らず、`clientCapabilities` も無視し、update は6種のみ（Plan/modes/commands/SessionInfo は意図的除外）。したがって round-1 の E-4/E-6/E-7/E-16 の一部は「DSH 接続では不要」として格下げすべき。**一方 `session/request_permission` は条件付きで確実に飛ぶ（R2A-03）ので E-5 は最優先で残る**。
2. **検証の構造的ブロッカー**: `acp_llm_probe.rs:46-55` が stop_reason を回答本文として agent-loop へ渡しており（D-13）、残高復旧後も本文検証ができない。まずこれを直し、`Channel::duplex()`（C-1/D-14）で残高非依存のモック E2E を組むのが最短経路。
3. **IME は「gpui の欠落」ではなく「PoC の未接続」**: gpui 0.2.2 に `EntityInputHandler` / IME marked range / `examples/input.rs` が存在（B-A2/A3）し、兄弟の Apache-2.0 `FilterInput`（C-21/E-04）が pure logic として移植可能。**gpui-pre 0.3.12 への移行は不要**（gpui-component は不採用: C-4）。

### ラウンド2 で判明した「既知項目の再定義」

- README の「session/update ストリーミング表示」→ 実装済み。残るは Real Data 検証と、DSH が実際に送る4種（ToolCall / ToolCallUpdate / AgentThoughtChunk / UsageUpdate）の表示のみ。
- README の「テキスト入力」→ 低レベル API は存在。残るは IME composition・カーソル・選択の接続のみ。
- 非公開レポート §5「wasm32 可否」→ **不可（errno で exit 101）** と確定。
- 非公開レポート §5「DnD/画像/シンタックスハイライト難度」→ gpui 側の受け取り口/描画 API は存在し、難度は PoC 側の接続作業に移る（ハイライトのみ self-made 必要性が残る）。

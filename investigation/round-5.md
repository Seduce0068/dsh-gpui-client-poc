# ラウンド5: 実実行観察・エッジケース・将来性・自己監査

日時: 2026-11-17T20:57+09:00 頃
起動エージェント: `r5-runtime-observe`, `r5-edge-cases`, `r5-future-proof`, `r5-self-audit`, `r5-concurrency`（5並列）
方針: 最後の発散ラウンド。実実行・異常系・移行戦略・成果物自身の品質・並行処理の5観点から、ラウンド6の統合に必要な最終材料を集める。

## C. `r5-future-proof`（将来性・移行戦略、17項目）

### 前提の陳腐化（最新の Runtime Proof）
- **`deps-cordis-already-v067`** [事実修正/high] `Cargo.toml:10` は **既に `tag = "v0.67.0"`**、git commit `c29db63 chore: track cordis-rs v0.67.0`。round-3/4 の「v0.62.0 ピン・上流 v0.66.0・4タグ遅れ」は **過去の事実**。README:107 の v0.29.0 は3段階古い。todo.md:51 の更新検討は完了扱いにできる。**並行チームが調査中に依存を更新した** 점에 주의（最終統合時に Cargo.toml の現状を再確認必須）。

### 依存の鮮度・順序
- **`gpui-stable-frozen-vs-pre-weekly`** [high/no] gpui 0.2.2 は 2025-10-22 公開以降更新なし。gpui-pre は 0.3.13 / 2026-11-16、2.5か月で 0.3.0→0.3.13（週次）。案C継続コストの定量根拠。判断: 保留（ユーザー）。
- **`gpui-upstream-release-issue`** [medium/no] [zed#66275](https://github.com/zed-industries/zed/issues/66275) タイトル「gpui: publish a new crates.io release (0.2.2 predates PinchEvent)」。状態・本文は取得失敗で未確認。
- **`acp-major-5x-gap`** [high/partial] 最新 **5.3.0**（2026-11-14）、MSRV 1.88、trustpub_only（署名付きパブリッシュ専用）。DSH 側がどの ACP 版を話すかは未確認（`agent_info=0.0.1` のみ）。判断: 要相談。
- **`acp-feature-gate-renamed`** [high/no] 0.11.x の `unstable_session_close` / `unstable_session_model` は 5.3.0 の feature 一覧に存在しない（5.3.0 は `unstable` / `unstable_session_status` / `unstable_session_set_title` / `unstable_plan_operations` / `unstable_session_archive` 等）。**session-close を先に実装すると ACP 更新時に作り直し**。順序制約。
- **`tokio-latest-unconfirmed`** [high/yes] curl 403 / web_fetch 失敗で最新版未確認。ピンは host 側間接依存との衝突回避で単独更新は崩れるリスク。判断: 保留。

### 案C継続 vs 案A/B
- **`plan-c-vs-plan-b-recommendation`** [medium/partial] ★戦略推奨。エージェントは **案B寄りハイブリッド**（UI層を gpui-pre + gpui-component へ移行、プロトコル層 ACP/cordis は案C成果を維持）を推奨。根拠: 案Cは短期的に安いが長期的に高い（UI17ファイルが gpui 型に直結、gpui 0.3 出た瞬間に全面書き換え再発、兄弟との共通化ボトルネック未解消）。案Aは ACP/cordis の Runtime Proof を捨てる。判断の前提条件4つ（DSH の ACP 継続、兄弟移行の実利益、残高回復、週次追従リソース）は未確認。**要相談（ユーザー最終承認）**。

### API 境界（型の浸透）
- **`acp-schema-types-leak`** [high/no] 6ファイルが `agent_client_protocol::schema::{...}` を直接 import。自前 `renderer::ChatEvent` + `acp_adapter` 1枚に変換を閉じ込めれば ACP 5.x 更新の影響を6ファイル→1モジュールに縮小。推奨。
- **`gpui-ui-state-types-leak`** [high/no] 全17 UI ファイルが `Render`/`Window`/`SharedString`/`div()` に直結（grep 106件）。状態フィールド自体が `SharedString`。Domain は `String`/自前 struct、View 境界でのみ変換する方針を今から。推奨。
- **`cordis-context-and-loader-leak`** [high/no] `Context`/`PluginFactory`/`plugin_ports` が11バイナリに浸透。`_config` 破棄バグが14箇所の修正になった原因。1つの `composition` モジュールへ集約。推奨。
- **`two-context-alias-collision`** [high/no] `gpui::Context` と `cordis_rs::Context` の同名衝突を alias で回避（todo_ui.rs:12-13 等）。恒久解はアダプタ境界で cordis Context を外部に出さないこと。推奨。

### 機能の陳腐化
- **`markdown-upstreamable`** [medium/yes] gpui-component 0.10.0 に `syntect` / `tree-sitter-markdown` / `tree-sitter-md` / `tree-sitter-markdown-inline` feature。自作して案B移行時に捨てるコストを回避。要相談（移行判断と連動）。
- **`ime-upstream-flag-unknown`** [low/no] IME 確定 Enter 判定は上流が提供するか未確認。上流待ちにせず PoC 側で分離実装が必要。保留（自前継続）。
- **`persistence-and-theme-stay-local`** [medium/partial] ACP 5.3.0 の `unstable_session_archive` はプロトコル側アーカイブで、ローカル GUI の保存先ポリシー（XDG + 原子的保存）とは別。theme もアプリ固有。**所有権は自前、描画は上流委譲**の分離が正しい。推奨。

### ユーザーへの相談事項（エージェント判断不可）
1. 案C継続 vs 案B移行（推奨は案B寄りハイブリッド、前提4条件の確認）
2. `dsh` 本体のバージョン／ACP 対応方針／公式ロードマップ（未確認）
3. モデル残高の補填 or `Channel::duplex()` モック検証の選択（課金・外部送信を伴う）
4. 配布時の Apache-2.0 NOTICE 表示方針（MIT 自体は判断済み）
5. gpui 0.3 が出た場合の再判断タイミング（issue #66275 監視）

**移行ロードマップ**: cordis(済 v0.67) → ACP(型境界を整えてから 5.3.0) → tokio(最後に lock) → gpui-pre(最大の書き換えを独立ブランチ) → 検証。**gpui だけを先に移すと ACP 5.x 変更時に UI と Adapter の両方を触るため逆転しない**。

**未確認**: tokio 最新値、gpui-component 0.10.0 が直接依存する gpui-pre 版、issue #66275 の状態、IME 状態 API、DSH 公式ロードマップ、ACP 5.x と DSH の互換性。

---

## E. `r5-concurrency`（並行・リソース・ライフサイクル深掘り、9項目 + 陰性確認3 + シナリオ7）

### 19バイナリの並行構成（Runtime Proof）
`#[tokio::main]` 9 / `new_current_thread()` 9 / `std::thread::spawn` 1 / `cx.spawn` 1 / `app.spawn` 1 / runtime 無し 1 = 19。thread join 漏れ1件（acp_stream_ui）。GPUI UI 8つは「current-thread runtime で初期化→その後 GPUI executor」の二段構成。

### E-1 `detached-poll-keeps-strong-entity-alive` [改善/high/no] ★
`acp_stream_ui.rs:154-170` + gpui 0.2.2 `app/entity_map.rs:374-382,307`。`let poll_target = panel.clone()` が **strong Entity** を無限 loop 内に閉じ込め `.detach()` がハンドルを破棄。Entity は App の EntityMap に所属し Window に紐づかないため Window 閉鎖後も `app.update → cx.notify` が届く。**round-1 A-7 の真の保持機構は「strong Entity + detached task + 終了条件なし」の3点セット**。WeakEntity + 完了通知 + CancellationToken が必要。

### E-2 `mutex-guard-held-across-block_on` [改善/high/no]
`cordis_entries_toggle_ui.rs:83-102`, `cordis_composition_toggle_ui.rs:28-48`。MutexGuard 保持中に `futures::executor::block_on(fiber.unload())` / `Fiber::mount`。再入で即デッドロック（現状 factory は state に触れないので未発火だが UI 同期ブロックは常時成立）。guard を block 前に drop すれば解消。

### E-3 `approval-read-guard-sync-callback-reentrancy` [残タスク/high/no] ★新規
cordis-rs `approval.rs:131-149`（**v0.67.0 も同一**）。`ApprovalService::request()` が `self.state.read()` の read guard を保持したまま `answerer(req)` を同期呼び出し。**round-4 A-5 で提案した「Answerer を mpsc::recv() で待ち UI が回答」回避策では、UI が `add_answerer()` / `set_policy()`（write）を呼んだ瞬間に RwLock 再入でデッドロック**。つまり「回避策自体がデッドロックする」。callback 前に guard を drop する最小修正＋async 化が必要。

### E-4 `no-poison-recovery-anywhere` [改善/medium/no]
アプリ側 `Mutex::lock()` 17箇所＋cordis 側全ロックが `expect`/`unwrap`。poison 回復 `unwrap_or_else(|e| e.into_inner())` は0件。round-3 A-9 の連鎖 panic が残る。

### E-5 `no-application-level-channel-backpressure` [調査必要/medium/no]
`mpsc` / `::channel` / `send()` / `recv()` は `src/**/*.rs` で0件。フロー制御を ACP crate の `block_task()` と transport の bounded channel に丸投げ。chunk バースト時のメモリ増を観測する手段が無い。bounded mpsc + ドロップ方針の明示が必要。

### E-6 `child-lifecycle-stages-not-RAII` [改善/high/no]
段階別残存リソースを特定: spawn 後 child+pipe（6バイナリ `kill_on_drop` 未指定）→ stdin/stdout.take() 後に pipe が transport へ移動 → initialize/prompt 中（`block_task().await` 6箇所、timeout なし）child+task+thread → 成功時のみ kill。早期リターンで kill を飛ばす。round-4 訂正どおり transport が先に drop されれば EPIPE で終了し得るため「pipe を持ち続ける」は条件付き。**acp_stream_ui は JoinHandle 破棄で停止手段が無い**。

### E-7 `fiber-arc-unload-ignores-strong-count` [調査必要/medium/no]
cordis-rs `plugin.rs:78-104,120-164`（v0.67 同一）。`Fiber(Arc<FiberInner>)` は strong handle だが `unload(&self)` は `Arc::strong_count` を検査せず任意クローンから dispose。`Context::register_fiber` にも登録されるため unload 後も Context 側登録が残り remount 時に新旧 Fiber が混在し得る（register_fiber 内部は未追跡）。

### E-8 `per-click-current-thread-runtime-recreation` [改善/low/no]
`cordis_permission_ui.rs:32-79,129-133`。クリック毎に current-thread runtime を build→block_on→drop。round-1 A-17 と近接。

### E-9 `cordis-version-triple-mismatch` [改善/medium/yes]
`Cargo.toml:10` v0.67.0 / `Cargo.lock` `#84425d1` / `README.md:107` v0.29.0 / `investigation/README.md:20` v0.62.0 の4箇所不一致。ただし監査した v0.62.0 と現状 v0.67.0 で `agent_loop.rs:232-234`, `approval.rs`, `plugin.rs` は同一で結論は変わらない。

### 陰性確認（完全性の根拠）
ロック保持中の `.await` / `cx.notify()` はアプリ側0件。多重 runtime 同時生成0件。`std::thread::spawn` は1件のみ。

**デッドロック/リーク7シナリオ**: S1 poison→描画/通知 panic→kill スキップ→孤立、S2 approval read guard→UI write 再入、S3 toggle guard 保持 block_on→再入、S4 Window 閉鎖後 detached loop が strong Entity 保持、S5 connect_with ハング→thread+runtime+child+pipe 残存・JoinHandle 破棄で停止不能、S6 unload 後 Context 登録残り remount 混在、S7 permission 連続クリックで runtime 生成繰り返し。

---

## D. `r5-self-audit`（成果物の自己監査、13項目）

### AUDIT-01 `round-2-count-off-by-one` [中/high]
round-1 115（20+43+3+15+18+16）✓、round-3 70（9+8+19+25+9）✓。**round-2 は B セクション実見出し16個なのに集計表は「17」→ 実合計87（88 は1件過大）**。cross-review「約120件」は見出し合計114でおおむね妥当だが根拠曖昧。

### AUDIT-02 `id-collision-across-rounds` [中/high]
`D-9` が round-1=`session-close-never-called`, round-2=`rv-spectacle-screenshot-works`, round-3=`build-registry-shared-mod` の3回使用。grep 追跡が壊れる。統合時にラウンド接頭辞を付与すべき。

### AUDIT-03 `tmp-representative-id-wrong` [中/high]
親が「A-16 と F-9 と D-9 は同一の /tmp 問題」と提示したが **誤り**。D-9 は session-close で /tmp と無関係。/tmp 系統は A-16 + F-9 + round-2 E-06 + final G4 の2〜4件。

### AUDIT-04 `substantive-count-estimate` [高/medium]
検出総数 272件（115+87+70）→ 反証後 約200件 → 代表 id 統合後 約150〜165件 → **todo G1〜G8 の実質アクション約35〜43件**。**対外報告は「272件検出 → 最終約40項目」の二段表記が正確**。

### AUDIT-05 `ux-43-integration-gap` [高/high] ★
UX43件のうち **B-29 deep-link / B-31 background-startup / B-5 session-folders は完全消滅**。B-9 virtualization は「API あり」と解消のみでタスク化されず、B-26 system-tray は言及されるが todo に実装タスク無し。final.md:125 の「43件を登録した」は誤りで、**実際は主要7件のみ、残り約30件は `ux-pool-triage` 未実施**。B-5 は round-3 の別 id B-5(`four-main-gates-pass`) と衝突。

### AUDIT-06 `c1-not-reflected-in-todo` [高/high] ★最重要
round-4 C1 の訂正（permission は条件付き、fs/terminal は refuted）が **cross-review final.md:46 と todo.md:24 に未反映**。`ReadTextFileRequest`/`terminal/*` の handler 登録は不要タスク。具体修正文案を提示（severity high→medium、fs/terminal は対象外、汎用 ACP 対応は別タスク）。

### AUDIT-07 `c2-not-reflected-in-todo` [高/high] ★最重要
round-4 C2 の訂正（6種のうち5種）が **final.md:32 と todo.md:27 に未反映**。「11種のうち10種、Plan を描画」のまま。具体修正文案を提示（DSH 実送信6種の未描画5種のみ、Plan 等は対応不要、severity medium 検討）。

### AUDIT-08 `vuln-triage-not-reflected` [中/high]
round-4 の「到達15件、最優先 rmcp 6.5、tokio 0386 は AsyncFd 未使用で現状発現せず」が todo に無い。G3 に `deps-vulnerability-triage` 追加を推奨。

### AUDIT-09 `final-unknown-section-wrong` [中/high]
cross-review final.md:158-159 が「cargo audit 未実行」と誤記。実際は round-3 B-1 で EXIT=1/26件実測済み、ACP 最新 5.3.0 も round-3 C-17 で確認済み。**todo.md:65 `external-research-retry` が無駄な再調査を誘発**。

### AUDIT-10 `a5-a6-a8-reflected` [低/high]
A-6 → `stream-fixed-prompt-no-input`、A-8 → `errors-swallowed` は反映済み。A-5 は共通化に吸収。未反映なし。

### AUDIT-11 `source-quality-mostly-fine` [低/high]
失敗出典の明示は概ね適切（round-2 A/C, round-3 C, round-4 E-4 等）。問題は AUDIT-09 のみ。

### AUDIT-12 `okf-h1-violation` [中/high]
**investigation/round-1〜4 の4ファイルすべて OKF frontmatter 無し・本文先頭 H1**（AGENTS.md §8 違反）。cross-review 6ファイルと todo.md は準拠 ✓。**最終 `investigation/final.md` が同じ違反を踏襲する前に修正必須**。

### AUDIT-13 `crossreview-future-generated` [低/high]
cross-review の `generated` が実 mtime より未来（round-2 +5分, round-3 +10分, round-4 +15分, round-5 +8分, final +14分）。round-1 は逆に −8分。r4 指摘後も修正されず。

**統合前に直すべき上位3**: ① AUDIT-06+07（C1/C2 の todo 反映漏れ）② AUDIT-05（UX43件の「登録済み」矛盾と宝の消滅）③ AUDIT-12（investigation の OKF/H1 違反を final で踏襲しない）。

---

## A. `r5-runtime-observe`（実実行による挙動観察、7項目・Runtime Proof）

### OBS-1 `acp-probe-stderr-only-and-noise` [改善/high/partial]
`timeout 20 ./target/debug/acp_probe` → exit 0、stdout **空**、stderr に `INIT OK` / `SESSION OK` + dsh 警告5件（preset 待機）+ `[llm-deepseek-api-key] model listing ... timed out after 5000 ms; using the bundled catalog`。**外部 dsh が無くてもモデル一覧5秒タイムアウトしても initialize/session/new は成功し bundled catalog にフォールバック**。`ps` で残存プロセスなし。残タスク: ①成功結果が stderr のみで stdout 空（パイプ処理不可）②警告と成功が混在し「成功したか分かりにくい」③5秒体感遅延④README の「残高不足で回答未確認」は本 probe では再現せず（prompt を送らないため）到達範囲の記述が乖離。

### OBS-2 `bridge-registered-commands-one` [改善/low/yes]
`timeout 20 ./target/debug/cordis_bridge` → exit 0、stderr 空、stdout は `plugin command-compact state = Active` / `Registered commands (1): - /compact`。期待通りだが登録コマンド1件のみで説得力が弱い（round-3 F-2 と整合）。

### OBS-3 `wayland-gui-self-verified-with-spectacle` [改善/high/partial] ★最重要
`timeout 8 ./target/debug/cordis_todo_ui`（DISPLAY=:1/WAYLAND_DISPLAY=wayland-0）→ exit 124（timeout SIGTERM）、stderr に `mounted tool-todo, tools=["todo_write"]` / `GPUI window created`。kill 後 `ps` 残存なし（きれいに終了）。`xwininfo -root -tree | grep -iE 'cordis|todo|GPUI'` → 該当なし（Wayland ネイティブ、README:69 と一致）。
**しかし起動中に `timeout 15 spectacle -b -n -o /tmp/r5-todo-visible.png` を撮り `read_image` でエージェント自身が画像を読んだところ、Wayland ネイティブウィンドウが実際に写っていた**: タイトル `cordis-rs x GPUI — tool-todo`、本文 `No todos yet. Use the buttons below to call todo_write.`、ボタン `+ prepare step / start / finish / clear`、下部ステータス `Ready`。**README:71-84 の「kdotool が無く Wayland ネイティブのウィンドウは自動列挙できず、目視はユーザー確認が必要」はもはや正確ではない**。spectacle + 画像読み取りでエージェント自身が GUI 目視検証が可能。PNG は確認後削除。

### OBS-4 `spectacle-screenshot-reproduced` [改善/medium/no]
`timeout 20 spectacle -b -n -o /tmp/r5-observe.png` → exit 0、全画面（環境依存）の PNG（8-bit/color RGBA、寸法・サイズ・ハッシュは環境依存）、削除済み・残存なし（round-2 D-9 再現）。**注意**: 全画面にはユーザーのブラウザ等も写るため、証跡共有時はマスキング/クロップ方針が必要（他画面内容は報告に含めず）。

### OBS-5 `smoke-script-not-executed` [調査必要/medium/yes]
`scripts/smoke-gui.sh` は静的読解のみ（指示により実実行中断）。12バイナリを `timeout 20` + `grep -q 'GPUI window created'` + `|| true` で成功判定、`ok: <bin>` を出力。mktemp ログを毎回削除し証跡が残らない。**「ok だが何も検証していない」典型**で描画内容・ボタン動作は未検証。実行結果は未確認なので断言しない。

### OBS-6 `tmp-path-collision-unproven` [調査必要/medium/partial]
固定パスは静的に特定: `cordis_composition_ui.rs:203` の `remove_dir_all("/tmp/cordis-gpui")`、同 :56,:76、`cordis_entries_toggle_ui.rs:253,:160,:237`、`cordis_agent_ui.rs:46,52`。2つ同時起動の衝突は構造的に明白だが**実証は未実施**。

### OBS-7 `no-orphan-process-after-timeout` [問題なし/low/no]
全実行後 `ps` で cordis_todo_ui / acp_probe / dsh --profile acp / spectacle の残存なし。（実行前からある `dsh --profile agentrouter` PID 3146590 は無関係で同一のまま。）

**実行できなかったこと**: smoke-gui.sh 実実行（最大300秒、指示で中断）、2つ同時起動の実証（同）、kdotool 自動列挙（NOT FOUND、spectacle で代替実証）、他11 GUI デモの個別起動（時間制約）。

---

## B. `r5-edge-cases`（エッジケースの棚卸し、28項目）

### 空/極端な入力
- **E5B-01 `empty-prompt-silently-dropped`** [改善/high/no] `cordis_prompt_ui.rs:63-66`。`draft.trim().is_empty()` で空 Enter を黙って return、理由表示なし。
- **E5B-02 `grapheme-backspace-corruption`** [バグリスク/high/no] `:59-61,150-153`。`String::pop()` と `key_char.chars()` は Unicode スカラ単位で、結合文字/ZWJ 絵文字で Backspace が構成要素を分割。IME 確定文字列も分割保持。
- **E5B-03 `unbounded-prompt-length`** [バグリスク/medium/no] prompt/draft に上限無し → JSON-RPC 1行巨大化＋`Vec<SharedString>` 無制限增长。round-3 の巨大行指摘を「入力側から到達可能」に接続。
- **E5B-04 `invalid-utf8-arg-panics-before-gui`** [調査必要/high/no] `std::env::args()` は不正 UTF-8 で panic、NUL は execve レベルで混入不可。**OS/標準ライブラリに丸投げで GUI 起動前にプロセスが落ちる**。改行混じり JSON は `serde_json::json!` が正しくエスケープ（確認済み）。
- **E5B-05 `fs-read-errors-abort-before-window`** [残タスク/high/no] ★ `cordis_fs_ui.rs:64-81`。存在しないパス/ディレクトリ/権限なし/不正 UTF-8 で `?` が early return、**GPUI ウィンドウが開かずエラーが一切可視化されない**。再現容易・修正は render 側 Err 表示に集約可。

### 順序・競合
- **E5B-06 `chunk-order-no-sequence`** [調査必要/low-medium/no] sequence 番号も session_id フィルタも無く到着順そのまま。現状は単一 ByteStreams で順序保存されるが Channel::duplex 化/複数 session で顕在化（推論）。
- **E5B-07 `status-before-final-chunks`** [改善/medium/no] `acp_stream_ui.rs:123-140`。PromptResponse 直後に status を `turn finished` へ上書きするが通知ハンドラはその後も chunk 追記。「完了表示なのに本文が伸びる」同期バリア無し。
- **E5B-08 `polling-loop-survives-window-close`** [バグリスク/high/no] ★ `:160-170,141`。Window 閉じても `app.update()` の Err が `let _ =` で握り潰され 100ms ループ永続。ACP スレッドも turn 完了まで動き dsh ハング時は子がデタッチ残存。**smoke では検知できない実リーク**。
- **E5B-09 `late-reply-dropped-on-closed-window`** [バグリスク/medium/no] `cordis_prompt_agent_ui.rs:183-190,78-84`。送信中に閉じると `panel.update` が Err で応答破棄、キャンセル無し。返信対応が「最後の要素」依存で future 完了順が変わると別 prompt を上書き（現状 busy で直列化し回避）。
- **E5B-10 `unload-error-ignored-in-toggle`** [改善/high/no] unload 失敗を無視して `fiber.take()` 済みで即「unloaded」表示、remount は別 Fiber で状態復元なし。

### 状態の境界
- **E5B-11 `todo-write-single-item-only`** [残タスク/medium-high/no] 3ボタン全てが1項目 `"Prepare workspace"` のみ。0件/複数/1000件が到達不能。`unwrap_or_default()` は projection 未登録と 0件を同一視。
- **E5B-12 `todo-render-unbounded`** [改善/high(コード)/medium(実害)/no] 1000件で1フレーム1000超要素、スクロール/仮想化なし。0件はplaceholder対応済み。
- **E5B-13 `turn-start-clears-todos`** [バグリスク/low-medium/no] `clear()` が `SessionEvent::TurnStart` を投げ、実クライアントで turn 開始のたびにユーザー todo が消え得る。
- **E5B-14 `empty-title-renders-blank`** [改善/high/no] 空タイトルで `s-llm [llm]: ` と空欄、長タイトル切り詰め無し、provider エラーで行0件のウィンドウが開きエラー消える。
- **E5B-15 `retry-exhaustion-unreachable`** [残タスク/high/no] provider が「1回目失敗→2回目成功」固定で retry 使い切り状態が到達不能、エラー表示コードが実行されない。
- **E5B-16 `permission-mode-model-mismatch`** [残タスク/high/yes] 「一度許可/毎回許可/拒否」の状態遷移が cordis UI に存在せず、クリック毎に Context 再生成で承認メモリも残らない（round-1 A-2/D-15/F-6 重複だが状態観点で補強）。
- **E5B-17 `session-list-hardcoded`** [残タスク/high/yes] 3件固定、0件/10000件境界も id/状態/永続化も無し（既知 B-2 の境界観点）。

### ファイル系
- **E5B-18 `fs-root-none-allows-symlink-escape`** [調査必要/medium/no] `ToolFsConfig::default()` の root=None なら `resolve()` が canonicalize/チェックせず、symlink で `/etc/*` も読める（推論込み、実値要確認）。
- **E5B-19 `corrupt-state-json-silent-reset`** [改善/high/no] ★ `/tmp/cordis-entries-toggle.json` 破損で `load()` が `Self::default()` を返し、保存済み「無効化」選択が全件消え YAML 既定に戻る。通知なし。
- **E5B-20 `state-save-error-invisible`** [改善/high/no] /tmp 満杯・権限エラーで save 失敗しても UI は成功表示、再起動で状態 revert。
- **E5B-21 `tmp-global-path-collision`** [改善/high(衝突)/low(変更検知)/partial] 固定グローバルパスで並列起動衝突。`agent_ui` は write→read をロック/バージョン確認なしで行い読取中変更を「成功」として描画。

### 外部依存の故障
- **E5B-22 `dsh-immediate-exit-no-recovery`** [改善/high/no] dsh exit 1 で stdout EOF → `connect_with().await?` が Err、再起動/リトライ無し、エラーに stderr 含まれず。
- **E5B-23 `hang-has-no-bounded-wait`** [残タスク/high/no] 全 `block_task().await` に timeout 無し。stream_ui は status「connecting…」のまま固まる。
- **E5B-24 `stderr-inherited-not-a-deadlock`** [調査必要/high/no]（反証）stderr は親端末継承で GPUI に映らないが、piped ではないため「stderr flood でハング」仮説は反証。
- **E5B-25 `jsonrpc-id-collision-unverified`** [調査必要/low/no] id 採番は依存クレートに委譲、静的読取りでは断定不可。`Channel::duplex()` モックで意図的衝突注入実験が必要。
- **E5B-26 `session-id-opaque-and-cwd-fallback`** [改善/high/no] session_id を検証なしで渡し、空/不正形式でもローカルはエラー出さず dsh 応答待ち。cwd 削除済みだと `/` に黙ってフォールバック。

### 設定の組み合わせ
- **E5B-27 `window-options-default-and-app-id-collision`** [残タスク/high(コード)/medium(WM)/yes] 13バイナリ全部 `open_window(Default::default())`。全デモが同じ既定 app_id で複数同時起動時に WM グループ化/activation 衝突し得る（app_id 重複は新規観点）。
- **E5B-28 `ime-compose-window-close`** [バグリスク/medium/no] `observe_keystrokes().detach()` にクローズ時解除無し。IME 変換中 Enter で送信（既知）、composition 中に閉じると古い Entity への update が Err で入力/応答破棄。Wayland 実機確認必要。

**実験が必要な項目**: E5B-06（chunk 順序）, E5B-25（id 衝突）, E5B-18（ToolFsConfig root 実値と symlink read）, E5B-04（不正 UTF-8 argv の終了コード）, E5B-02/E5B-28（グラフェーム Backspace と IME close）, E5B-12/E5B-13（todo 1000件性能と TurnStart 発火順序）, E5B-23（dsh ハング timeout）, E5B-21（読取中変更の競合）。

---

## ラウンド5 集計

| 観点 | 検出数 | 主な貢献 |
| --- | --- | --- |
| A: 実実行観察 | 7 | **spectacle + read_image で Wayland GUI をエージェント自身が目視検証（README の記述が obsolete）**、acp_probe の stderr 出力設計 |
| B: エッジケース | 28 | fs エラーで Window 不开、detached poll リーク、状態 JSON 破損サイレントリセット、グラフェーム Backspace |
| C: 将来性 | 17 | cordis v0.67 更新済み、案B寄りハイブリッド推奨、3層の型浸透、要相談5件 |
| D: 自己監査 | 13 | 272件→約40件、C1/C2 未反映、UX43件の約30件消滅、OKF/H1 違反 |
| E: 並行処理 | 9+陰性3+シナリオ7 | approval read guard 再入デッドロック、strong Entity 保持、段階別リソース残存 |

**合計 79項目**（7+28+17+13+9、シナリオ/陰性確認は別計上）。

### ラウンド5 の最重要結論
1. **GUI 目視検証は可能になった**（OBS-3）。README:71-84 を更新すれば「ユーザー目視待ち」という開発サイクルの停滞要因が消える。これは全 GUI 機能の検証速度に効く。
2. **並行セッションが cordis-rs を v0.67.0 へ更新済み**（round-5 C）。investigation/README.md の v0.62.0 記載も含め、全ドキュメントのバージョンを同期する必要がある。
3. **成果物の統合漏れが発覚**（round-5 D）。round-4 の訂正（C1/C2）が cross-review final/todo に未反映で、不要な handler と Plan 描画を実装するリスク。UX43件の約30件が消滅。**最終ラウンド6で最優先で反映する**。
4. **承認回避策自体がデッドロックする**（round-5 E-3）。round-4 で提案した mpsc 回避策は `ApprovalService::request` の read guard 保持で再入デッドロック。guard drop が最小修正。
5. **検証可能性と実害のギャップ**（E5B）。再現容易なバグ（fs エラーで Window 不开、状態 JSON 破損サイレントリセット）が	static 解析だけでは見えなかった。逆に「stderr flood でハング」は反証され、 stuck 系は未実験として明示が必要。

### 全ラウンドの検出数（最終集計・重複含む）
| ラウンド | 検出数 | 累計 |
| --- | --- | --- |
| 1 | 115 | 115 |
| 2 | 87（集計表の88は1件過大、自己監査 AUDIT-01 で訂正） | 202 |
| 3 | 70 | 272 |
| 4 | 反証（訂正11件・新規7件） | — |
| 5 | 79 | 351（重複含む） |

**自己監査 AUDIT-04 の見積もり**: 代表 id へ統合後 **約150〜165件**、todo G1〜G8 の実質アクション **約35〜43件**。「351件検出（重複含む）→ 最終 約40項目」の二段表記が正確。

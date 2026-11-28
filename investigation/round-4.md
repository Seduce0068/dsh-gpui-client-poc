# ラウンド4: 敵対的妥当性検証（主張の反証）

日時: 2026-11-17T20:37+09:00 頃
起動エージェント: `r4-verify-acp`, `r4-verify-security`, `r4-verify-gates`, `r4-verify-crossreview`, `r4-verify-solutions`（5並列）
方針: 新規発見ではなく、ラウンド1〜3の高インパクト主張を **反証する** ことで妥当性を高める。判定は confirmed / overstated / refuted / unverifiable の4値。

## A. `r4-verify-acp`（ACP 主張の反証、6主張）

| 主張 | 判定 | 根拠 | 結論 |
| --- | --- | --- | --- |
| E-5/R2A-03 permission → method_not_found | **overstated** | acp `role/acp.rs:20-29`, `incoming_actor.rs:344-352` は事実。DSH `index.ts:158-176` は owned session + callId + policy='ask' の3条件でのみ送信、`never` なら即 rejected | 「確実に method_not_found」は言い切れない。**条件付き**に格下げ。R2A-03 の訂正自体は confirmed |
| E-7/A-02 9バリアントドロップ | **overstated** | dsh-acp `updates.ts:26,36,54,80,98` + `session.ts:197` の6種のみ。README:82,177 で plans/modes/commands unsupported | 「9バリアント」は誤り。**6種のうち5種の表示未実装**に書き換え（A-02 訂正 confirmed） |
| E-6/A-04 fs/* terminal/* method_not_found | **refuted** | `methods.client` 使用は `index.ts:126,171` の2件のみ、fs/terminal 0件 | DSH 接続では発生しない。「他 Agent 接続時の一般課題」へ格下げ（A-04 訂正 confirmed） |
| E-4/A-05 ClientCapabilities false でスキップ | **refuted** | `index.ts:179-193` の `initialize(_params)` が `_params` を一切参照せず、返すのみ | 推測を却下。A-05 confirmed |
| D-13/RV-13 stop_reason を本文として渡す | **confirmed** | `acp_llm_probe.rs:46-53` が `format!("{:?}", stop_reason)` のみ、購読ハンドラ無し、`:67-76` で provider 戻り値に。schema 0.12.0 `agent.rs:3068-3081` は PromptResponse に本文フィールド無し（本文は session/update 経由） | 現行コードに本文取得経路が構造的に存在しない |
| E-1/A-07,A-09 session/load, fork | **confirmed（load/fork に限る）** | `index.ts:379-390` の登録に load/fork 無し、`sessionCapabilities` は close/list/resume のみ。README:82,177 | PoC が送らない設計は正しい。**ただし session/list は DSH 対応済みで PoC 側実装残タスク**（E-1 を分割修正） |

**集計**: 反証2（E-6, E-4）/ 誇張2（E-5, E-7）/ 確定2（D-13, load・fork）/ 一部修正1（E-1 list 部分）。round-2 の訂正（A-02/A-04/A-05）はすべて confirmed。

---

## B. `r4-verify-security`（セキュリティ主張の反証、5主張）

| 主張 | 判定 | 根拠・反証の結果 |
| --- | --- | --- |
| A-1 子プロセスの全環境継承 | **confirmed**（下位記述1つ refuted） | 6箇所すべて `tokio::process::Command::new("dsh")`、`env_clear()` 0件、`std::process::Command` 0件。**「stream_ui はスレッド環境も継承」は refuted**（Unix にスレッド単位環境は無く、全6箇所がプロセス環境継承）。実 API キー存在は未検証（秘密情報を読まないため） |
| A-2 異常終了時の孤立 | **confirmed** | tokio 1.48.0 `process/mod.rs:1076-1091`（kill_on_drop 既定 false 明記）、`:1123-1127`（ChildDropGuard は true の時だけ kill）。早期リターン多数。**「pipe を持ち続ける」は条件付きでやや誇張**（reader が閉じれば EPIPE で終了し得る）。std 版の可能性は削除 |
| A-5 同期 Answerer で対話承認が不可能 | **overstated**（前提 confirmed・結論 overstated） | `approval.rs:49` 同期、`ApprovalRequest{action,description}` に args 無し、`tools.rs:311-315` の description は固定文字列。回避策あり: ①予め答えを決める Answerer（AtomicBool）、②tool 実行を別スレッド/runtime へオフロードし Answerer を `mpsc::recv()` 同期ブロック＋UI 側が送信。**「async 化しないと不可能」は誤り**。args 欠如は confirmed で独立問題 |
| A-6 bash RCE チェーン | **overstated**（一部 refuted） | `sh -c` confirmed、default config confirmed、scripted provider 抑制 confirmed。**因果が誤り**: cordis-rs v0.62.0 の `AgentLoop::run` は `self.tools.invoke(...)` を直接呼び（`agent_loop.rs:234`）、`ToolPipeline::run` を通らない。pipeline を注入する API も無い。`let (tools, _)` は症状であって原因ではない。**「stderr 未捕捉で外部送信」は refuted**（`shell.rs:136` で piped、`tool_bash.rs:84-89,115` で rendered に含まれ UI 表示） |
| A-8 session cwd が子 Command に適用されない | **overstated** | `grep -rn '\.current_dir' src/` = 0件 confirmed。ただし `NewSessionRequest.cwd` は ACP 経由でエージェント側へ渡す「セッション作業ディレクトリ」で親 `Command::current_dir()` とは別概念。dsh が尊重するかは未検証。**被害の結論は自動的に導かれない** |

**集計**: confirmed 2（A-1, A-2）/ overstated 3（A-5, A-6, A-8）/ refuted 主張レベル 0（下位記述3つは refuted）/ unverifiable 2（実キー存在、dsh 側 cwd 解釈）。

**ラウンド3→4 の訂正ポイント**: A-6 の真正な根本原因は「AgentLoop が ToolPipeline を通らない設計」で、修正は「AgentLoop に pipeline を注入する API 追加＋host 側で `with_approval`」。A-5 の修正は「ワーカースレッド＋チャネル、または args を description に埋め込む」。

---

## C. `r4-verify-gates`（検証主張の反証、8主張・Runtime Proof 付き）

| 主張 | 判定 | 要点 |
| --- | --- | --- |
| B-1/C-6 数値 | **confirmed** | `cargo audit` EXIT=1、26 vulns / 23 warnings。advisory DB 更新後(11/17 20:28)も同一 |
| B-1/C-6 実害範囲 | **overstated** | **26件中11件は非到達**（quinn-proto×8, quinn-udp×1, quick-xml 0.30.0×2; `cargo tree -i` → `nothing to print`）。最大 Severity 8.7 の RUSTSEC-2026-0037 も非到達。「TLS/QUIC が中心」はミスリーディング。実害候補15件、最優先は到達する rmcp `RUSTSEC-2026-0462`（Severity 6.5, Unbounded response body buffering, agent-client-protocol 直下） |
| C-7 advisory 事実 | **confirmed** | RUSTSEC-2026-0386 実在、`patched=[">=1.51.6,<1.52.0",">=1.53.3"]`、`=` 1.48.0 で修正不能 |
| C-7 因果 | **overstated** | advisory の影響は `tokio::io::unix::AsyncFd` の4コンストラクタのみ、**`grep -rn 'AsyncFd\|asyncfd' src/` = 0件**。PoC の制御フローで発現経路なし。さらに cargo audit では tokio 0386 は `warning: unsound`（error 26件に含まれない）for CI audit ゲートでも tokio 単体では赤にならない |
| B-2 数値 | **confirmed** | `cargo test --doc --locked` EXIT=101、`no library targets found`、src/lib.rs 不在 |
| B-2/D-18 絶対性 | **overstated** | `cargo test --locked` / `--all-targets --locked` は EXIT=0（8 passed）。`--doc` を外せば回避可能 |
| D-19 数値 | **confirmed** | `cargo machete 0.10.1` EXIT=0、未使用依存0件 |
| D-19 言い切り | **overstated** | machete 自身 "fast yet imprecise"、regex ベースで false positive/negative あり得る。「削除タスク不要と確定」は「machete の検出範囲では0件」に緩和。過剰 feature（例: tokio features=["full"]）は別観点で未検証 |
| D-11/D-12 | **confirmed** | open_window=13バイナリ、rgb(0x…)=79箇所（round-3 の訂正値どおり） |

**Runtime Proof（主要）**: `cargo tree -i quinn-proto` → `nothing to print`（通常/--target all/-e features すべて）; `cargo tree -i rustls` → gpui→gpui_http_client→zed-reqwest→hyper-rustls（到達）; `cargo tree -i rmcp` → agent-client-protocol→本クレート（到達）; `grep -rn 'AsyncFd' src/` → 0件; `cargo test --doc` EXIT=101 vs `cargo test --locked` EXIT=0。

**ラウンド3→4 の訂正ポイント**: 脆弱性は「26件」より「到達15件、最優先 rmcp 6.5」が正確。tokio soundness は「将来 AsyncFd 導入時に修正必須」へ格下げ。doc test は「使うなら src/lib.rs が前提、回避策は --doc を外す」。

---

## D. `r4-verify-crossreview`（並行チーム成果物の監査、独自発見10クラス + 矛盾6件）

### 前提の更新（指示文が stale）
cross-review は **全ラウンド完了**（round-1〜5 + final、20:35〜20:37）。`todo.md` も空ではなく79行/9528bytes、G1〜G8 が反映済み（20:34）。frontmatter `generated` が実 mtime より未来（round-4 `20:40` vs 実 20:24 等）というメタ矛盾あり。

### D-1 `cross-review-only-findings` [独自発見]
cross-review にしか無い項目を逆輸入候補として特定:
- **`mount-failed-report-ignored`** [high] `cordis_entries_toggle_ui.rs:250-271`, `cordis_composition_ui.rs:176-197`, cordis-rs `loader.rs:344-375`。`MountReport::failed` を両デモがサイレントドロップ。failed 行は entries にも入らず再試行不能。**実コード確認済みの genuinely new バグ**。
- **`disabled-default-bypassed`** [high] `cordis_entries_toggle_ui.rs:238,260`。`with_env_value("DSH_SNAPSHOT_FEEDBACK","1")` + `saved.is_enabled(&id, true)` ハードコードで YAML `disabled` 分岐が一度も通らない。
- **`entry-state-collapse`** [medium-high] disabled / skipped_unknown / 明示 false が「unloaded」1状態に潰れる（round-1 A-10 と部分重複だが state 汚染は独自）。
- **`review-artifacts-not-ignored`** [high] `.gitignore:1` は `/target/` のみ。`git add -A` で investigation/ と cross-review/ が混入し得る。
- **`no-msrv-pin`** [high] `Cargo.toml:4` edition 2024 だが rust-toolchain.toml 不在。
- **`docs-investigation-report-section-missing`** [high] README:60 が参照する「§4.4」がリポジトリ内に無い（非公開資料）。
- **`inconsistent-log-stream`** [high] `acp_probe.rs:32,38` の println! 対 GPUI の eprintln! で smoke grep が誤 FAIL。
- **メタ監査6件** [high] known_in_readme 反転、分母3通り、confidence 定義不統一、重複トップ5、UX43件ドロップ、fixture パスすり替え。
- **`ci-gates-missing`** [独自優位] raw workflow 取得で `undocumented_unsafe_blocks` と `audit: true` の不在を**確認済みギャップ**へ格上げ（DSH 側 round-3 B-7 は「未確認」のまま）。

### D-2 `contradictions-resolved` [矛盾の判定・Runtime Proof 優先]

| # | 争点 | 判定 |
| --- | --- | --- |
| **C1** | ACP handler 不在 → method_not_found | **DSH 側が正しい**。cross-review `final.md:46` の high/confirmed は過大。Runtime Proof: `dsh-acp/src/index.ts:158-176` は条件付き送信、`methods.client` は `:126,:171` の2件のみで fs/terminal 0件。一般 ACP 接続向けとして格下げすべき |
| **C2** | SessionUpdate バリアントドロップ数 | **DSH 側が正しい**。crate 定義11種は事実だが DSH は6種のみ送信、Plan/Commands は unsupported 明記。cross-review の「Plan/Commands が出ない」は DSH 接続では誤り |
| **C3** | fixture エントリ数 7 vs 6 | **cross-review が正しい**。実 id は6件（round-1 D-11 の「7」は未訂正） |
| **C4** | 「5タグ遅れ」vs「4タグ遅れ」 | cross-review の統合が正しい（比較対象が異なるだけで矛盾ではない） |
| **C5** | テスト分母 18bin/19/12 | **cross-review が正しい**（18 bin + main = 19、テスト6、残り13） |
| **C6** | known_in_readme 反転 | **cross-review が正しい**（`yes（欠陥自体は no）` に統一） |

### D-3 `cross-review-stronger-points` [DSH 側が劣っていた点]
- cross-review は `agent-ui-pipeline-without-approval` を「high（潜在）」とし、`tool-call-parser-absent` を抑制要因として明示 → round-3 A-6 より一歩進んでいた。
- `<placeholder-key>` の実キー漏洩は却下（`resolve_api_key` は渡された map のみ参照）→ 妥当。
- CI ゲートの `audit: true` / `undocumented_unsafe_blocks` 不在は確認済みギャップへ格上げ → DSH 側「未確認」を上回る。

### D-4 `todo-integration-policy` [統合方針]
`todo.md` は**追記専用**。既存23〜65行（opencode の成果物）は削除・書き換えしない。C1/C2 は既存行を残したまま直下に訂正行を追加し、`## メモ` 末尾に統合元と日時を追記。`cross-review/` は読み取り専用を維持。

---

## E. `r4-verify-solutions`（解決策の実装可能性検証、6解決策）

### E-1 `channel-duplex-api-wrong` [条件付き] ★重要な訂正
`Channel::duplex()` 自体は存在（`jsonrpc.rs:3355-3373`、feature gate なし、lib.rs:114 再 export）。**しかし round-2 の API 呼び出しは誤り**:
- トレイトは **`ConnectTo`**（`component.rs:109`, `lib.rs:125`）で `ConnectWith` ではない。
- `Builder::connect_to(transport)` は1引数「server-only」で `future::pending()` 待ち（`jsonrpc.rs:1097-1101`）。**Client 側は `connect_with(transport, main_fn)` の2引数が必須**（`:1164-1168`, lib.rs:27-45 quickstart）。`Client.builder().connect_to(channel)` はコンパイル通るがハング。
- `duplex()` はタプル返却なので `connect_with(Channel::duplex())` もコンパイル不可。
- **正形**: `let (cc, ac) = Channel::duplex(); tokio::try_join!(Client.builder().connect_with(cc, async |cx| {...}), Agent.builder().on_receive_request(...).connect_to(ac));` — **並行駆動が必須**。

### E-2 `gpui-input-example-needs-unicode-segmentation` [条件付き] ★Runtime Proof
`cargo build --example input -p gpui --offline` → **EXIT=101**、`unresolved import unicode_segmentation`（input.rs:11 + grapheme_indices 2件、計3エラー）。`gpui-0.2.2/Cargo.toml:445-446` で `unicode-segmentation` は **dev-dependency** のため `-p gpui` ビルドでは解決されない。外部依存は gpui / unicode_segmentation / std のみで `crate::`/`super::` 内部 API 参照なし（他は公開 API）。追加作業: `unicode-segmentation = "1"` を PoC へ（lock に1.12.0既存）+ Entity 化 + `Window::handle_input` + actions/keymap。実 IME(Wayland text-input-v3) 検証は別途。

### E-3 `filterinput-adapter-60-80-lines` [実行可能]
`work/gpui-task-manager/crates/tm-viewmodel/src/filter_input.rs`（481行）は **std のみ**、バージョン依存なし、Apache-2.0。**不整合**: 兄弟の参照 impl `app/src/app.rs:1956-2073`（118行）は gpui-pre 0.3.12 用で `text_length_utf16` / `set_selected_text_range` / `accepts_text_input` / `text_input_configuration` を含む。gpui 0.2.2 のトレイト（`src/input.rs:10-70`）は **8メソッドのみ**でこれらは無く、**逐語コピーはコンパイルエラー**。アダプタ見積もり約60〜80行（委譲6 + 定型2）。`accepts_text_input` 不在で「編集中だけ IME を受け付ける」制御不可（focus handle で代替）。

### E-4 `tray-notify-deps-constraints` [条件付き/困難]
- **notify-rust 4.19.0** は **zbus 6** を要求（`Cargo.toml:140-144`）。workspace lock は zbus 5.12.0。Cargo は major 同居可能だが **zbus 5+6 の二重リンク**リスク（バイナリ増加・型非互換）。D-Bus 経由で Wayland 自体は可。
- **tray-icon 0.25.1** は `ksni` feature で SNI/D-Bus（独自ワーカースレッドでイベントループ不要）。既定の libappindicator は GTK3/X11 で Wayland 不可。Plasma の SNI 実動作は要確認。
- **ashpd 0.13.13** は zbus ^5.13 要求で lock 5.12.0 と**不整合**、ローカルキャッシュ無し＋ネットワーク不可でオフライン解決困難。portal/compositor 依存と D-Bus→GPUI 橋渡しも新規作業。**即時採用は却下**。
- crates.io API はネットワーク失敗で最新バージョンは未確認。

### E-5 `src-lib-extraction-feasible` [実行可能]
`cargo metadata --no-deps`: package `dsh-gpui-client-poc`、bin 19 targets。`src/lib.rs` 追加で lib target 名は **`dsh_gpui_client_poc`**（`-`→`_`）、bins から `use dsh_gpui_client_poc::...` が解決（`[lib]` セクション不要）。`cargo test --doc --offline` EXIT=101 は lib 追加で解消（round-3 の依存関係は正しい）。リスク: 19 bin の逐語複製 mod を共通 use へ置換する横断作業。

### E-6 `cordis-offline-provider-feasible` [実行可能]
pinned checkout `llm.rs:192 from_async_text` / `:219 echo_provider`。PoC `acp_llm_probe.rs:18,67` が既に `from_async_text` を使用（コンパイル済み）。リスク低。**ただし round-2 D-13 の stop_reason 誤用を直さないと final_text assert が無意味**。

**最終リスト**:
- **すぐ着手可**: ① Channel::duplex モック（2引数 connect_with + 並行驱动に修正）② src/lib.rs + 4モジュール ③ echo_provider テスト ④ FilterInput 移植（8メソッド版アダプタ）
- **事前調査要**: ① examples/input.rs（unicode-segmentation + 実 IME）② tray-icon ksni の Plasma 実動作 ③ notify-rust の zbus 6 併存 ④ crates.io 最新（ネットワーク復旧後）
- **却下**: ① `Client.builder().connect_to(channel)`（ハング）② 単引数 `connect_with(Channel::duplex())`（コンパイル不可）③ 兄弟 impl の逐語コピー（4メソッド余剰）④ ashpd 即時採用（zbus 不一致＋オフライン不可）

---

## ラウンド4 集計

| 検証者 | 対象 | 確定 | 誇張 | 反証 | 検証不能 |
| --- | --- | --- | --- | --- | --- |
| r4-verify-acp | ACP 主張6 | 2 | 2 | 2 | 0 |
| r4-verify-security | セキュリティ主張5 | 2 | 3 | 0（下位3） | 2 |
| r4-verify-gates | 検証主張8 | 4 | 4 | 3（下位） | 0 |
| r4-verify-crossreview | 並行チーム監査 | C3/C5/C6 で DSH 劣位を指摘 | C1/C2 で cross-review 過大と判定 | — | — |
| r4-verify-solutions | 解決策6 | 2 実行可 | 4 条件付き/困難 | 4 案を却下 | 1（crates.io 最新） |

### ラウンド4 の最重要結論（最終ラウンドへ引き継ぐ訂正リスト）

1. **ACP 関連の誇張を3つ訂正**: E-5 permission は「条件付き」、E-7 は「6種のうち5種」、E-6 fs/terminal は「DSH 接続では発生しない（一般課題へ格下げ）」。E-4 ClientCapabilities 推測も却下。cross-review の C1/C2 も同趣旨で過大。**一方 D-13（stop_reason 本文化）は confirmed で最優先修正**。
2. **セキュリティの根本原因を修正**: A-6 の真因は「AgentLoop が ToolPipeline を通らない設計」（`let (tools, _)` は症状）。A-5 は「async 化が必須」ではなく「ワーカースレッド＋チャネル等の回避策あり」。A-6 の「stderr 外送信」は refuted（`shell.rs:136` で piped、UI 表示）。A-1/A-2 は confirmed（ただし A-1 は「プロセス環境、6箇所」に訂正）。
3. **脆弱性の実害を絞り込み**: 26件中11件は非到達（quinn 系/quick-xml 0.30.0）。最大 Severity 8.7 も非到達。**実害候補15件、最優先は到達する rmcp `RUSTSEC-2026-0462`（6.5）**。tokio soundness は AsyncFd 未使用で現状発現せず。
4. **推奨解決策の API を修正**: Channel::duplex は `connect_with(2引数)` + `try_join!` 並行駆動が必須（round-2 の提案はハング/コンパイル不可）。FilterInput は gpui 0.2.2 向けアダプタ60〜80行が必要（逐語コピー不可）。ashpd は却下、notify-rust は zbus 6 併存に注意。
5. **並行チームとの統合方針**: cross-review は完成・高品質だが C1/C2 は過大。逆に DSH 側は fixture 数（6）、テスト分母（18+main=19）、known 反転、CI ゲート確認で劣位。**機械マージせず判定を優先**。todo.md は追記専用（既存行を保持）。cross-review 独自の `MountReport::failed` と `disabled` バイパスは実コード確認済みで逆輸入必須。

**ラウンド4 で新たに得られた「未知の残タスク」**: `MountReport::failed` サイレントドロップ、`disabled` バイパス、`review-artifacts-not-ignored`（.gitignore）、`no-msrv-pin`、docs 参照先不在、ログストリーム不整合、CI ゲート `audit`/`undocumented_unsafe_blocks` 不在（確認済み）。

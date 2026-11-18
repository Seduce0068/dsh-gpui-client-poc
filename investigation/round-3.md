# ラウンド3: 実測監査（セキュリティ / ビルド / 依存ライセンス / 整理 / GitHub）

日時: 2026-11-17T20:16+09:00 頃
起動エージェント: `r3-security`, `r3-build-gates`, `r3-deps-license`, `r3-deadcode-consolidation`, `r3-github-discovery`（5並列）
方針: ラウンド1-2が静的解析・外部リサーチだったのに対し、本ラウンドは **実コマンド実行（Runtime Proof）と敵対的レビュー** で未知の残タスクを掘り起こす。

## E. `r3-github-discovery`（GitHub 側のタスク発掘、9項目）

### E-1 `cordis-rs-upstream-v066` [機能追加案/high/high/no]
PoC commit aacbf9a で v0.62.0 にピン。上流 cordis-rs は v0.66.0 まで進行（commits 30d9c94 `ctx.schedule`, 8713b16 `tool-schedule` port, b388f82 `ctx.planMode`, 5fdb0ec `exit_plan_mode`, c4a76d1 v0.66.0 到達）。git dependency なので自動更新されず、v0.63〜66 の新 seam が未アクセス。GPUI デモへの露出候補。

### E-2 `gpui-022-known-issues` [調査必要/high/high/no]
[zed#66275](https://github.com/zed-industries/zed/issues/66275)（closed 2026-10-30）+ コメント。0.2.2 は PinchEvent より前で Windows D&D クラッシュ（#42227 修正未取込み）残存。メンバーは「自動リリース準備中、それまで git revision にピンが必要」と回答。**0.3 への breaking changes の明確な予告は見つからず**。

### E-3 `gpui-open-bugs` [調査必要/medium/medium/no]
[zed#66836](https://github.com/zed-industries/zed/issues/66836)（Linux font cap_height=0）, [#65780](https://github.com/zed-industries/zed/issues/65780)（Wayland + screen-capture）, #66730, #65743。上流 master の issue で 0.2.2 への直接該当は未検証。Kubuntu/Wayland 環境なので #66836/#65780 は注視値。

### E-4 `readme-unimplemented-drift` [残タスク/medium/high/yes]
README:109-113 の5項目のうち「session/update ストリーミング表示」「テキスト入力」は commit 2a60bc7/05c2eea で実装済み。**実残りは Markdown 描画 / テーマ切替 / locale の3項目**（round-1 D-10, round-2 D-5 と同一）。

### E-5 `text-input-self-made` [改善/medium/high/no]
commit 05c2eea + [zed#64389](https://github.com/zed-industries/zed/issues/64389)（IME 合成位置の不具合、open）。`observe_keystrokes` ベースで IME・Unicode 堅牢性は未検証。ただし round-2 B-A2/A3 で判明したとおり gpui 0.2.2 に低レベル入力 API は存在し、移行候補あり。

### E-6 `ci-commit-status-empty` [調査必要/medium/medium/no]
最新 commit 6b6bb1e の `GET /commits/<sha>/status` → `{"state":"pending","statuses":[],"total_count":0}`。GitHub Actions check runs は認証なし API で取得不可。**CI が実際に走っているか（trigger 含む）は要確認**。失敗ゲート/スキップジョブの有無は未確認。

### E-7 `cordis-rs-upstream-gaps` [調査必要/medium/medium/no]
cordis-rs commits adf2198（ADR-001 に gaps 明記）, fd2adbe（todo.md 追加）。本文取得失敗で内容未確認。v0.66 更新前に確認が必要。

### E-8 `real-model-response-unverified` [残タスク/medium/high/yes]
commit 69aaab1, b134528 + README:94,100。残高不足で応答本文未取得。アカウント補充後の実応答確認が残る（外部要因）。

### E-9 `wayland-manual-verification` [改善/low-medium/high/partial]
commit 2a60bc7（tokio sleep → gpui timer 切替）, a8f9174 + README:73。kdotool 不在で目視依存のまま。ただし round-2 D-9 で spectacle 証跡取得が実証済み。

### GitHub 側に「隠れたタスク一覧」は無い（否定結果の明示）

- dsh-gpui-client-poc: issue **0件**（open/closed とも）、PR **0件**、release **0件**。commit 本文の `TODO`/`follow-up`/`未実装`/`next`/`WIP` キーワード **0件**（初期 commit bbdaddf まで遡及）。
- `Seduce0068/workflows`: issue/PR 0件、commit 6件。破壊的変更予告無し（cargo audit オプションゲート 2f8fb81 は PoC 未使用）。
- `Seduce0068/cordis-rs`: issue/PR 0件。
- 一部 MCP 呼び出しが `fetch failed` → web_fetch / ローカル README で補完。

---

## D. `r3-deadcode-consolidation`（残す/統合/削除の整理、25項目）

### Runtime Proof（ラウンド1の数値を訂正）
19バイナリ = main.rs + bin/*.rs 18個（合計2,809行）。`open_window` は **13バイナリ**（round-1 F-13 の「11」は過小）、headless 6（acp_* 5 + bridge）。`rgb(0x…)` は **79箇所**。`cargo machete 0.10.1` → **未使用依存0件**（exit 0、7依存すべて使用 confirmed）。fixture entry は6個（tool-fs / tool-bash の entry は存在しない）。

### D-1 `demo-purpose-keep-merge-map` [改善/high/high/no]
19バイナリを「UIシェル / ACP確認 / cordis統合 / headless」の4層に分類し判定（別表参照）。削除判断を再現可能な基準に。

### D-2 `prompt-ui-replies-dead-and-doc-drift` [残タスク/high/high/no]
`cordis_prompt_ui.rs:1-5,41,50,97-99,128-129`、round-1 A-1 と同一。doc は "wired to the agent loop" だが `replies` は書き込みゼロ。**判断: フィールドと参照行を削除が最小修正**。

### D-3 `prompt-ui-superseded-by-prompt-agent-ui` [改善/high/high/no]
`cordis_prompt_agent_ui` は同じ入力に加え `cx.spawn` 実 agent-loop + busy 制御 + エラー表示を備える上位互換。**prompt_ui を削除して集約する判断が可能**（履歴的価値は README 1行で代替）。

### D-4 `composition-ui-static-subset` [改善/medium/medium/no]
`composition_ui` は静的 mount 表示のみ、`entries_toggle_ui` は同じ fixture + 個別切替 + 永続化を持つ。**削除候補**（confidence medium: 「静的確認の独立価値」反論あり。残すなら registry 共通化のみ必須）。

### D-5 `composition-toggle-vs-entries-toggle` [調査必要/medium/medium/no]
前者は単一 Fiber の最小機構（テストあり）、後者は実構成全エントリ（テストなし）。概念は重複するが実証対象が異なる。残すなら README で位置づけを明記。

### D-6 `acp-probes-keep-distinct-stages` [改善/high/high/yes]
5本は「ハンドシェイク / 単発 prompt / セッション再利用 / 通知ストリーム / AgentLoop 接続」で実証対象が明確に異なる。**統合せず残し、起動ボイラープレートのみ抽出**。

### D-7 `static-shell-keep-but-reword` [残タスク/high/high/yes]
静的2ペインは GPUI レイアウト実証の起点として残す。README:14 を「静的 UI シェルの実証（入力・接続なし）」と明記し、未実装リストを整理。

### D-8 `acp-client-shared-mod` [改善/high/high/no]
6箇所の `Command::new("dsh")` → initialize/session/new を `acp_client::spawn_session() -> Result<Session, AcpError>` に集約。エラー型も統一。`Channel::duplex()` でモック差し替え可能（round-2 C-1）。round-1 A-5 と同一。

### D-9 `build-registry-shared-mod` [改善/high/high/no]
14回の PluginFactory 登録を `build_registry(ctx, home, include_tools)` へ。`sk-demo` キーも一括管理。round-1 A-3 と同一。

### D-10 `text-input-shared-mod` [改善/high/high/no]
`draft/push_char/backspace/observe_keystrokes` と echo_provider が2箇所逐語複製。`text_input::TextInput` + `test_support::echo_provider()` へ。round-1 A-4 と同一。

### D-11 `window-runner-shared-mod` [改善/high/high/no]
`Application::new().run` → `open_window(Default)` → `eprintln` が **13箇所**（round-1 F-13 の11は過小）。only todo_ui は Result を match。共通 `run_demo(name, build)` で生成失敗の扱いも統一。

### D-12 `ui-theme-shared-mod` [改善/high/high/yes]
79箇所の色リテラルを `ui_theme::Palette`（`palette()` + DARK/LIGHT 切替）へ。既知テーマ切替の前提条件。

### D-13 `echo-provider-shared-test-support` [改善/high/high/no]
同一 echo provider 2箇所を `#[cfg(test)]` 用共通モジュールへ。

### D-14 `demos-intentionally-independent` [調査必要/medium/medium/no]
PoC として各デモは独立実験。過剰な共通化は「何を実証したか」の可視性を損なう。**共通化はボイラープレートに限定し、実証対象ロジックは各バイナリに残す**方針を README に1節追加（confidence medium: README に明記なし）。

### D-15 `permission-deny-unreachable` [残タスク/high/high/yes]
answerer が常に Allow で deny 到達不能、bash も固定文字列スタブ。**判断: answerer を UI 切替可能にして deny を実証するか、README を「allow 経路のみ」に修正**。

### D-16 `composition-tool-factories-unused` [残タスク/high/high/no]
`composition_ui.rs:146-174` の tool-fs/bash factory は fixture に entry が無く一度も mount されない。**判断: 削除**（8→6登録）。

### D-17 `skipped-unknown-toggle-noop` [残タスク/high/high/no]
`skipped_unknown` 行は「unloaded」と表示されるがクリック no-op。**判断: 表示を `(skipped: no factory)` に変更し非クリック化**（実装不要、誤表示修正）。round-1 A-10 と同一。

### D-18 `acp-stream-poll-keep-and-fix` [残タスク/high/high/no]
デモは残すが 100ms 無限ポーリング + `.detach()` を、spawn_acp 完了通知でのループ終了または通知ドリブンへ変更。round-1 A-7 と同一。

### D-19 `unused-deps-none-confirmed` [調査必要/high/high/no]（否定結果）
`cargo machete 0.10.1` exit 0。**未使用依存0件**。gpui=13箇所, cordis_rs=12, agent_client_protocol=6, tokio_util=6, futures=5, serde_json=4, tokio=全 bin。**「未使用依存削除」タスクは不要と確定**。

### D-20 `smoke-script-string-only` [改善/high/high/no]
round-1 D-3 と同一。window_runner 抽出後に runner 側結果を共通検証し文字列依存を排除。

### D-21 `common-module-design-with-gpui-license` [改善/high/high/yes] ★
`src/lib.rs` 新設 + 4モジュール（`acp_client` / `ui_theme` / `text_input` / `window_runner`）を `use dsh_gpui_client_poc::{...}` で参照。gpui =0.2.2 をそのまま共有し、**gpui-pre/gpui-component は混用しない**。TextInput は `examples/input.rs` を直接移植、IME は `FilterInput`（Apache-2.0）を利用。

### D-22 `license-boundary` [調査必要/high/high/yes]
PoC = MIT、gpui = Apache-2.0。task-manager の Apache 製 theme/filter_input は帰属明記で移植可、file-explorer の AGPL 製は直接コピー不可（設計参考のみ）。round-2 E-13 と同一。

### D-23 `run-examples-10-missing` [改善/high/high/no]
README:12-32（19行）対 README:36-47（9行）。欠落10バイナリを列挙（round-1 D-4 と同一）。

### D-24 `readme-unimplemented-stale` [改善/high/high/yes]
round-1 D-10 と同一。

### D-25 `readme-restructure-by-purpose` [改善/medium/medium/no]
表を「バイナリ / 実証すること / 実行例 / 既知制約 / 対応モジュール」の5列へ再構成。headless(6) と GUI(13) を分けて記載。

**共通モジュール抽出ロードマップ**: フェーズ0（README 5列化・実行例10件・未実装更新）→ フェーズ1（src/lib.rs + 4モジュール）→ フェーズ2（#8/#17 削除・factory 削除・skipped 表示修正）→ フェーズ3（smoke を runner 結果へ接続・モックテスト・deny テスト）。

---

## B. `r3-build-gates`（決定論的検証の実測、8項目）

### Runtime Proof（実行コマンドと結果）

| コマンド | Exit | 主要出力 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | 0 | 差分なし |
| `cargo check --all-targets --locked` | 0 | Finished in 35.33s; future-incompat warning 2件 |
| `cargo clippy --all-targets --locked -- -D warnings` | 0 | Finished in 7.88s; clippy 警告0件 |
| `cargo test --all-targets --locked` | 0 | 8 passed / 0 failed（テスト保持6バイナリ） |
| `cargo test --doc --locked` | **101** | `error: no library targets found in package` |
| `cargo audit` | **1** | `26 vulnerabilities found!` / 23 warnings / 733 crates |
| `grep -rn 'unsafe' src/` | 1 | 一致0件（§15 遵守） |
| `cargo tree -d` | 0 | 1042行 / 103重複ブロック |
| `rustc` / `cargo` / `cargo audit --version` | 0 | 1.100.0 / 1.100.0 / 0.22.2 |

### B-1 `cargo-audit-26-vulnerabilities` [脆弱性/high/high/no] ★重要
`cargo audit` EXIT=1、26件。内訳: quinn-proto 0.11.13 が8件（最大 Severity 8.7: RUSTSEC-2026-0037）、rustls-webpki 0.103.7 が5件、rustls 0.23.33 が4件、quick-xml 0.30.0/0.37.5 が各2件（7.5 high ×4）、h2 0.4.12 / bytes 1.10.1 / crossbeam-epoch 0.9.18 / quinn-udp 0.5.14 / rmcp 1.8.0 が各1件。加えて unmaintained 15件・unsound 7件・yanked 1件（spin 0.9.8）。**すべて推移的依存**。CI の extra_gates に audit が含まれていればそのまま CI 赤（ただし委譲先の audit は既定 false = round-2 D-16）。

### B-2 `cargo-doc-test-no-lib-target` [テストゲート/high/high/no]
`cargo test --doc --locked` EXIT=101、既存 baseline（src/lib.rs が無く doc test 対象なし）。**doc test を CI ゲートに入れるとそのまま失敗**。ラウンド2 D-18 で提案した `cargo test --doc` を extra_gates に追加する案は **src/lib.rs 追加（round-3 D-21）が前提** と判明（重要な依存関係）。

### B-3 `cargo-tree-103-duplicates` [依存/medium/high/partial]
103重複バージョンブロック。ttf-parser / thiserror / syn / strum / strum_macros / getrandom / bitflags が各3バージョン、他多数が各2。ピン留めによる衝突回避の代償。

### B-4 `future-incompat-warnings` [ツールチェーン/medium/high/no]
num-bigint-dig 0.8.4 と proc-macro-error2 2.0.1 の future-incompat 警告。cargo ノートであり rustc lint でないため `-D warnings` でも EXIT=0。R3B-01 の unmaintained 警告と対象が重複。

### B-5 `four-main-gates-pass` [品質ゲート/high/high/partial]
fmt / check / clippy / test --all-targets の4つはすべて EXIT=0、8 tests passed。同一ツールチェーン・同一 lock なら CI も通過 see込み。

### B-6 `no-unsafe-confirmed` [コンプライアンス/high/high/no]（否定結果）
19個の .rs ファイルで `unsafe` 0件。AGENTS.md §15 遵守。`clippy::undocumented_unsafe_blocks` 必須化しても現状通過。

### B-7 `delegated-workflow-body-unverified` [CI/medium/medium/no]
委譲先 `rust-gates.yml@v1` の本文取得が不安定（GitHub MCP は SHA のみ、web_fetch は truncated、rustxng は timeout）。**audit / doc test が extra_gates に含まれるかは未確認**。ただし同じコマンド・同じ lock なら「ローカル失敗・CI 成功」は起こり得ない。

### B-8 `readme-version-drift-reconfirmed` [ドキュメント/high/high/no]
`Cargo.toml:10` は tag v0.62.0、check ログは `#22290d93`。README:107 は v0.29.0。round-1 D-7 を再確認。

### 補足: 並行セッションの活动
`git status` が `## master...origin/master [ahead 1]` — **別セッション（opencode）が1コミット作成済み**。untracked の `cross-review/` と `investigation/` は既存。私（DSH lead）はコミットしていない。成果物の最終統合時にこの並行活動を考虑する必要がある。

---

## A. `r3-security`（実コード敵対的セキュリティ監査、9項目）

### A-1 `child-inherits-full-parent-env` [改善/high/high/no] ★
`acp_probe.rs:14-18` ほか6箇所、`env_clear()` なし。`Command::new("dsh")` が親環境をそのまま継承。実 `DEEPSEEK_API_KEY` / クラウドトークン / プロキシ認証が `dsh --profile acp` と配下の `dsh-tool-subagent`（provider: spawn）や feedback-producer へ全て渡る。**round-1 の「sk-demo はダミーで漏洩なし」判断は PoC 側の渡し方のみで、親環境継承は未検証だった**。PATH 汚染と組み合わせると攻撃者の `dsh` が全シークレットを取得。`.env_clear()` + 最小限 `.env()` を推奨。

### A-2 `orphan-dsh-on-early-return-or-panic` [改善/high/high/no]
6箇所すべてで `kill_on_drop` 未指定、`child.kill()` は成功パス末尾のみ。`ok_or("no stdin")?` / `spawn()?` / `connect_with(...).await?` の早期リターンや `assert_eq!` panic、`expect("stream buffer poisoned")` で子が孤立。孤立 dsh は継承した認証情報と pipe を持ち続ける。`.kill_on_drop(true)` + RAII ガードを推奨。

### A-3 `unbounded-chunk-buffer-dos` [改善/medium/high/no]
`acp_stream_ui.rs:22-26,34-37,100-108`。chunk text を `Vec<String>` へ上限なし push。100ms ごとの `chunks.join("")` と合わせてメモリ DoS。バイト予算／リングバッファ＋backpressure を推奨。実 exploit は未実行検証。

### A-4 `jsonrpc-no-frame-limit` [残タスク/medium/high/no]
`agent-client-protocol-0.11.1/src/jsonrpc.rs:3313`（`BufReader::new(incoming).lines()` に上限なし）、`transport_actor.rs:96-114`（parse 失敗時に `"line": &line` をエラーへ埋め込み + `tracing::trace!(message = %line)`）。巨大1行で OOM、不正 JSON 行がログへ全文（秘密混入時に二次漏洩）。PoC は `ByteStreams` を直接使い parse error を inspection していない。

### A-5 `sync-answerer-cannot-do-interactive-approval` [残タスク/high→medium/no] ★根本原因
cordis-rs `approval.rs:14-16,49`（`Answerer = Fn(&ApprovalRequest) -> Option<ApprovalDecision>` で同期）、`tools.rs:309-315`（async パイプライン内で同期呼び出し）、`cordis_permission_ui.rs:33-38,48-49`（current-thread runtime + block_on + 常 Allow）。**「承認ダイアログを出してクリックを待つ」は UI スレッドと runtime をデッドロックさせるため構造的に不可能**。これが「常 Allow スタブ」の根本原因。`ApprovalRequest` に args が無く「この `curl ... | sh` を許可しますか？」と表示すること自体できない。Answerer を async/チャネル駆動＋args 追加が必要。

### A-6 `bash-rce-chain-from-untrusted-content` [残タスク/high(潜在)/high/no] ★潜在 critical
`cordis_agent_ui.rs:110,116-121,125-127`（`let (tools, _)` で pipeline を破棄し `with_approval` せず tool-bash mount）、`tool_bash.rs:47-64`（command/workdir はモデル引数）、`shell.rs:131-135`（`sh -c <command>`）。シナリオ: 未信頼コンテンツ（ACP chunk または fs_ui で読ませたファイル）の prompt injection → 実 LLM＋call parser への差し替え時に agent_ui が approval pipeline を捨てているため gate を通らない → `ToolBashConfig::default()`（background/timeout promote = true）のまま `sh -c` → A-1 の環境秘密を読み出し stderr 未捕捉で外部送信。**現状は scripted provider のため発火しない（抑制要因を維持）**。

### A-7 `untrusted-render-content-sanitization` [残タスク/medium/high/no]
`acp_stream_ui.rs:34-59,100-108`, `cordis_fs_ui.rs:27-52`, `cordis_agent_ui.rs:72-78`。現状は `SharedString` プレーンテキストで安全だが、Markdown/リンク/画像実装時に `[open](file:///home/koppe/.ssh/id_rsa)` や `<img src="file:///...">` の自動ロード、`http://attacker/pixel` ビーコン、`javascript:`/`data:` が成立。tool 出力の ANSI エスケープや双方向制御文字（RLO）で "ALLOWED/DENIED" 表示を spoof 可能。スキーム allowlist（https のみ）・自動ロード禁止・制御文字除去を推奨。

### A-8 `session-cwd-not-applied-to-child` [調査必要/low/high/no]
全 `acp_*` の `current_dir().unwrap_or_else(|_| PathBuf::from("/"))` が `NewSessionRequest` に渡るのみで、**子 `Command` に `.current_dir()` を呼ばない**（`grep -rn "\.current_dir" src/` = 0件）。UI が `/path/session` を宣言しても dsh 実体は親起動ディレクトリ（例: `~/.ssh`）で動き、意図しないファイルへの read/write が起こり得る。`cmd.current_dir(cwd)` 明示を推奨。

### A-9 `panic-in-render-drops-lock-and-skips-cleanup` [改善/medium→medium/no]
`acp_stream_ui.rs:35,71,86,91,107,141,160-170`。Mutex poison → GPUI 描画スレッドの `expect` panic + ACP notification callback の `unwrap` panic → status 更新と `child.kill()` を飛ばす。**描画→通知→子プロセス清掃の連鎖**は新規。poison 回復（`unwrap_or_else(|e| e.into_inner())`）と cleanup の RAII 移行を推奨。

**未確認/抑制要因の明示**: A-6 は scripted provider と固定 parser のため現状発火しない。A-3/A-4/A-9 はコード構造確認のみで actual exploit 未実行。A-8 の具体的被害ファイルは未確認。A-7 は将来リスクで現状の SharedString 描画自体は安全。

---

## C. `r3-deps-license`（依存・ライセンス・サプライチェーン、19項目）

### ライセンス
- **C-1 `no-agpl-gpl-in-tree`** [問題なし/high/no] `cargo metadata --offline` 733 packages・35ライセンス種、AGPL/GPL 文字列0件。最も近い `r-efi` は `MIT OR Apache-2.0 OR LGPL-2.1-or-later`（MIT 選択可）。
- **C-2 `mpl2-weak-copyleft`** [要注意/high/no] `cbindgen 0.28.0` / `dwrote 0.11.5` / `option-ext 0.2.0` が MPL-2.0。未改変なら軽微、改変配布なら当該ファイルのソース提供義務。
- **C-3 `agpl-default-vs-mit`** [要注意/medium/yes] AGENTS.md §4 の個人リポジトリ既定 AGPL 対 README の MIT 明示選択。既存判断を尊重しつつ、AGPL 兄弟コードの直接コピー回避の境界を明文化すべき（round-2 E-13 / round-3 D-22 と同一）。
- **C-4 `apache-attribution-obligation`** [要注意/high/no] gpui / ACP は Apache-2.0。バイナリ配布時に LICENSE/NOTICE 保持義務。PoC に third-party notices が無い。
- **C-5 `cordis-license-mit`** [問題なし/high/no] git dependency だが license=MIT で互換。

### 脆弱性
- **C-6 `audit-26-vulnerabilities`** [問題あり/high/no] B-1 と同一だが RUSTSEC ID を特定: bytes 1.10.1 `RUSTSEC-2026-0007` = **CVE-2026-25541**（修正 ≥1.11.1）、crossbeam-epoch `RUSTSEC-2026-0204`, h2 `RUSTSEC-2026-0258`, quick-xml ×4, quinn-proto ×8（RUSTSEC-2026-0037 等）, quinn-udp, rmcp, rustls ×4, rustls-webpki ×5。全て推移依存。**到達性（quinn/rustls/rmcp が ACP 経由で使われるか）は未確認**。
- **C-7 `tokio-pin-blocks-soundness-fix`** [問題あり/high/no] ★直接依存の実害。`Cargo.toml:14 tokio = "=1.48.0"` が `RUSTSEC-2026-0386`（I/O safety 違反、use-after-free / memory-corruption、修正 ≥1.51.6 または ≥1.53.3）の修正をブロック。RUSTSEC-2026-0384 は 1.48.0 非該当。`=` ピンが唯一の実害要因。
- **C-8 `no-advisory-gpui-acp`** [問題なし/high/no] RustSec advisory-db の gpui / ACP / cordis-rs は空。2026-11-17 時点で既知 CVE なし。
- **C-9 `unmaintained-warnings`** [要注意/high/no] warnings 23件。async-std discontinued、anyhow unsound (`RUSTSEC-2026-0190`)、spin yanked 等。テキスト描画・D-Bus 周辺に集中。

### git dependency / サイズ / ピン / 鮮度
- **C-10 `tag-rewritable-sha-recorded`** [要注意/high/no] ★ `Cargo.toml:10 tag="v0.62.0"` は書き換え可能だが `Cargo.lock:1019-1021` に `#22290d93...` を記録（GitHub API の tag SHA と一致）。通常ビルドは再現可。`rev = "22290d9..."` への固定でサプライチェーン強化可能。
- **C-11 `readme-cordis-version-stale`** [要注意/high/yes] round-1 D-7 / B-8 と同一。上流は v0.66.0（round-3 E-1）。
- **C-12 `gpui-heavy-transitives`** [要注意/medium/no] lock 733 crates、gpui 60直接依存。blade/naga/cosmic-text/wayland/x11/resvg/image/ashpd 等。feature で切る術が無く、削減には gpui-pre 移行（不採用済み）か動的リンク方針見直し。
- **C-13 `binary-size-measured`** [問題なし/high/no] release 24M（実測、ユーザー提示と一致）、debug 488M（profile で緩和済み）。**README に24MBの記載は無い**。
- **C-14 `libc-mio-lock-only`** [要注意/high/yes] P-01 とも呼ばれる。lock 喪失時に libc 0.2.191 / mio 1.2.5 へ上がり xattr/ENOATTR 問題再発（round-1 D-5 と同一）。
- **C-15 `acp-caret-pin-inconsistent`** [要注意/high/no] `agent-client-protocol = "0.11.1"` のみキャレット。round-1 cross-review `acp-caret-pin` と同一。
- **C-16 `gpui-latest-is-pinned`** [問題なし/high/no] crates.io API: gpui 最新 = 0.2.2（published 2025-10-22）。`=0.2.2` は妥当。
- **C-17 `acp-major-drift`** [要注意/high/yes] ACP 0.11.1 = 2026-04-21、最新 **5.3.0** = 2026-11-14。7か月で4メジャー進行。追従計画が必要。
- **C-18 `tokio-patch-lag`** [要注意/high/yes] tokio 最新 1.54.1 対 lock 1.48.0（6マイナー遅れ）。C-7 の修正も取り込めていない。
- **C-19 `libc-mio-upstream-fresh`** [要注意/high/no] libc 最新 0.2.191 / mio 1.2.5 対 lock 0.2.177 / 1.1.0。

**未確認**: `cargo update` 未実行（更新時のビルド成否は推論）。V-01/C-6 の到達性未追跡。MPL-2.0 クレートの改変有無未検証。web_search/web_fetch が HTTP 障害で失敗し crates.io/GitHub API の読み取り GET で代替。

---

## ラウンド3 集計

| 観点 | 検出数 | 主な貢献 |
| --- | --- | --- |
| A: セキュリティ | 9 | 子プロセス環境継承、同期 Answerer の構造的破綻、bash RCE チェーン（潜在） |
| B: ビルドゲート | 8 | **cargo audit 26脆弱性 exit 1**、doc test exit 101、4ゲート pass、unsafe 0件 |
| C: 依存ライセンス | 19 | **tokio 1.48.0 の soundness advisory が `=` ピンで修正不能**、cordis tag→rev 固定提案 |
| D: デッドコード | 25 | 19バイナリ判定表、4モジュール抽出設計、実測訂正（window 13/色79/未使用依存0） |
| E: GitHub | 9 | cordis-rs v0.66.0 未取込み、issue/PR 0件（隠れたタスク一覧なし） |

**合計 70項目**（重複含む）。

### ラウンド3 の最重要結論

1. **セキュリティ上最優先は A-1（子プロセスの全環境継承）と A-5（同期 Answerer の構造的破綻）**。前者は実際の API キーが dsh と subagent へ流れる実経路で、後者は「常 Allow スタブ」を直せない根本原因（async/channel 化＋args 追加が必要）。A-6 の bash RCE チェーンは現状 scripted provider のため発火しないが、実 LLM 接続時に顕在化する。
2. **依存監査が2つの実害を検出**: `cargo audit` は26件 exit 1（TLS/QUIC 系中心、推移的）、さらに **直接依存 tokio 1.48.0 が RUSTSEC-2026-0386（soundness）で `=` ピンにより修正不能**。gpui 0.2.2 自体は crates.io 最新で advisory なし。
3. **doc test ゲート追加には `src/lib.rs` が前提**: `cargo test --doc` は lib target 不在で exit 101（既存 baseline）。round-2 D-18 で提案した extra_gates への追加は、round-3 D-21 の `src/lib.rs` 新設と順序依存にある。
4. **GitHub 側に隠れたタスク一覧は無い**（issue/PR/release/TODO キーワードすべて0件）。未知の残タスクはコード・依存・上流の差分に存在する。

### 並行セッション（opencode）との関係

`cross-review/` と空の `todo.md` が並行セッションにより作成されている（round-3 B-8 補足で確認、git は `ahead 1`）。本 DSH セッションは `investigation/` を担当し、`cross-review/` は触らない。**最終ラウンド6では両方の成果物を `investigation/final.md` に統合し、`todo.md` は上書きせず追記・統合する**（AGENTS.md §12 準拠）。

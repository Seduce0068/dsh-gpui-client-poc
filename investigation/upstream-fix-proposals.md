---
type: Reference
title: "cordis-rs 上流側最小修正提案（2件）"
description: >-
  agent-ui-pipeline-no-gate と cordis-approval-guard-reentrancy について、
  ローカル固定版 cordis-rs のソースを読み、上流側の最小修正
  （関数シグネチャ／guard drop 位置）をコード断片付きで提案する。
  host 側回避策とその不可理由も記録する。
tags:
  - investigation
  - cordis-rs
  - upstream-fix
  - approval
  - pipeline
generated: 2026-11-18T17:29:48+09:00
verified: 2026-11-18T17:29:48+09:00
status: current
---

## 結論（先に読む）

- `agent-ui-pipeline-no-gate`: 真因は `AgentLoop::run` が `ToolPipeline`
  を通らず `ToolRegistry::invoke` を直接呼ぶ設計。上流の最小修正は
  **pipeline 注入 API の追加**（`AgentLoop::with_pipeline` と `run` 内の
  dispatch）。現行固定版 f64f328 には既に反映済み。
- `cordis-approval-guard-reentrancy`: `ApprovalService::request` が
  read guard を保持したまま answerer を同期呼び出す。上流の最小修正は
  **policy と answerer 一覧を guard 下で複製し、callback 前に guard を
  drop する**。現行固定版にも未反映。
- host 側の「mpsc + UI 回答」回避策は、UI が `add_answerer` /
  `set_policy` を呼ぶと RwLock 再入でデッドロックするため単独では
  成立しない。上流修正が先決である。

## 参照したローカル固定版ソース（Runtime Proof）

- 依存指定: `Cargo.toml:10` = git tag `v0.136.0`。
  `Cargo.lock:1021` = commit
  `f64f328b431d495fbf2328e3dab04f93c895a889`。
- checkout:
  `~/.cargo/git/checkouts/cordis-rs-5f2363c780da6763/f64f328/`
  （以下「現行固定版」と表記）。
- 比較用の旧版: 同じく `22290d9/`（v0.62.0、以下「旧固定版」）。
- host: `src/bin/cordis_agent_ui.rs`、`Cargo.toml`。
- 既存調査: `investigation/round-4.md:29,34`、
  `investigation/round-5.md:57-58`、`investigation/final.md:45-46,61`、
  `todo.md:66-67`。

## 1. `agent-ui-pipeline-no-gate`

### 1-1. 真因と根拠

- 旧固定版 `22290d9/src/services/agent_loop.rs:234`:
  `let result = self.tools.invoke(&call.name, call.args, exec).await;`
  `ToolPipeline::run` を通らないため approval / middleware /
  post-execute / 実行キャンセル検査が一切適用されない。
  `AgentLoop` に pipeline フィールドも注入 API も無い。
- host の `let (tools, _) = provide_tools(&ctx)`
  （`src/bin/cordis_agent_ui.rs:119`）は症状であって原因ではない。
  `investigation/round-4.md:29,34` の訂正を採用する。
- 現行固定版 `f64f328/src/services/agent_loop.rs:68,123,305-307` では
  上流が既に修正済みであることをローカルソースで確認した。

### 1-2. 上流側の最小修正（関数シグネチャ）

旧固定版（修正前・要約）:

```rust
// 22290d9/src/services/agent_loop.rs:61-90,234
pub struct AgentLoop {
    llm: LlmService,
    tools: ToolRegistry,
    // pipeline フィールドなし
    // ... 既存フィールド
}

pub fn new(llm: LlmService, tools: ToolRegistry) -> Self { /* ... */ }

// run() 内
let result = self.tools.invoke(&call.name, call.args, exec).await;
```

提案（現行固定版に含まれる形。追加はフィールド1つと builder 1つだけ）:

```rust
// f64f328/src/services/agent_loop.rs:61-91
pub struct AgentLoop {
    llm: LlmService,
    tools: ToolRegistry,
    // 既定 pipeline も経由させる。approval は host が注入する。
    pipeline: Option<crate::services::tools::ToolPipeline>,
    // ... 既存フィールドは変更なし
}

pub fn new(llm: LlmService, tools: ToolRegistry) -> Self {
    Self {
        llm,
        tools,
        pipeline: Some(crate::services::tools::ToolPipeline::default()),
        // ... 既存初期化は変更なし
    }
}

// f64f328/src/services/agent_loop.rs:121-126（追加 API）
pub fn with_pipeline(
    mut self,
    pipeline: crate::services::tools::ToolPipeline,
) -> Self {
    self.pipeline = Some(pipeline);
    self
}

// f64f328/src/services/agent_loop.rs:305-308（run 内の dispatch）
let result = match &self.pipeline {
    Some(pipeline) => {
        pipeline
            .run(&self.tools, &call.name, call.args, exec)
            .await
    }
    None => self.tools.invoke(&call.name, call.args, exec).await,
};
```

- 変更される公開シグネチャは `with_pipeline(self, ToolPipeline) -> Self`
  の追加のみ。`run` のシグネチャは不変。
- `provide_tools` は旧版から既に `(ToolRegistry, ToolPipeline)` を返して
  おり（`f64f328/src/services/tools.rs:420-425`）、上流の変更は不要。

### 1-3. host 側回避策と不可理由

- **可能（現行固定版）**: `wire_agent_loop`
  （`src/bin/cordis_agent_ui.rs:97-114`）で `provide_tools` の第2戻り値を
  保持し、`pipeline.with_approval(approval)` を付与した上で
  `AgentLoop::new(..).with_pipeline(pipeline)` を呼ぶ。
  `AgentLoop` は直後に `Arc` 化されるため wire 時に決める必要がある。
- **不可（旧固定版 v0.62.0）**: pipeline フィールドも `with_pipeline` も
  無く、`run()` の `self.tools.invoke` を差し替える API が無い。
  `ToolRegistry` の handler をラップしても approval / middleware /
  post-execute / cancel check（`f64f328/src/services/tools.rs:304-399`）は
  再現できず、host に同じロジックを複製するのは fork 相当になる。
- **現行固定版でも未配線**: 既定 pipeline は answerer 未登録のため
  fail-open（`agent_loop.rs:64-67`、`tools.rs:253-257`）。host が第2戻り値を
  捨てている `cordis_agent_ui.rs:119` を直さない限り gate は載らない。
- 対話的な UI 承認に繋ぐには §2 の修正も必須である。

## 2. `cordis-approval-guard-reentrancy`

### 2-1. 真因と根拠

- 現行固定版 `f64f328/src/services/approval.rs:130-150`:
  `let guard = self.state.read()...`（134行）を保持したまま 141-142行で
  `answerer(req)` を同期呼び出す。
- `add_answerer`（97-116行、103行で `drop(guard)`）と `set_policy`
  （119-121行）は write guard を取る。answerer が UI に渡り UI がこれらを
  呼ぶと再入デッドロック。`policy()` / `answerer_names()` の read も、
  writer が待機中なら std RwLock の writer 優先でブロックし得る。
- 旧固定版 `22290d9/src/services/approval.rs:128-149` も同一。
  v0.67.0 も同一と記録され（`investigation/round-5.md:58`）、
  現行固定版 v0.136.0 でも未修正をローカルソースで確認した。

### 2-2. 上流側の最小修正（guard drop 位置）

現状（要約）:

```rust
// f64f328/src/services/approval.rs:130-150
pub fn request(
    &self,
    req: &ApprovalRequest,
) -> std::result::Result<ApprovalOutcome, ApprovalError> {
    let guard = self.state.read().expect("approval state poisoned");
    if guard.policy == ApprovalMode::Never {
        return Ok(ApprovalOutcome {
            decision: ApprovalDecision::Deny,
            answerer: Some("policy:never".to_string()),
        });
    }
    for (_, name, answerer) in guard.answerers.iter().rev() {
        if let Some(decision) = answerer(req) {
            return Ok(ApprovalOutcome {
                decision,
                answerer: Some(name.clone()),
            });
        }
    }
    Err(ApprovalError::Unavailable)
}
```

提案（guard をブロックで閉じ、callback の前に drop する）:

```rust
// 提案: f64f328/src/services/approval.rs:130-150 を置き換え
pub fn request(
    &self,
    req: &ApprovalRequest,
) -> std::result::Result<ApprovalOutcome, ApprovalError> {
    let (policy, answerers) = {
        let guard = self.state.read().expect("approval state poisoned");
        (guard.policy, guard.answerers.clone())
    }; // ← 追加箇所はこのブロック末尾（guard がここで drop される）

    if policy == ApprovalMode::Never {
        return Ok(ApprovalOutcome {
            decision: ApprovalDecision::Deny,
            answerer: Some("policy:never".to_string()),
        });
    }
    for (_, name, answerer) in answerers.iter().rev() {
        if let Some(decision) = answerer(req) {
            return Ok(ApprovalOutcome {
                decision,
                answerer: Some(name.clone()),
            });
        }
    }
    Err(ApprovalError::Unavailable)
}
```

- `answerers` は `Vec<(u64, String, Answerer)>` の複製だが、`Answerer` は
  `Arc`（`approval.rs:49`）なので各要素の複製は軽量。
- `Answerer` 型エイリアスと `request` の公開シグネチャは不変。
  `add_answerer` の `drop(guard)`（103行）と同じ流儀で、修正量は
  スナップショット取得と guard のスコープ化だけ。
- callback 中に登録された answerer は当該 request には反映されない
  （スナップショット意味論）。逆順の waterfall 順序は維持される。
- answerer 自体を async にする変更は `Answerer` 型と
  `f64f328/src/services/tools.rs:342` の呼び出し側を変える破壊的変更
  であり、デッドロック解消の最小修正には含めない（第2段の課題）。

### 2-3. host 側回避策と不可理由

- **可能だが対話的でない**: 予め決めた `AtomicBool` answerer を登録する
  方法（`investigation/round-4.md:28`）は動くが、ユーザーへの確認を
  伴わないため承認 gate の目的を満たさない。
- **不成立**: ワーカースレッド + `mpsc::recv()` で待ち、UI が回答を送る
  方法（`investigation/round-4.md:28,34` の提案）。理由は次のとおり。
  - UI が `add_answerer` / `set_policy` を呼ぶ瞬間に write guard を
    取得できず RwLock 再入デッドロックする（round-5 E-3）。
  - host は `ApprovalService.state`（`approval.rs:85-88`、private）へ
    アクセスできず、guard を drop する手段を持たない。
  - `answerer(req)` は同期 `Fn`（`approval.rs:49`）なので GPUI executor を
    `.await` できない。async 化には上流の型変更が必要。
  - `main` は `new_current_thread()`
    （`src/bin/cordis_agent_ui.rs:228`）のため、同じ thread で
    `recv()` ブロックすると runtime 全体が停止する。
  - `ApprovalRequest` は `action` / `description` のみ
    （`approval.rs:14-17`）、description も固定文字列
    （`tools.rs:338-341`）で、画面に tool 引数を表示できない。
    引数表示には上流の struct 変更が必要（独立課題）。
  - `ToolPipeline::with_approval` を付けても pipeline は
    `approval.request()` を同期呼ぶだけ（`tools.rs:342`）なので、
    デッドロックは解消しない。

## 推奨する上流 PR の分割

1. PR-A（issue 1、現行固定版 v0.136.0 に反映済み）: `AgentLoop` への
   pipeline 注入 API 追加と `run` 内の dispatch。
2. PR-B（issue 2、未反映・最優先）: `ApprovalService::request` の
   guard スナップショットと callback 前 drop。
3. PR-C（任意・破壊的）: answerer / request の async 化と
   `ApprovalRequest` への tool 引数追加。

## 検証と未実施事項

- 本レポートは新規ファイル1件のみの作成で、既存ファイルは未変更。
- 参照したコード断片と行番号はローカル固定版ソースを read して確認した
  （Runtime Proof）。上流リポジトリへの PR 作成は行っていない。
- 文字検証: `markdownlint investigation/upstream-fix-proposals.md` を
  実行し、exit code 0（警告なし）を確認した。
- Rust の compile / test は本タスクでコード変更がないため未実施。

## 根拠ファイル

- `Cargo.toml`、`Cargo.lock`、`src/bin/cordis_agent_ui.rs`
- `~/.cargo/git/checkouts/cordis-rs-5f2363c780da6763/f64f328/`
  - `src/services/agent_loop.rs`
  - `src/services/approval.rs`
  - `src/services/tools.rs`
- `~/.cargo/git/checkouts/cordis-rs-5f2363c780da6763/22290d9/`
  - `src/services/agent_loop.rs`
  - `src/services/approval.rs`
- `investigation/round-4.md`、`investigation/round-5.md`
- `investigation/final.md`、`todo.md`

# 残タスク・機能追加案 調査ログ

目的（ユーザー回答）: 未知の残タスク、機能追加案などを調査する。エージェントチームをふんだんに使う。5回繰り返し、6回目で重複、妥当性などを吸収する。

## 進捗

| ラウンド | 観点 | 状態 | 成果物 |
| --- | --- | --- | --- |
| 1 | コードベース/ドキュメント/ACP/cordis/UX の5角度並列調査 | **完了**（115項目、重複あり） | `round-1.md` |
| 2 | 外部/上流リサーチ + 検証戦略（87項目・r1 訂正4件） | **完了** | `round-2.md` |
| 3 | セキュリティ/ビルド/依存ライセンス/デッドコード/GitHub の実測監査（70項目） | **完了** | `round-3.md` |
| 4 | 敵対的妥当性検証（主張の反証・訂正11件） | **完了** | `round-4.md` |
| 5 | 実実行観察・エッジケース・将来性・自己監査（79項目） | **完了** | `round-5.md` |
| 6 | 重複排除・妥当性評価・優先順位付き最終取りまとめ（351件→約40項目） | **完了** | `final.md` |

## 最終結果

エージェント25体・5ラウンド（115/87/70/反証/79件＝351件・重複含む）＋6回目の統合で、
反証・誇張11件を除外し約40件の実質アクションへ集約。`todo.md` に訂正行として追記済み
（opencode チームの既存行は保持）。詳細は `final.md` を参照。

件数の根拠: ラウンド2 は `round-2.md` の生集計 88 項目を 1 件過大として訂正し 87 項目
とした（根拠: `final.md:27`）。合計 351 = 115 + 87 + 70 + 79 で、ラウンド4 の反証
新規7件は訂正扱いとして累計に加算しない（根拠: `final.md:20`）。

## 調査対象

- リポジトリ: `/home/koppe/document/deepseek-harness/default-workspace/work/dsh-gpui-client-poc`
- GitHub: <https://github.com/Seduce0068/dsh-gpui-client-poc>
- 主要技術: Rust / GPUI `=0.2.2` / ACP `0.11.1` / cordis-rs `v0.426.0`

## 既知の「未実装」（README 記載 = 新規発見ではない）

- GPUI の `Render` と ACP の Tokio セッションの統合
- Markdown 描画、テーマ切替、locale / i18n（現行 README でも未実装のまま）

次は 2026-11-17 時点の「未実装」記載。現行では実装済みのため、現行状態は
ルート `README.md` の「未実装 / 未確認」を参照。

- `session/update` のストリーミング表示: 実装済み。`AgentMessageChunk` に加え
  `ToolCall` / `ToolCallUpdate` / `AgentThoughtChunk` を描画し、`UsageUpdate` は
  ignored 理由を表示する。
- テキスト入力: `cordis_prompt_ui` でキーボード入力と送信表示を実装済み。
  残るは IME composition / カーソル / 選択の接続と AgentLoop 実行
  （`cordis_prompt_agent_ui`）。

## 検出項目の統一フォーマット

各ラウンドの成果物では、各項目を次の構造で記録する:

- `id`: 短い slug
- `title`: 見出し
- `category`: 残タスク / 機能追加案 / 改善 / 調査必要
- `evidence`: ファイル:行 または 出典 URL、あるいは明示した推論
- `description`: 何が欠けている/提案か
- `value`: 価値・根拠
- `confidence`: high / medium / low
- `known_in_readme`: README に記載済みか (yes/no)

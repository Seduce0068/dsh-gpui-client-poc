# Third-party license notices

以下は、本リポジトリを `AGPL-3.0-or-later` に変更する前に適用されていたライセンス本文です。
上流の著作権表示・許諾通知を保持するため、変更後もここに残します。
加えて、Apache-2.0 依存の著作権表示とライセンス写しへの導線を記載します。

---

MIT License

Copyright (c) 2026 Seduce0068

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

---

## Apache-2.0 依存

本リポジトリは以下の Apache-2.0 依存を含む。ライセンス全文は
[LICENSE-APACHE.txt](LICENSE-APACHE.txt) を参照。

### gpui 0.2.2

- ライセンス: Apache-2.0
- 著作権表示: Copyright 2022 - 2025 Zed Industries, Inc.
- 出典: <https://github.com/zed-industries/zed>
- 根拠: 公開 crate の `LICENSE-APACHE` に著作権表示と本文を同梱。
  NOTICE ファイルは同梱されていない。

### agent-client-protocol 0.11.1

- ライセンス: Apache-2.0
- 著作権表示: Copyright 2025 Zed Industries, Inc. and contributors
- 出典: <https://github.com/agentclientprotocol/rust-sdk>
- 根拠: 上流ルートの `LICENSE` に Apache-2.0 本文と著作権表示を確認。
  公開 crate 自体には LICENSE / NOTICE が同梱されていない。

### agent-client-protocol-schema 0.12.0（推移依存）

- ライセンス: Apache-2.0
- 著作権表示: Copyright 2025 Zed Industries, Inc. and contributors
- 根拠: ローカルに同梱された `LICENSE` に著作権表示を確認。

## NOTICE ファイルについて

gpui / agent-client-protocol の公開 crate と上流既定ブランチのルートには
NOTICE ファイルが見つからなかった。この根拠だけでは Apache-2.0 §4(d) の
NOTICE 複製義務が発動するとは断定しない。上流で NOTICE が追加された
場合は、ここへ追記する。

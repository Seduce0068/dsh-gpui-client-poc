---
type: Reference
title: "Windows/macOS ビルド戦略: gpui / gpui-kit feature と CI 追加前の条件"
description: "gpui 0.2.2 / gpui-kit 0.9.1 の feature 調査と、Windows / macOS CI ジョブ追加前に必要な条件の記録"
tags:
  - gpui
  - gpui-kit
  - ci
  - cross-platform
generated: 2026-11-18T16:50:30+09:00
status: needs-validation
---

## 結論（先に読む）

- 現行 `Cargo.toml:12` は `gpui = "=0.2.2"` で feature を明示していない。
  したがって `default = ["font-kit", "wayland", "x11", "windows-manifest"]`
  が有効である（gpui 0.2.2 `Cargo.toml:38-45` / `Cargo.toml.orig:20`）。
- **ローカルに Windows / macOS の実機もクロスターゲットも無いため、
  両 OS のビルド成否は未確認である。**
  `rustup target list --installed` の出力は
  `x86_64-unknown-linux-gnu` のみ（本セッションで実測）。
- gpui 0.2.2 の同梱 README は
  "be on macOS or Linux" とのみ記載しており、
  **Windows は公式に案内されていない。**
  Windows 向け target 依存は存在するが、サポートの表明は無い。
- 共通ワークフロー `rust-gates.yml@v1` は `runner` 入力を持つが、
  `sudo` と `apt-get` のクリーンアップが無条件で実行されるため、
  macOS / Windows runner へそのまま `runner` を差し替えても失敗する。
  本リポジトリ側でゲートを複製せず、提供側での OS 分岐が必要である。

## 調査範囲と方法

- 実施日時: 2026-11-18（Asia/Tokyo）。
- 対象: 本リポジトリの `Cargo.toml` / `Cargo.lock` /
  `.github/workflows/ci.yml` / `README.md` / `todo.md` /
  `cross-review/cycle-001/round-5.md`、およびローカル cargo registry の
  `gpui-0.2.2` / `gpui-kit-0.9.1` / `gpui-pre-0.3.12` /
  `gpui-pre-platform-0.3.12` / `embed-resource-3.0.12`。
- 実測コマンド:
  `cargo tree --offline -e features -i gpui`、
  `rustup target list --installed`、`rustc --version`。
- 外部情報: 共通ワークフローの raw ファイルを 1 点取得したのみ。
  取得内容は参考データとして扱い、実行結果は未確認とした。
- 「未確認」の扱い: ローカルで実行・観測していない項目は、
  推測であっても必ず未確認と明記する。

## 現状の feature 選択

`cargo tree --offline -e features -i gpui` の実測では、
`dsh-gpui-client-poc` の default feature から
`gpui feature "default"` が有効化され、
そこから `wayland` / `x11` 経由で
`blade-graphics` / `blade-macros` / `blade-util` / `bytemuck` /
`ashpd` / `cosmic-text` / `xkbcommon` / `open` 等が活性化されている。

- `wayland` / `x11` が引く依存の多くは
  `gpui-0.2.2/Cargo.toml:453-577` の
  `[target.'cfg(any(target_os = "linux", target_os = "freebsd"))'
  .dependencies.*]` 配下にある。
  つまり macOS / Windows では feature 名は有効でも、
  これらの optional 依存自体はターゲット条件で除外される。
- `windows-manifest = []` は空の feature だが、
  `gpui-0.2.2/build.rs:263-272` の
  `#[cfg(feature = "windows-manifest")] embed_resource()` が
  `resources/windows/gpui.rc` を埋め込む。
  Windows では `embed-resource 3.0.12` が
  `rc.exe`（Windows SDK）を探索する
  （`embed-resource-3.0.12/src/windows_msvc.rs:34-69`）。
- Linux の baseline は既に確認済みである。
  `.github/workflows/ci.yml:18-25` の apt 3 パッケージ
  （`libxcb1-dev` / `libxkbcommon-dev` / `libxkbcommon-x11-dev`）で
  `cargo build --release` が成功し、Wayland ネイティブ描画も
  `README.md:54-56,83-85` で実証されている。

## gpui 0.2.2 のプラットフォーム別条件

### macOS

- `gpui-0.2.2/build.rs:57-62` は `CARGO_CFG_TARGET_OS == "macos"` で
  `macos::build()` を呼ぶ。
- `generate_dispatch_bindings()` は `bindgen 0.71`
  （`build.rs:82-104`）を使用する。
  bindgen が一般に libclang を要する前提はあるが、
  本リポジトリの clang 環境での要否・動作は未確認である。
- `generate_shader_bindings()` は `cbindgen 0.28`
  （`build.rs:106-170`）で `scene.h` を生成する。
- `runtime_shaders` feature が無効な既定では
  `compile_metal_shaders()`（`build.rs:183-228`）が
  `xcrun -sdk macosx metal` と `xcrun -sdk macosx metallib` を実行し、
  `-mmacosx-version-min=10.15.7` を指定する。
- gpui 同梱 README:22-38 は
  Xcode と Xcode command line tools の導入、
  `sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer`
  を要求する。
- 代替 feature は `runtime_shaders`（シェーダの実行時コンパイルへ切替）
  と `macos-blade`（blade / WGSL 経由）。
  いずれも本リポジトリでは未使用・未検証である。

### Windows

- gpui 同梱 README:8 の文言は
  "be on macOS or Linux" であり、Windows の記載が無い。
  target 依存として `windows = "0.61"` と
  `Win32_Graphics_Direct3D11` / `DirectComposition` / `DirectWrite` /
  `Dxgi` / `Win32_UI_Input_Ime` 等が
  `gpui-0.2.2/Cargo.toml:661-727` に存在するが、
  **ビルド可能かどうかは未確認**である。
- release ビルドのみ `#[cfg(not(debug_assertions))] compile_shaders()`
  （`build.rs:258-265`）が走り、HLSL を `fxc.exe` でコンパイルする。
  `find_fxc_compiler()`（`build.rs:326-350`）は
  `GPUI_FXC_PATH` → `where.exe fxc.exe` →
  `C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\fxc.exe`
  の順に探索し、見つからなければ panic する。
- `cargo check --all-targets` / `cargo test --all-targets` は
  dev プロファイルなので `compile_shaders()` は呼ばれない。
  ただし `windows-manifest` による `embed_resource()` は
  プロファイルに依存せず呼ばれるため、
  `rc.exe`（Windows SDK）側の条件は残る。
- `[profile.dev] debug = "line-tables-only"` は
  debuginfo 量の抑制であり `debug_assertions` を無効化しない。
  したがって fxc の要否は変わらない（コード上の推論、未実測）。

## gpui-kit の feature（比較・将来候補）

- ローカル registry の `gpui-kit 0.9.1` は
  `longbridge/gpui-kit`（Apache-2.0）のクレートである。
  `Cargo.lock` には `gpui-kit` は存在せず、**現行依存ではない**。
- 主な依存は `gpui-pre 0.3.12`（package 名を `gpui` から変更）、
  `gpui-base 0.9.1`、任意の `gpui-component 0.9.1` /
  `gpui-kit-assets 0.9.1` / `gpui-fast 0.1.14`、
  および `gpui-pre-platform 0.3.12` である。
- `gpui-kit` の feature は
  `default = ["component", "assets"]` のほか
  `gpui-fast` / `inspector` / `profiler` / `speech` / `syntect` /
  `tree-sitter-*` / `test-support` 等である。
  `wayland` / `x11` / `windows-manifest` という名の feature は
  `gpui-kit` 直には無い。
- プラットフォーム系 feature は下位クレート側にある。
  `gpui-pre 0.3.12` は
  `default = ["font-kit", "wayland", "x11", "windows-manifest"]` で、
  `windows-manifest = ["dep:embed-resource"]` である。
  `gpui-pre-platform 0.3.12` は
  `wayland` / `x11` / `font-kit` / `screen-capture` /
  `test-support` 等を公開する。
- `gpui-fast 0.1.14` 依存には
  `features = ["windows-manifest-provided-by-host"]` が付く。
  manifest をホスト側で提供する趣旨と解釈できるが、
  意図の確認と実ビルドは未実施である。
- `todo.md:137` の「案B（gpui-pre + gpui-component）移行」は
  要相談事項であり、今回の CI 追加とは別案件として扱う。
  両者を混在させた変更は行わない。

## 共通ワークフローの制約

`https://raw.githubusercontent.com/Seduce0068/workflows/v1/
.github/workflows/rust-gates.yml`（2026-11-18 取得）を確認した。

- `on.workflow_call.inputs.runner` は存在し、
  既定値は `ubuntu-24.04` である。
- しかし「ディスクを空ける」ステップは無条件で
  `sudo rm -rf /usr/share/dotnet ...` と `sudo apt-get clean` を実行し、
  `set -euo pipefail` を付ける。
  macOS / Windows runner では `apt-get` や `sudo` が無く失敗する。
  Windows の既定 shell は PowerShell であり、
  `set -euo pipefail` 自体も成立しない。
- `apt_packages` 入力も Linux 専用である。
  現行の `.github/workflows/ci.yml:18-25` はこの入力に依存する。
- `~/.agents/AGENTS.md` §5 の方針に従い、
  対応は提供側 `Seduce0068/workflows` の
  `rust-gates.yml` で OS 分岐を行い、
  `self-test.yml` で検証してからタグ `v1` を動かすのが筋である。
  呼び出し側にゲートを複製しない。
- macOS / Windows runner で本ワークフローを呼び出した実例は
  確認しておらず、上記はコード上の読解に基づく（未実測）。

## CI 追加前に必要な条件（チェックリスト）

1. feature 方針を明示する。
   `default` のままにするか、
   `default-features = false` して必要 feature を列挙するかを決定する。
   Linux で `wayland` / `x11` を両方外すと
   バックエンドが無くなるため、少なくとも片方は必要である
   （コード上の推論、未実測）。
2. 共通ワークフロー側で OS 分岐を入れる。
   apt / sudo のクリーンアップを Linux のみに限定し、
   `runner` 入力で `macos-*` / `windows-*` を受け付ける構成を
   `self-test.yml` で検証してからタグを更新する。
3. macOS runner で Xcode / CLT と
   `xcrun -sdk macosx metal` / `metallib`、bindgen の動作を確認する。
   必要なら `runtime_shaders` / `macos-blade` への切替を検討する
   （いずれも未検証）。
4. Windows はまず「追加するか」を判断する。
   gpui の README が macOS / Linux のみを案内しているため、
   サポート外として見送る選択肢がある。
   追加するなら `rc.exe` と、
   release ビルド時の `fxc.exe` / `GPUI_FXC_PATH` を整備する。
5. 各 OS runner で
   `cargo check --all-targets --locked` と
   `cargo build --release` を実際に実行し、結果を記録する。
6. feature unification の影響を再確認する。
   `cargo tree --offline -e features -i gpui` を
   依存更新後に再実行し、意図しない feature の増加を検出する。
7. 本タスク外だが、配布前には
   `todo.md:113` の Apache-2.0 NOTICE 同梱（G3）も別途解消する。

## 未確認項目一覧

- Windows ビルド成否: 未確認。
  根拠はローカル target が Linux のみであること。
  解消には Windows runner での `cargo check` / `cargo build` 実行が必要。
- macOS ビルド成否: 未確認。
  同上。macOS runner での実行が必要。
- `xcrun` / `metallib` の runner 上での利用可否: 未確認。
  根拠は `build.rs:183-228`。CI 実実行で確認する。
- `rc.exe` / `fxc.exe` の runner 上での利用可否: 未確認。
  根拠は `embed-resource` と `build.rs:326-350`。CI 実実行で確認する。
- `runtime_shaders` / `macos-blade` の実効果: 未確認。
  feature 定義のみを確認し、実ビルドはしていない。
- gpui-kit の Windows / macOS 対応状況: 未確認。
  `Cargo.lock` に無く、ビルド検証もしていない。別タスクで扱う。
- GitHub Hosted runner の preinstalled 状況: 未確認。
  公式の runner-images ドキュメントで要確認。

## 出典

- ローカル:
  `Cargo.toml`、`Cargo.lock`、`.github/workflows/ci.yml`、
  `README.md`、`todo.md`、`cross-review/cycle-001/round-5.md`。
- ローカル cargo registry:
  `gpui-0.2.2/{Cargo.toml,Cargo.toml.orig,build.rs,README.md}`、
  `gpui-kit-0.9.1/Cargo.toml`、
  `gpui-pre-0.3.12/Cargo.toml`、
  `gpui-pre-platform-0.3.12/Cargo.toml`、
  `embed-resource-3.0.12/src/windows_msvc.rs`。
- 共通ワークフロー:
  <https://raw.githubusercontent.com/Seduce0068/workflows/v1/.github/workflows/rust-gates.yml>
  （2026-11-18 取得）。
- runner イメージ（要確認）:
  <https://github.com/actions/runner-images>。

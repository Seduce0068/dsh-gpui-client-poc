#!/usr/bin/env bash
# GUI デモを起動し、spectacle で実画面の証跡 PNG を取得する。
# 成功判定はログの grep ではなく、取得した PNG を DSH の read_image で
# 目視確認して行う（Runtime Proof）。ローカルディスプレイ必須・CI 非対象。
set -euo pipefail

# 証跡の出力先。SMOKE_EVIDENCE_DIR で上書き可能。
EVIDENCE_DIR="${SMOKE_EVIDENCE_DIR:-$(mktemp -d /tmp/dsh-gpui-smoke.XXXXXX)}"
mkdir -p "${EVIDENCE_DIR}"

# ウィンドウが描画されるまでの待ち秒数（環境に応じて調整可）。
RENDER_WAIT_SEC="${RENDER_WAIT_SEC:-5}"
# spectacle 撮影のタイムアウト秒数。
CAPTURE_TIMEOUT_SEC="${CAPTURE_TIMEOUT_SEC:-15}"

BINS=(
	cordis_todo_ui
	cordis_fs_ui
	cordis_agent_ui
	cordis_prompt_ui
	cordis_prompt_agent_ui
	cordis_composition_ui
	cordis_composition_toggle_ui
	cordis_entries_toggle_ui
	cordis_permission_ui
	cordis_retry_ui
	cordis_title_ui
	acp_stream_ui
)

# --- 事前確認（失敗したら即終了） ---
if ! command -v spectacle >/dev/null 2>&1; then
	echo "FAIL: spectacle が見つかりません。KDE の spectacle を導入してください。" >&2
	exit 1
fi

for bin in "${BINS[@]}"; do
	if [[ ! -x "./target/debug/${bin}" ]]; then
		echo "FAIL: ./target/debug/${bin} がありません。" >&2
		echo "      先に cargo build --bins を実行してください。" >&2
		exit 1
	fi
done

app_pid=""

stop_app() {
	# 撮影後にアプリを停止し、残存プロセスを残さない。
	# 終了コードは kill による想定終了のため成功判定に使わない。
	if [[ -n "${app_pid}" ]]; then
		if kill -0 "${app_pid}" 2>/dev/null; then
			if ! kill "${app_pid}" 2>/dev/null; then
				:
			fi
		fi
		if ! wait "${app_pid}" 2>/dev/null; then
			:
		fi
		app_pid=""
	fi
}

trap stop_app EXIT

for bin in "${BINS[@]}"; do
	echo "=== ${bin} ==="
	log="${EVIDENCE_DIR}/${bin}.log"
	png="${EVIDENCE_DIR}/${bin}.png"

	# ログは失敗時の診断用のみ。成功判定には使わない。
	"./target/debug/${bin}" >"${log}" 2>&1 &
	app_pid=$!

	# ウィンドウが描画されるまで待つ。
	sleep "${RENDER_WAIT_SEC}"

	if ! kill -0 "${app_pid}" 2>/dev/null; then
		echo "FAIL: ${bin} が撮影前に終了しました。ログ:" >&2
		cat "${log}" >&2
		exit 1
	fi

	# 全画面 PNG を取得。-b(背景) -n(通知なし) -o(出力先)。
	if ! timeout "${CAPTURE_TIMEOUT_SEC}" spectacle -b -n -o "${png}"; then
		echo "FAIL: spectacle の撮影に失敗しました: ${bin}" >&2
		exit 1
	fi

	# 空ファイル・非 PNG を弾く（形式チェックであり描画確認ではない）。
	if [[ ! -s "${png}" ]] ||
		[[ "$(file --mime-type -b "${png}")" != "image/png" ]]; then
		echo "FAIL: 有効な PNG が得られませんでした: ${png}" >&2
		exit 1
	fi

	stop_app

	echo "証跡: ${png}"
	echo "  → DSH の read_image ツールで開き、実描画を確認してください"
done

echo "全 ${#BINS[@]} 件の証跡を ${EVIDENCE_DIR} に取得しました。"
echo "成功判定は各 PNG の read_image 確認結果です。"
echo "ログや終了コード単体では成功と判定しないでください。"

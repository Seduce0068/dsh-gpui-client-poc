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
# アプリ停止時の待ち上限秒数（超過で SIGKILL し、明示的な FAIL とする）。
# ポーリング回数を $(( 秒 * 10 )) で計算するため整数のみ受け付ける。
STOP_WAIT_SEC="${STOP_WAIT_SEC:-5}"
if ! [[ "${STOP_WAIT_SEC}" =~ ^[0-9]+$ ]]; then
	echo "FAIL: STOP_WAIT_SEC は整数秒で指定してください: ${STOP_WAIT_SEC}" >&2
	exit 1
fi

BINS=(
	cordis_todo_ui
	cordis_fs_ui
	cordis_agent_ui
	cordis_prompt_ui
	cordis_prompt_agent_ui
	cordis_composition_toggle_ui
	cordis_permission_ui
	cordis_retry_ui
	cordis_title_ui
)

# Intentionally excluded (swarm review R5-VU02): these need external state and
# would abort the whole script on `set -e` in a clean environment:
#   - cordis_entries_toggle_ui: requires DEEPSEEK_API_KEY / --api-key
#   - cordis_composition_ui: DEEPSEEK_API_KEY / --api-key が必要、手動起動
#   - acp_stream_ui: requires an absolute-path `dsh` binary (DSH_BIN)
# Run them manually when those prerequisites are available.

# --- 事前確認（失敗したら即終了） ---
if ! command -v spectacle >/dev/null 2>&1; then
	echo "FAIL: spectacle が見つかりません。KDE の spectacle を導入してください。" >&2
	exit 1
fi

# file は証跡の形式チェックに、timeout は撮影の時間制限に使うため必須条件。
# 不足していれば撮影前に明示終了し、後続の判定に流れないようにする。
for required_cmd in file timeout; do
	if ! command -v "${required_cmd}" >/dev/null 2>&1; then
		echo "FAIL: ${required_cmd} が見つかりません。必要条件です。" >&2
		exit 1
	fi
done

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
	if [[ -z "${app_pid}" ]]; then
		return 0
	fi

	# EXIT trap からの再入でも二重に stop しないよう、pid を退避して先にクリアする。
	local pid="${app_pid}"
	app_pid=""

	if kill -0 "${pid}" 2>/dev/null; then
		kill -TERM "${pid}" 2>/dev/null || :
	fi

	# wait を無期限に呼ばない。0.1 秒間隔で kill -0 をポーリングし、
	# STOP_WAIT_SEC を過ぎたら SIGKILL してから wait する（EXIT trap でもハングしない）。
	local ticks=0
	local max_ticks=$(( STOP_WAIT_SEC * 10 ))
	while kill -0 "${pid}" 2>/dev/null && (( ticks < max_ticks )); do
		sleep 0.1
		ticks=$((ticks + 1))
	done

	if kill -0 "${pid}" 2>/dev/null; then
		# 期限切れ: 強制終了のうえ、明示的な FAIL として扱う。
		kill -KILL "${pid}" 2>/dev/null || :
		wait "${pid}" 2>/dev/null || :
		echo "FAIL: PID ${pid} が ${STOP_WAIT_SEC} 秒以内に終了しませんでした (SIGKILL で強制終了)。" >&2
		return 1
	fi

	wait "${pid}" 2>/dev/null || :
	return 0
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

	# 前回の証跡が今回の成功判定に流用されないよう、撮影直前に旧 PNG を削除する。
	rm -f -- "${png}"

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

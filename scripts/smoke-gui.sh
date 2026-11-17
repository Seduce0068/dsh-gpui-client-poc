#!/usr/bin/env bash
# Launch each GPUI demo with a timeout and assert that it created a window.
# Requires a local display (X11/Wayland). Not part of CI.
set -euo pipefail

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

for bin in "${BINS[@]}"; do
	echo "=== ${bin} ==="
	log="$(mktemp)"
	# A GPUI window keeps the process alive; a timeout exit is expected.
	timeout 20 "./target/debug/${bin}" >"${log}" 2>&1 || true
	if ! grep -q 'GPUI window created' "${log}"; then
		echo "FAIL: ${bin} did not create a window"
		tail -n 20 "${log}"
		rm -f "${log}"
		exit 1
	fi
	echo "ok: ${bin}"
	rm -f "${log}"
done

echo "all ${#BINS[@]} GPUI demos created a window"

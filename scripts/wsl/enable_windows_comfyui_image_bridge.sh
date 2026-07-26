#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

COMFYUI_ROOT="${BENSHU_WINDOWS_COMFYUI_ROOT:-}"
CHECKPOINT_NAME="${BENSHU_WINDOWS_COMFYUI_CHECKPOINT:-}"
BRIDGE_PORT="${BENSHU_WINDOWS_IMAGE_BRIDGE_PORT:-8022}"
BRIDGE_HOST="${BENSHU_WINDOWS_IMAGE_BRIDGE_HOST:-$(ip route | awk '/default/ {print $3; exit}')}"
MODEL_ALIAS="${BENSHU_WINDOWS_IMAGE_BRIDGE_MODEL:-local-image-model}"
PYTHON_EXE="${BENSHU_WINDOWS_COMFYUI_PYTHON_EXE:-}"

if [[ -z "${COMFYUI_ROOT}" ]]; then
  echo "BENSHU_WINDOWS_COMFYUI_ROOT is required."
  exit 1
fi

if [[ -z "${CHECKPOINT_NAME}" ]]; then
  echo "BENSHU_WINDOWS_COMFYUI_CHECKPOINT is required."
  exit 1
fi

to_windows_path() {
  local path="$1"
  if [[ -z "${path}" ]]; then
    return 0
  fi
  if [[ "${path}" = /* ]]; then
    wslpath -w "${path}"
  else
    printf '%s\n' "${path}"
  fi
}

WIN_COMFYUI_ROOT="$(to_windows_path "${COMFYUI_ROOT}")"
WIN_PYTHON_EXE="$(to_windows_path "${PYTHON_EXE}")"
WIN_START_SCRIPT="$(wslpath -w "${REPO_ROOT}/scripts/windows/start_comfyui_directml_bridge.ps1")"

POWERSHELL_ARGS=(
  -NoLogo
  -NoProfile
  -ExecutionPolicy Bypass
  -File "${WIN_START_SCRIPT}"
  -ComfyUiRoot "${WIN_COMFYUI_ROOT}"
  -CheckpointName "${CHECKPOINT_NAME}"
  -ComfyUiPort "8188"
  -BridgePort "${BRIDGE_PORT}"
  -BridgeHost "0.0.0.0"
  -ModelAlias "${MODEL_ALIAS}"
)

if [[ -n "${WIN_PYTHON_EXE}" ]]; then
  POWERSHELL_ARGS+=(-PythonExe "${WIN_PYTHON_EXE}")
fi

powershell.exe "${POWERSHELL_ARGS[@]}"

export BENSHU_WINDOWS_IMAGE_BRIDGE_BASE_URL="http://${BRIDGE_HOST}:${BRIDGE_PORT}/v1"
export BENSHU_WINDOWS_IMAGE_BRIDGE_MODEL="${MODEL_ALIAS}"

"${REPO_ROOT}/scripts/wsl/enable_windows_image_bridge.sh"

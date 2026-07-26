#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
WINDOWS_SYNC_ROOT="${BENSHU_WINDOWS_SYNC_ROOT:-/mnt/d/benshu}"

MODEL_DIR="${BENSHU_WINDOWS_DIFFUSION_MODEL_DIR:-${REPO_ROOT}/models/live/image-model}"
PYTHON_EXE="${BENSHU_WINDOWS_DIFFUSION_PYTHON_EXE:-D:\\benshu\\windows-diffusion-venv\\Scripts\\python.exe}"
BRIDGE_PORT="${BENSHU_WINDOWS_IMAGE_BRIDGE_PORT:-8022}"
BRIDGE_HOST="${BENSHU_WINDOWS_IMAGE_BRIDGE_HOST:-$(ip route | awk '/default/ {print $3; exit}')}"
MODEL_ALIAS="${BENSHU_WINDOWS_IMAGE_BRIDGE_MODEL:-local-image-model}"
NUM_STEPS="${BENSHU_WINDOWS_DIFFUSION_STEPS:-20}"
GUIDANCE_SCALE="${BENSHU_WINDOWS_DIFFUSION_GUIDANCE_SCALE:-7.5}"
DTYPE="${BENSHU_WINDOWS_DIFFUSION_DTYPE:-float16}"

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

WIN_MODEL_DIR="$(to_windows_path "${MODEL_DIR}")"

mkdir -p "${WINDOWS_SYNC_ROOT}/scripts/windows"
cp "${REPO_ROOT}/scripts/windows/directml_diffusers_image_service.py" "${WINDOWS_SYNC_ROOT}/scripts/windows/"
cp "${REPO_ROOT}/scripts/windows/start_directml_diffusion_bridge.ps1" "${WINDOWS_SYNC_ROOT}/scripts/windows/"
cp "${REPO_ROOT}/scripts/windows/stop_image_bridge_service.ps1" "${WINDOWS_SYNC_ROOT}/scripts/windows/"

WIN_START_SCRIPT="$(wslpath -w "${WINDOWS_SYNC_ROOT}/scripts/windows/start_directml_diffusion_bridge.ps1")"

powershell.exe \
  -NoLogo \
  -NoProfile \
  -ExecutionPolicy Bypass \
  -File "${WIN_START_SCRIPT}" \
  -PythonExe "${PYTHON_EXE}" \
  -ModelDir "${WIN_MODEL_DIR}" \
  -ModelAlias "${MODEL_ALIAS}" \
  -ListenHost "0.0.0.0" \
  -Port "${BRIDGE_PORT}" \
  -NumSteps "${NUM_STEPS}" \
  -GuidanceScale "${GUIDANCE_SCALE}" \
  -DType "${DTYPE}"

export BENSHU_WINDOWS_IMAGE_BRIDGE_BASE_URL="http://${BRIDGE_HOST}:${BRIDGE_PORT}/v1"
export BENSHU_WINDOWS_IMAGE_BRIDGE_MODEL="${MODEL_ALIAS}"

"${REPO_ROOT}/scripts/wsl/enable_windows_image_bridge.sh"

#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CONFIG_FILE="${REPO_ROOT}/data/benshu.yaml"
BACKUP_FILE="${CONFIG_FILE}.windows-image-bridge.bak"
WIN_STOP_SCRIPT="$(wslpath -w "${REPO_ROOT}/scripts/windows/stop_image_bridge_service.ps1")"

if [[ -f "${BACKUP_FILE}" ]]; then
  cp "${BACKUP_FILE}" "${CONFIG_FILE}"
  rm -f "${BACKUP_FILE}"
  echo "Restored ${CONFIG_FILE} from image bridge backup."
else
  echo "No image bridge backup found; leaving config untouched."
fi

powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "${WIN_STOP_SCRIPT}"

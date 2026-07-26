#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
AGENT_FILE="${REPO_ROOT}/data/agents/benshu/AGENT.md"
BACKUP_FILE="${AGENT_FILE}.windows-bridge.bak"
WIN_STOP_SCRIPT="$(wslpath -w "${REPO_ROOT}/scripts/windows/stop_llama_server_vulkan.ps1")"

if [[ -f "${BACKUP_FILE}" ]]; then
  cp "${BACKUP_FILE}" "${AGENT_FILE}"
  rm -f "${BACKUP_FILE}"
  echo "Restored ${AGENT_FILE} from bridge backup."
else
  echo "No bridge backup found; leaving AGENT.md untouched."
fi

powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "${WIN_STOP_SCRIPT}"

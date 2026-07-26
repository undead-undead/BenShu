#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CONFIG_FILE="${REPO_ROOT}/data/benshu.yaml"
BACKUP_FILE="${CONFIG_FILE}.windows-image-bridge.bak"

BRIDGE_HOST="${BENSHU_WINDOWS_IMAGE_BRIDGE_HOST:-$(ip route | awk '/default/ {print $3; exit}')}"
BRIDGE_PORT="${BENSHU_WINDOWS_IMAGE_BRIDGE_PORT:-8022}"
BRIDGE_MODEL="${BENSHU_WINDOWS_IMAGE_BRIDGE_MODEL:-local-image-model}"
BRIDGE_BASE_URL="${BENSHU_WINDOWS_IMAGE_BRIDGE_BASE_URL:-http://${BRIDGE_HOST}:${BRIDGE_PORT}/v1}"
GATEWAY_URL="${BENSHU_GATEWAY_URL:-http://127.0.0.1:3000}"
API_KEY="${BENSHU_SESSION_TOKEN:-${BENSHU_API_KEY:-}}"
ALLOW_CONFIG_FILE_WRITE="${BENSHU_ALLOW_IMAGE_BRIDGE_CONFIG_FILE_WRITE:-0}"
WINDOWS_COMMAND="${BENSHU_WINDOWS_IMAGE_BRIDGE_COMMAND:-}"
WINDOWS_ARGS="${BENSHU_WINDOWS_IMAGE_BRIDGE_ARGS:-}"
WINDOWS_WORKDIR="${BENSHU_WINDOWS_IMAGE_BRIDGE_WORKDIR:-}"
WINDOWS_HEALTH_URL="${BENSHU_WINDOWS_IMAGE_BRIDGE_HEALTH_URL:-${BRIDGE_BASE_URL%/v1}/health}"
WIN_START_SCRIPT="$(wslpath -w "${REPO_ROOT}/scripts/windows/start_image_bridge_service.ps1")"

BRIDGE_SPEC="bridge-image:${BRIDGE_BASE_URL}|${BRIDGE_MODEL}"

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

update_bridge_config_via_api() {
  python3 - <<'PY' "${GATEWAY_URL}" "${API_KEY}" "${BRIDGE_SPEC}"
import json
import sys
import urllib.error
import urllib.request

gateway_url, api_key, bridge_spec = sys.argv[1:4]
base = gateway_url.rstrip("/")

req = urllib.request.Request(
    f"{base}/api/config",
    headers={"X-API-Key": api_key},
)
with urllib.request.urlopen(req, timeout=10) as resp:
    payload = json.load(resp)

payload.setdefault("sensory", {})
payload["sensory"]["image_gen_model"] = bridge_spec
body = json.dumps(payload).encode("utf-8")

save_req = urllib.request.Request(
    f"{base}/api/config",
    data=body,
    method="POST",
    headers={
        "Content-Type": "application/json",
        "X-API-Key": api_key,
    },
)
with urllib.request.urlopen(save_req, timeout=10) as resp:
    if resp.status < 200 or resp.status >= 300:
        raise SystemExit(f"config update failed with HTTP {resp.status}")
PY
}

update_bridge_config_via_file() {
  if [[ ! -f "${CONFIG_FILE}" ]]; then
    echo "Config not found: ${CONFIG_FILE}"
    exit 1
  fi

  if [[ ! -f "${BACKUP_FILE}" ]]; then
    cp "${CONFIG_FILE}" "${BACKUP_FILE}"
  fi

  python3 - <<'PY' "${CONFIG_FILE}" "${BRIDGE_SPEC}"
import sys
from pathlib import Path

config_path = Path(sys.argv[1])
bridge_spec = sys.argv[2]
text = config_path.read_text(encoding="utf-8")
lines = text.splitlines()

for i, line in enumerate(lines):
    if line.lstrip().startswith("image_gen_model:"):
        indent = line[: len(line) - len(line.lstrip())]
        lines[i] = f"{indent}image_gen_model: {bridge_spec}"
        break
else:
    raise SystemExit("image_gen_model field not found in config")

config_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY
}

if [[ -n "${WINDOWS_COMMAND}" ]]; then
  WIN_COMMAND="$(to_windows_path "${WINDOWS_COMMAND}")"
  WIN_WORKDIR="$(to_windows_path "${WINDOWS_WORKDIR}")"

  POWERSHELL_ARGS=(
    -NoLogo
    -NoProfile
    -ExecutionPolicy Bypass
    -File "${WIN_START_SCRIPT}"
    -CommandPath "${WIN_COMMAND}"
    -Arguments "${WINDOWS_ARGS}"
    -HealthUrl "${WINDOWS_HEALTH_URL}"
  )

  if [[ -n "${WIN_WORKDIR}" ]]; then
    POWERSHELL_ARGS+=(-WorkingDirectory "${WIN_WORKDIR}")
  fi

  powershell.exe "${POWERSHELL_ARGS[@]}"
fi

CONFIG_UPDATE_MODE="manual"
if [[ -n "${API_KEY}" ]]; then
  if update_bridge_config_via_api; then
    CONFIG_UPDATE_MODE="api"
  fi
fi

if [[ "${CONFIG_UPDATE_MODE}" = "manual" && "${ALLOW_CONFIG_FILE_WRITE}" = "1" ]]; then
  update_bridge_config_via_file
  CONFIG_UPDATE_MODE="file"
fi

echo "Windows image bridge enabled."
echo "sensory.image_gen_model -> ${BRIDGE_SPEC}"
echo "If your Windows image service requires auth, export BENSHU_IMAGE_BRIDGE_API_KEY in WSL before starting gateway."
if [[ "${CONFIG_UPDATE_MODE}" = "api" ]]; then
  echo "Applied through gateway /api/config (panel-compatible flow)."
elif [[ "${CONFIG_UPDATE_MODE}" = "file" ]]; then
  echo "Applied through direct config-file write because BENSHU_ALLOW_IMAGE_BRIDGE_CONFIG_FILE_WRITE=1."
else
  echo "Bridge is ready, but config was not changed automatically."
  echo "Set it from the panel or call POST ${GATEWAY_URL}/api/config with sensory.image_gen_model=${BRIDGE_SPEC}."
fi
if [[ -z "${WINDOWS_COMMAND}" ]]; then
  echo "No Windows service command was started. Set BENSHU_WINDOWS_IMAGE_BRIDGE_COMMAND if you want this script to launch the Windows image server too."
fi

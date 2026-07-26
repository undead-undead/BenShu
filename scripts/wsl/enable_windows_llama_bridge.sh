#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CONFIG_FILE="${REPO_ROOT}/data/benshu.yaml"

yaml_get() {
  local key="$1"
  local default_value="${2:-}"
  python3 - "$CONFIG_FILE" "$key" "$default_value" <<'PY'
import sys
from pathlib import Path

try:
    import yaml
except Exception:
    print(sys.argv[3], end="")
    raise SystemExit(0)

config_path = Path(sys.argv[1])
key = sys.argv[2]
default = sys.argv[3]
if not config_path.exists():
    print(default, end="")
    raise SystemExit(0)

with config_path.open("r", encoding="utf-8") as fh:
    data = yaml.safe_load(fh) or {}

node = data
for part in key.split("."):
    if isinstance(node, dict) and part in node:
        node = node[part]
    else:
        print(default, end="")
        raise SystemExit(0)

if node is None:
    print(default, end="")
elif isinstance(node, bool):
    print("true" if node else "false", end="")
else:
    print(node, end="")
PY
}

yaml_parse_url_port() {
  local url="$1"
  local default_value="${2:-}"
  python3 - "$url" "$default_value" <<'PY'
import sys
from urllib.parse import urlparse

raw = sys.argv[1]
default = sys.argv[2]
if not raw:
    print(default, end="")
    raise SystemExit(0)

try:
    parsed = urlparse(raw)
    port = parsed.port
except Exception:
    port = None

print(port if port is not None else default, end="")
PY
}

resolve_setting() {
  local env_name="$1"
  local yaml_key="$2"
  local fallback="$3"
  local env_value="${!env_name-}"
  if [[ -n "${env_value}" ]]; then
    printf '%s\n' "${env_value}"
  else
    yaml_get "${yaml_key}" "${fallback}"
  fi
}

configured_agent_value() {
  local field="$1"
  yaml_get "agents.${AGENT_ROLE}.${field}" ""
}

looks_like_model_path() {
  local path="$1"
  [[ -n "${path}" && ( "${path}" = /* || "${path}" = /mnt/* || "${path}" =~ ^[A-Za-z]:\\\\ ) ]]
}

AGENT_ROLE="${BENSHU_WINDOWS_LLAMA_AGENT_ROLE:-benshu}"
CONFIGURED_AGENT_BASE_URL="$(configured_agent_value base_url)"
if [[ -n "${BENSHU_WINDOWS_LLAMA_PORT-}" ]]; then
  PORT="${BENSHU_WINDOWS_LLAMA_PORT}"
elif [[ -n "${CONFIGURED_AGENT_BASE_URL}" ]]; then
  PORT="$(yaml_parse_url_port "${CONFIGURED_AGENT_BASE_URL}" 8012)"
else
  PORT="$(resolve_setting BENSHU_WINDOWS_LLAMA_PORT windows_ml_bridge.port 8012)"
fi
CTX_SIZE="$(resolve_setting BENSHU_WINDOWS_LLAMA_CTX_SIZE llama_cpp_runtime.ctx_size 8192)"
GPU_LAYERS="$(resolve_setting BENSHU_WINDOWS_LLAMA_GPU_LAYERS llama_cpp_runtime.gpu_layers 24)"
THREADS="$(resolve_setting BENSHU_WINDOWS_LLAMA_THREADS llama_cpp_runtime.threads -1)"
THREADS_BATCH="$(resolve_setting BENSHU_WINDOWS_LLAMA_THREADS_BATCH llama_cpp_runtime.threads_batch '')"
BATCH_SIZE="$(resolve_setting BENSHU_WINDOWS_LLAMA_BATCH_SIZE llama_cpp_runtime.batch_size 2048)"
UBATCH_SIZE="$(resolve_setting BENSHU_WINDOWS_LLAMA_UBATCH_SIZE llama_cpp_runtime.ubatch_size 512)"
PARALLEL_SLOTS="$(resolve_setting BENSHU_WINDOWS_LLAMA_PARALLEL_SLOTS llama_cpp_runtime.parallel_slots 1)"
CACHE_RAM="$(resolve_setting BENSHU_WINDOWS_LLAMA_CACHE_RAM llama_cpp_runtime.cache_ram 256)"
CTX_CHECKPOINTS="$(resolve_setting BENSHU_WINDOWS_LLAMA_CTX_CHECKPOINTS llama_cpp_runtime.ctx_checkpoints 0)"
FLASH_ATTN_MODE="$(resolve_setting BENSHU_WINDOWS_LLAMA_FLASH_ATTN_MODE llama_cpp_runtime.flash_attn_mode auto)"
KV_OFFLOAD="$(resolve_setting BENSHU_WINDOWS_LLAMA_KV_OFFLOAD llama_cpp_runtime.kv_offload true)"
MMAP_MODE="$(resolve_setting BENSHU_WINDOWS_LLAMA_MMAP llama_cpp_runtime.mmap true)"
MLOCK_MODE="$(resolve_setting BENSHU_WINDOWS_LLAMA_MLOCK llama_cpp_runtime.mlock false)"
CACHE_PROMPT="$(resolve_setting BENSHU_WINDOWS_LLAMA_CACHE_PROMPT llama_cpp_runtime.cache_prompt false)"
CONT_BATCHING="$(resolve_setting BENSHU_WINDOWS_LLAMA_CONT_BATCHING llama_cpp_runtime.cont_batching false)"
WARMUP_MODE="$(resolve_setting BENSHU_WINDOWS_LLAMA_WARMUP llama_cpp_runtime.warmup true)"
CONTEXT_SHIFT="$(resolve_setting BENSHU_WINDOWS_LLAMA_CONTEXT_SHIFT llama_cpp_runtime.context_shift false)"
JINJA_MODE="$(resolve_setting BENSHU_WINDOWS_LLAMA_JINJA llama_cpp_runtime.jinja true)"
ROPE_SCALING="$(resolve_setting BENSHU_WINDOWS_LLAMA_ROPE_SCALING llama_cpp_runtime.rope_scaling '')"
ROPE_SCALE="$(resolve_setting BENSHU_WINDOWS_LLAMA_ROPE_SCALE llama_cpp_runtime.rope_scale '')"
ROPE_FREQ_BASE="$(resolve_setting BENSHU_WINDOWS_LLAMA_ROPE_FREQ_BASE llama_cpp_runtime.rope_freq_base '')"
ROPE_FREQ_SCALE="$(resolve_setting BENSHU_WINDOWS_LLAMA_ROPE_FREQ_SCALE llama_cpp_runtime.rope_freq_scale '')"
YARN_ORIG_CTX="$(resolve_setting BENSHU_WINDOWS_LLAMA_YARN_ORIG_CTX llama_cpp_runtime.yarn_orig_ctx '')"
YARN_EXT_FACTOR="$(resolve_setting BENSHU_WINDOWS_LLAMA_YARN_EXT_FACTOR llama_cpp_runtime.yarn_ext_factor '')"
YARN_ATTN_FACTOR="$(resolve_setting BENSHU_WINDOWS_LLAMA_YARN_ATTN_FACTOR llama_cpp_runtime.yarn_attn_factor '')"
YARN_BETA_SLOW="$(resolve_setting BENSHU_WINDOWS_LLAMA_YARN_BETA_SLOW llama_cpp_runtime.yarn_beta_slow '')"
YARN_BETA_FAST="$(resolve_setting BENSHU_WINDOWS_LLAMA_YARN_BETA_FAST llama_cpp_runtime.yarn_beta_fast '')"
CACHE_TYPE_K="$(resolve_setting BENSHU_WINDOWS_LLAMA_CACHE_TYPE_K llama_cpp_runtime.cache_type_k '')"
CACHE_TYPE_V="$(resolve_setting BENSHU_WINDOWS_LLAMA_CACHE_TYPE_V llama_cpp_runtime.cache_type_v '')"
DEVICE="$(resolve_setting BENSHU_WINDOWS_LLAMA_DEVICE llama_cpp_runtime.device '')"
SPLIT_MODE="$(resolve_setting BENSHU_WINDOWS_LLAMA_SPLIT_MODE llama_cpp_runtime.split_mode '')"
TENSOR_SPLIT="$(resolve_setting BENSHU_WINDOWS_LLAMA_TENSOR_SPLIT llama_cpp_runtime.tensor_split '')"
MAIN_GPU="$(resolve_setting BENSHU_WINDOWS_LLAMA_MAIN_GPU llama_cpp_runtime.main_gpu '')"
FIT_MODE="$(resolve_setting BENSHU_WINDOWS_LLAMA_FIT_MODE llama_cpp_runtime.fit_mode on)"
FIT_TARGET="$(resolve_setting BENSHU_WINDOWS_LLAMA_FIT_TARGET llama_cpp_runtime.fit_target '')"
FIT_CTX="$(resolve_setting BENSHU_WINDOWS_LLAMA_FIT_CTX llama_cpp_runtime.fit_ctx '')"
CPU_MOE="$(resolve_setting BENSHU_WINDOWS_LLAMA_CPU_MOE llama_cpp_runtime.cpu_moe false)"
N_CPU_MOE="$(resolve_setting BENSHU_WINDOWS_LLAMA_N_CPU_MOE llama_cpp_runtime.n_cpu_moe '')"
MMPROJ_OFFLOAD="$(resolve_setting BENSHU_WINDOWS_LLAMA_MMPROJ_OFFLOAD llama_cpp_runtime.mmproj_offload true)"
IMAGE_MIN_TOKENS="$(resolve_setting BENSHU_WINDOWS_LLAMA_IMAGE_MIN_TOKENS llama_cpp_runtime.image_min_tokens '')"
IMAGE_MAX_TOKENS="$(resolve_setting BENSHU_WINDOWS_LLAMA_IMAGE_MAX_TOKENS llama_cpp_runtime.image_max_tokens '')"
REASONING_MODE="$(resolve_setting BENSHU_WINDOWS_LLAMA_REASONING_MODE llama_cpp_runtime.reasoning_mode auto)"
REASONING_FORMAT="$(resolve_setting BENSHU_WINDOWS_LLAMA_REASONING_FORMAT llama_cpp_runtime.reasoning_format auto)"
REASONING_BUDGET="$(resolve_setting BENSHU_WINDOWS_LLAMA_REASONING_BUDGET llama_cpp_runtime.reasoning_budget '')"
REASONING_BUDGET_MESSAGE="$(resolve_setting BENSHU_WINDOWS_LLAMA_REASONING_BUDGET_MESSAGE llama_cpp_runtime.reasoning_budget_message '')"
SAMPLING_TEMPERATURE="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_TEMPERATURE llama_cpp_runtime.sampling_temperature 0.8)"
SAMPLING_TOP_K="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_TOP_K llama_cpp_runtime.sampling_top_k 40)"
SAMPLING_TOP_P="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_TOP_P llama_cpp_runtime.sampling_top_p 0.95)"
SAMPLING_MIN_P="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_MIN_P llama_cpp_runtime.sampling_min_p 0.05)"
SAMPLING_TYPICAL_P="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_TYPICAL_P llama_cpp_runtime.sampling_typical_p 1.0)"
SAMPLING_REPEAT_PENALTY="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_REPEAT_PENALTY llama_cpp_runtime.sampling_repeat_penalty 1.0)"
SAMPLING_PRESENCE_PENALTY="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_PRESENCE_PENALTY llama_cpp_runtime.sampling_presence_penalty 0.0)"
SAMPLING_FREQUENCY_PENALTY="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_FREQUENCY_PENALTY llama_cpp_runtime.sampling_frequency_penalty 0.0)"
SAMPLING_MIROSTAT="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_MIROSTAT llama_cpp_runtime.sampling_mirostat 0)"
SAMPLING_MIROSTAT_ETA="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_MIROSTAT_ETA llama_cpp_runtime.sampling_mirostat_eta 0.1)"
SAMPLING_MIROSTAT_TAU="$(resolve_setting BENSHU_WINDOWS_LLAMA_SAMPLING_MIROSTAT_TAU llama_cpp_runtime.sampling_mirostat_tau 5.0)"
SEED="$(resolve_setting BENSHU_WINDOWS_LLAMA_SEED llama_cpp_runtime.seed '')"
CONFIGURED_AGENT_MODEL="$(configured_agent_value model)"
CONFIGURED_LOCAL_MODEL_ARTIFACT="$(configured_agent_value local_model_artifact)"
CONFIGURED_LOCAL_MMPROJ_ARTIFACT="$(configured_agent_value local_mmproj_artifact)"
CONFIGURED_LOCAL_RUNTIME_FAMILY="$(configured_agent_value local_runtime_family)"
if [[ -n "${BENSHU_WINDOWS_LLAMA_MODEL_ALIAS-}" ]]; then
  ALIAS="${BENSHU_WINDOWS_LLAMA_MODEL_ALIAS}"
elif [[ -n "${CONFIGURED_AGENT_MODEL}" ]]; then
  ALIAS="${CONFIGURED_AGENT_MODEL}"
else
  ALIAS="benshu-main-brain"
fi
API_KEY="${BENSHU_WINDOWS_LLAMA_API_KEY:-sk-local-llama-key}"
READY_TIMEOUT_SECS="${BENSHU_WINDOWS_LLAMA_READY_TIMEOUT_SECS:-240}"
PORT_RETRY_COUNT="${BENSHU_WINDOWS_LLAMA_PORT_RETRY_COUNT:-0}"
SERVER_EXE="${BENSHU_WINDOWS_LLAMA_SERVER_EXE:-}"
if [[ -n "${BENSHU_WINDOWS_LLAMA_MODEL_PATH-}" ]]; then
  MODEL_PATH="${BENSHU_WINDOWS_LLAMA_MODEL_PATH}"
elif [[ -n "${CONFIGURED_LOCAL_MODEL_ARTIFACT}" ]]; then
  MODEL_PATH="${CONFIGURED_LOCAL_MODEL_ARTIFACT}"
elif looks_like_model_path "${CONFIGURED_AGENT_MODEL}"; then
  MODEL_PATH="${CONFIGURED_AGENT_MODEL}"
else
  MODEL_PATH="/home/biubiuboy/BenShu/models/live/gemma4-e4b-q2k/google_gemma-4-E4B-it-Q2_K_L.gguf"
fi
if [[ -n "${BENSHU_WINDOWS_LLAMA_MMPROJ_PATH-}" ]]; then
  MMPROJ_PATH="${BENSHU_WINDOWS_LLAMA_MMPROJ_PATH}"
else
  MMPROJ_PATH="${CONFIGURED_LOCAL_MMPROJ_ARTIFACT}"
fi
MEDIA_PATH="${BENSHU_WINDOWS_LLAMA_MEDIA_PATH:-}"
WINDOWS_BRIDGE_HOST="${BENSHU_WINDOWS_LLAMA_BRIDGE_HOST:-$(ip route | awk '/default/ {print $3; exit}')}"
WINDOWS_SERVER_BIND_HOST="${BENSHU_WINDOWS_LLAMA_BIND_HOST:-${WINDOWS_BRIDGE_HOST}}"
STAGE_MODE="${BENSHU_WINDOWS_LLAMA_STAGE_MODE:-auto}"
STAGE_ROOT="${BENSHU_WINDOWS_LLAMA_STAGE_ROOT:-}"
update_bridge_base_url() {
  BRIDGE_BASE_URL="http://${WINDOWS_BRIDGE_HOST}:${PORT}"
}
update_bridge_base_url

is_wsl_local_linux_path() {
  local path="$1"
  [[ "${path}" = /* && "${path}" != /mnt/* ]]
}

default_stage_root() {
  if [[ -d /mnt/d ]]; then
    printf '%s\n' "/mnt/d/benshu-model-cache"
  else
    printf '%s\n' "/mnt/c/Users/Public/benshu-model-cache"
  fi
}

sanitize_stage_key() {
  local raw="$1"
  raw="${raw//\//__}"
  raw="${raw// /_}"
  printf '%s\n' "${raw}"
}

sync_stage_file() {
  local src="$1"
  local dst="$2"

  mkdir -p "$(dirname "${dst}")"

  if [[ -f "${dst}" ]]; then
    local src_size dst_size src_mtime dst_mtime
    src_size="$(stat -c '%s' "${src}")"
    dst_size="$(stat -c '%s' "${dst}")"
    src_mtime="$(stat -c '%Y' "${src}")"
    dst_mtime="$(stat -c '%Y' "${dst}")"
    if [[ "${src_size}" = "${dst_size}" && "${src_mtime}" = "${dst_mtime}" ]]; then
      return 0
    fi
  fi

  cp -f "${src}" "${dst}"
  touch -r "${src}" "${dst}"
}

stage_windows_local_asset() {
  local src="$1"
  local label="$2"
  local root="$3"

  if [[ -z "${src}" ]]; then
    return 0
  fi

  if ! is_wsl_local_linux_path "${src}"; then
    printf '%s\n' "${src}"
    return 0
  fi

  local key target
  key="$(sanitize_stage_key "$(dirname "${src}")")"
  target="${root}/${key}/$(basename "${src}")"

  echo "Staging ${label} into Windows-local cache: ${target}" >&2
  sync_stage_file "${src}" "${target}"
  printf '%s\n' "${target}"
}

probe_health() {
  curl --connect-timeout 2 --max-time 5 -fsS "${BRIDGE_BASE_URL}/health" >/dev/null 2>&1
}

probe_models() {
  local body
  body="$(
    curl --connect-timeout 2 --max-time 8 -fsS "${BRIDGE_BASE_URL}/v1/models" 2>/dev/null || true
  )"
  [[ -n "${body}" ]] || return 1
  python3 - "${ALIAS}" "${body}" <<'PY'
import json
import sys

alias_id = sys.argv[1]
raw = sys.argv[2]
try:
    payload = json.loads(raw)
except Exception:
    raise SystemExit(1)

models = payload.get("data") or []
for model in models:
    if isinstance(model, dict) and model.get("id") == alias_id:
        raise SystemExit(0)

raise SystemExit(1)
PY
}

probe_text_ready() {
  local payload body
  payload="$(cat <<EOF
{"model":"${ALIAS}","messages":[{"role":"user","content":"Print exactly: BENSHU_READY"}],"temperature":0.0,"max_tokens":16}
EOF
)"
  body="$(
    curl -sS \
      --connect-timeout 2 \
      --max-time 15 \
      -H "Content-Type: application/json" \
      -H "Authorization: Bearer ${API_KEY}" \
      -d "${payload}" \
      "${BRIDGE_BASE_URL}/v1/chat/completions" 2>/dev/null || true
  )"

  if [[ -z "${body}" ]]; then
    return 1
  fi

  if grep -q '"message":"Loading model"' <<<"${body}"; then
    return 1
  fi

  if grep -q '"error"' <<<"${body}"; then
    echo "Bridge probe returned non-ready response: ${body}" >&2
    return 2
  fi

  set +e
  python3 - "${body}" <<'PY'
import json
import re
import sys

raw = sys.argv[1]
try:
    payload = json.loads(raw)
except Exception:
    print(f"Bridge text probe returned invalid JSON: {raw[:500]}", file=sys.stderr)
    raise SystemExit(2)

choices = payload.get("choices") or []
if not choices:
    print(f"Bridge text probe returned no choices: {raw[:500]}", file=sys.stderr)
    raise SystemExit(1)

message = choices[0].get("message") if isinstance(choices[0], dict) else None
content = ""
reasoning = ""
if isinstance(message, dict):
    content = message.get("content") or ""
    reasoning = message.get("reasoning_content") or ""
else:
    content = choices[0].get("text") or ""

visible = str(content).strip()
if "BENSHU_READY" in visible:
    raise SystemExit(0)

sample = re.sub(r"\s+", " ", visible or str(reasoning).strip())[:240]
print(
    "Bridge text probe produced unusable visible output; "
    f"expected BENSHU_READY, got: {sample!r}",
    file=sys.stderr,
)
raise SystemExit(2)
PY
  local probe_status=$?
  set -e
  case "${probe_status}" in
    0) return 0 ;;
    1) return 1 ;;
    *) return 2 ;;
  esac

  return 1
}

windows_port_is_bindable() {
  local port="$1"
  powershell.exe -NoLogo -NoProfile -Command "
\$listener = \$null
try {
  \$ip = [System.Net.IPAddress]::Parse('${WINDOWS_SERVER_BIND_HOST}')
  \$listener = [System.Net.Sockets.TcpListener]::new(\$ip, ${port})
  \$listener.Start()
  \$listener.Stop()
  exit 0
} catch {
  if (\$listener) {
    try { \$listener.Stop() } catch {}
  }
  exit 1
}" >/dev/null 2>&1
}

select_bindable_windows_port() {
  local base_port="$1"
  local candidate
  local candidates=(
    "${base_port}"
    18013
    28013
    38013
    48013
    58013
  )
  for candidate in "${candidates[@]}"; do
    if windows_port_is_bindable "${candidate}"; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done
  return 1
}

sync_bridge_runtime_config() {
  python3 - "$CONFIG_FILE" "$AGENT_ROLE" "${BRIDGE_BASE_URL}/v1" "$ALIAS" "$WINDOWS_BRIDGE_HOST" "$PORT" <<'PY'
import sys
from pathlib import Path

try:
    import yaml
except Exception as exc:
    print(f"Skipping config sync because PyYAML is unavailable: {exc}", file=sys.stderr)
    raise SystemExit(0)

config_path = Path(sys.argv[1])
agent_role = sys.argv[2]
base_url = sys.argv[3]
alias = sys.argv[4]
host = sys.argv[5]
port = int(sys.argv[6])

if not config_path.exists():
    raise SystemExit(0)

with config_path.open("r", encoding="utf-8") as fh:
    data = yaml.safe_load(fh) or {}

agents = data.setdefault("agents", {})
agent = agents.setdefault(agent_role, {})
agent["provider"] = "openai"
agent["base_url"] = base_url
agent["model"] = alias

windows_bridge = data.setdefault("windows_ml_bridge", {})
windows_bridge["base_url"] = base_url
windows_bridge["host"] = host
windows_bridge["port"] = port

with config_path.open("w", encoding="utf-8") as fh:
    yaml.safe_dump(data, fh, allow_unicode=True, sort_keys=False)
PY
}

if [[ -z "${SERVER_EXE}" ]]; then
  echo "BENSHU_WINDOWS_LLAMA_SERVER_EXE is required."
  echo "Example:"
  echo "  export BENSHU_WINDOWS_LLAMA_SERVER_EXE='C:\\\\llama.cpp\\\\build\\\\bin\\\\Release\\\\llama-server.exe'"
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

resolve_mmproj_path() {
  local model_path="$1"
  local explicit_mmproj="$2"
  if [[ -n "${explicit_mmproj}" ]]; then
    printf '%s\n' "${explicit_mmproj}"
    return 0
  fi

  if [[ "${model_path}" != /* ]]; then
    return 0
  fi

  local model_dir
  model_dir="$(dirname "${model_path}")"

  local direct_match
  direct_match="$(find "${model_dir}" -maxdepth 1 -type f -name 'mmproj-*.gguf' | head -n 1 || true)"
  if [[ -n "${direct_match}" ]]; then
    printf '%s\n' "${direct_match}"
  fi
}

apply_llama_reasoning_compatibility() {
  local model="$1"
  local lowered_model lowered_mode lowered_format adjusted
  lowered_model="$(printf '%s' "${model}" | tr '[:upper:]' '[:lower:]')"
  lowered_mode="$(printf '%s' "${REASONING_MODE}" | xargs | tr '[:upper:]' '[:lower:]')"
  lowered_format="$(printf '%s' "${REASONING_FORMAT}" | xargs | tr '[:upper:]' '[:lower:]')"
  adjusted=0

  case "${lowered_mode}" in
    off|false|none|disabled|0)
      if [[ "${REASONING_MODE}" != "off" ]]; then
        REASONING_MODE="off"
        adjusted=1
      fi
      if [[ "${REASONING_FORMAT}" != "none" ]]; then
        REASONING_FORMAT="none"
        adjusted=1
      fi
      REASONING_BUDGET=""
      REASONING_BUDGET_MESSAGE=""
      if [[ "${adjusted}" = 1 ]]; then
        echo "Applied llama.cpp reasoning-off preset: reasoning=${REASONING_MODE} reasoning-format=${REASONING_FORMAT}"
      fi
      return 0
      ;;
    true|enabled|1)
      REASONING_MODE="on"
      ;;
  esac

  if [[ "${lowered_model}" == *qwen* ]]; then
    case "${lowered_format}" in
      ""|false|off|none|disabled|0|auto)
        REASONING_FORMAT="deepseek"
        adjusted=1
        ;;
    esac
    if [[ "${adjusted}" = 1 ]]; then
      echo "Applied Qwen llama.cpp reasoning compatibility preset: reasoning=${REASONING_MODE} reasoning-format=${REASONING_FORMAT}"
    fi
  fi
}

MMPROJ_PATH="$(resolve_mmproj_path "${MODEL_PATH}" "${MMPROJ_PATH}")"

if [[ -z "${STAGE_ROOT}" ]]; then
  STAGE_ROOT="$(default_stage_root)"
fi

case "${STAGE_MODE}" in
  auto)
    if is_wsl_local_linux_path "${MODEL_PATH}" || { [[ -n "${MMPROJ_PATH}" ]] && is_wsl_local_linux_path "${MMPROJ_PATH}"; }; then
      MODEL_PATH="$(stage_windows_local_asset "${MODEL_PATH}" "model" "${STAGE_ROOT}")"
      if [[ -n "${MMPROJ_PATH}" ]]; then
        MMPROJ_PATH="$(stage_windows_local_asset "${MMPROJ_PATH}" "mmproj" "${STAGE_ROOT}")"
      fi
    fi
    ;;
  always)
    MODEL_PATH="$(stage_windows_local_asset "${MODEL_PATH}" "model" "${STAGE_ROOT}")"
    if [[ -n "${MMPROJ_PATH}" ]]; then
      MMPROJ_PATH="$(stage_windows_local_asset "${MMPROJ_PATH}" "mmproj" "${STAGE_ROOT}")"
    fi
    ;;
  never)
    ;;
  *)
    echo "Unsupported BENSHU_WINDOWS_LLAMA_STAGE_MODE: ${STAGE_MODE}"
    exit 1
    ;;
esac

WIN_SERVER_EXE="$(to_windows_path "${SERVER_EXE}")"
WIN_MODEL_PATH="$(to_windows_path "${MODEL_PATH}")"
WIN_MMPROJ_PATH="$(to_windows_path "${MMPROJ_PATH}")"
WIN_MEDIA_PATH="$(to_windows_path "${MEDIA_PATH}")"
apply_llama_reasoning_compatibility "${MODEL_PATH}"

SELECTED_PORT="$(select_bindable_windows_port "${PORT}" || true)"
if [[ -z "${SELECTED_PORT}" ]]; then
  echo "Unable to find a bindable Windows port starting from ${PORT}."
  exit 1
fi
if [[ "${SELECTED_PORT}" != "${PORT}" ]]; then
  echo "Windows port ${PORT} is not bindable right now; falling back to ${SELECTED_PORT}."
  PORT="${SELECTED_PORT}"
  update_bridge_base_url
fi

WINDOWS_SCRIPT_ROOT="${STAGE_ROOT}/scripts/windows"
WINDOWS_START_SCRIPT="${WINDOWS_SCRIPT_ROOT}/start_llama_server_vulkan.ps1"
mkdir -p "${WINDOWS_SCRIPT_ROOT}"
cp -f "${REPO_ROOT}/scripts/windows/start_llama_server_vulkan.ps1" "${WINDOWS_START_SCRIPT}"
touch -r "${REPO_ROOT}/scripts/windows/start_llama_server_vulkan.ps1" "${WINDOWS_START_SCRIPT}"
WIN_START_SCRIPT="$(wslpath -w "${WINDOWS_START_SCRIPT}")"
WINDOWS_LOG_ROOT="${STAGE_ROOT}/logs/windows-llama-bridge"
mkdir -p "${WINDOWS_LOG_ROOT}"
PID_FILE_PATH="${WINDOWS_LOG_ROOT}/benshu-llama-vulkan-${PORT}.pid"
STDOUT_LOG_PATH="${WINDOWS_LOG_ROOT}/benshu-llama-vulkan-${PORT}.out.log"
STDERR_LOG_PATH="${WINDOWS_LOG_ROOT}/benshu-llama-vulkan-${PORT}.err.log"
WIN_PID_FILE_PATH="$(wslpath -w "${PID_FILE_PATH}")"
WIN_STDOUT_LOG_PATH="$(wslpath -w "${STDOUT_LOG_PATH}")"
WIN_STDERR_LOG_PATH="$(wslpath -w "${STDERR_LOG_PATH}")"

echo "Bridge target agent role: ${AGENT_ROLE}"
if [[ -n "${CONFIGURED_LOCAL_RUNTIME_FAMILY}" ]]; then
  echo "Configured local runtime family: ${CONFIGURED_LOCAL_RUNTIME_FAMILY}"
fi
echo "Bridge alias: ${ALIAS}"
echo "Bridge model source: ${MODEL_PATH}"
if [[ -n "${MMPROJ_PATH}" ]]; then
  echo "Bridge mmproj source: ${MMPROJ_PATH}"
fi

POWERSHELL_ARGS=(
  -NoLogo
  -NoProfile
  -ExecutionPolicy Bypass
  -File "${WIN_START_SCRIPT}"
  -ServerExe "${WIN_SERVER_EXE}"
  -MinBuild 9592
  -ModelPath "${WIN_MODEL_PATH}"
  -Port "${PORT}"
  -CtxSize "${CTX_SIZE}"
  -GpuLayers "${GPU_LAYERS}"
  -Threads "${THREADS}"
  -BatchSize "${BATCH_SIZE}"
  -UbatchSize "${UBATCH_SIZE}"
  -ParallelSlots "${PARALLEL_SLOTS}"
  -CacheRam "${CACHE_RAM}"
  -CtxCheckpoints "${CTX_CHECKPOINTS}"
  -FlashAttnMode "${FLASH_ATTN_MODE}"
  -KvOffload "${KV_OFFLOAD}"
  -Mmap "${MMAP_MODE}"
  -Mlock "${MLOCK_MODE}"
  -CachePrompt "${CACHE_PROMPT}"
  -ContBatching "${CONT_BATCHING}"
  -Warmup "${WARMUP_MODE}"
  -ContextShift "${CONTEXT_SHIFT}"
  -Jinja "${JINJA_MODE}"
  -CpuMoe "${CPU_MOE}"
  -FitMode "${FIT_MODE}"
  -MmprojOffload "${MMPROJ_OFFLOAD}"
  -ReasoningMode "${REASONING_MODE}"
  -ReasoningFormat "${REASONING_FORMAT}"
  -SamplingTemperature "${SAMPLING_TEMPERATURE}"
  -SamplingTopK "${SAMPLING_TOP_K}"
  -SamplingTopP "${SAMPLING_TOP_P}"
  -SamplingMinP "${SAMPLING_MIN_P}"
  -SamplingTypicalP "${SAMPLING_TYPICAL_P}"
  -SamplingRepeatPenalty "${SAMPLING_REPEAT_PENALTY}"
  -SamplingPresencePenalty "${SAMPLING_PRESENCE_PENALTY}"
  -SamplingFrequencyPenalty "${SAMPLING_FREQUENCY_PENALTY}"
  -SamplingMirostat "${SAMPLING_MIROSTAT}"
  -SamplingMirostatEta "${SAMPLING_MIROSTAT_ETA}"
  -SamplingMirostatTau "${SAMPLING_MIROSTAT_TAU}"
  -BindHost "${WINDOWS_SERVER_BIND_HOST}"
  -Alias "${ALIAS}"
  -ApiKey "${API_KEY}"
  -PidFile "${WIN_PID_FILE_PATH}"
  -StdoutLogFile "${WIN_STDOUT_LOG_PATH}"
  -StderrLogFile "${WIN_STDERR_LOG_PATH}"
  -ReadyTimeoutSecs "${READY_TIMEOUT_SECS}"
)

if [[ -n "${WIN_MMPROJ_PATH}" ]]; then
  POWERSHELL_ARGS+=(-MmprojPath "${WIN_MMPROJ_PATH}")
fi

if [[ -n "${WIN_MEDIA_PATH}" ]]; then
  POWERSHELL_ARGS+=(-MediaPath "${WIN_MEDIA_PATH}")
fi

if [[ -n "${THREADS_BATCH}" ]]; then
  POWERSHELL_ARGS+=(-ThreadsBatch "${THREADS_BATCH}")
fi

if [[ -n "${ROPE_SCALING}" ]]; then
  POWERSHELL_ARGS+=(-RopeScaling "${ROPE_SCALING}")
fi

if [[ -n "${ROPE_SCALE}" ]]; then
  POWERSHELL_ARGS+=(-RopeScale "${ROPE_SCALE}")
fi

if [[ -n "${ROPE_FREQ_BASE}" ]]; then
  POWERSHELL_ARGS+=(-RopeFreqBase "${ROPE_FREQ_BASE}")
fi

if [[ -n "${ROPE_FREQ_SCALE}" ]]; then
  POWERSHELL_ARGS+=(-RopeFreqScale "${ROPE_FREQ_SCALE}")
fi

if [[ -n "${YARN_ORIG_CTX}" ]]; then
  POWERSHELL_ARGS+=(-YarnOrigCtx "${YARN_ORIG_CTX}")
fi

if [[ -n "${YARN_EXT_FACTOR}" ]]; then
  POWERSHELL_ARGS+=(-YarnExtFactor "${YARN_EXT_FACTOR}")
fi

if [[ -n "${YARN_ATTN_FACTOR}" ]]; then
  POWERSHELL_ARGS+=(-YarnAttnFactor "${YARN_ATTN_FACTOR}")
fi

if [[ -n "${YARN_BETA_SLOW}" ]]; then
  POWERSHELL_ARGS+=(-YarnBetaSlow "${YARN_BETA_SLOW}")
fi

if [[ -n "${YARN_BETA_FAST}" ]]; then
  POWERSHELL_ARGS+=(-YarnBetaFast "${YARN_BETA_FAST}")
fi

if [[ -n "${CACHE_TYPE_K}" ]]; then
  POWERSHELL_ARGS+=(-CacheTypeK "${CACHE_TYPE_K}")
fi

if [[ -n "${CACHE_TYPE_V}" ]]; then
  POWERSHELL_ARGS+=(-CacheTypeV "${CACHE_TYPE_V}")
fi

if [[ -n "${DEVICE}" ]]; then
  POWERSHELL_ARGS+=(-Device "${DEVICE}")
fi

if [[ -n "${SPLIT_MODE}" ]]; then
  POWERSHELL_ARGS+=(-SplitMode "${SPLIT_MODE}")
fi

if [[ -n "${TENSOR_SPLIT}" ]]; then
  POWERSHELL_ARGS+=(-TensorSplit "${TENSOR_SPLIT}")
fi

if [[ -n "${MAIN_GPU}" ]]; then
  POWERSHELL_ARGS+=(-MainGpu "${MAIN_GPU}")
fi

if [[ -n "${FIT_TARGET}" ]]; then
  POWERSHELL_ARGS+=(-FitTarget "${FIT_TARGET}")
fi

if [[ -n "${FIT_CTX}" ]]; then
  POWERSHELL_ARGS+=(-FitCtx "${FIT_CTX}")
fi

if [[ -n "${N_CPU_MOE}" ]]; then
  POWERSHELL_ARGS+=(-NCpuMoe "${N_CPU_MOE}")
fi

if [[ -n "${IMAGE_MIN_TOKENS}" ]]; then
  POWERSHELL_ARGS+=(-ImageMinTokens "${IMAGE_MIN_TOKENS}")
fi

if [[ -n "${IMAGE_MAX_TOKENS}" ]]; then
  POWERSHELL_ARGS+=(-ImageMaxTokens "${IMAGE_MAX_TOKENS}")
fi

if [[ -n "${REASONING_BUDGET}" ]]; then
  POWERSHELL_ARGS+=(-ReasoningBudget "${REASONING_BUDGET}")
fi

if [[ -n "${REASONING_BUDGET_MESSAGE}" ]]; then
  POWERSHELL_ARGS+=(-ReasoningBudgetMessage "${REASONING_BUDGET_MESSAGE}")
fi

if [[ -n "${SEED}" ]]; then
  POWERSHELL_ARGS+=(-Seed "${SEED}")
fi

POWERSHELL_OUTPUT_FILE="$(mktemp)"
set +e
powershell.exe "${POWERSHELL_ARGS[@]}" >"${POWERSHELL_OUTPUT_FILE}" 2>&1 &
POWERSHELL_PID=$!
POWERSHELL_STATUS=""
for _ in $(seq 1 "${READY_TIMEOUT_SECS}"); do
  if ! kill -0 "${POWERSHELL_PID}" 2>/dev/null; then
    wait "${POWERSHELL_PID}"
    POWERSHELL_STATUS=$?
    break
  fi
  if probe_models; then
    echo "PowerShell start wrapper is still attached after model registry became reachable; detaching wrapper PID=${POWERSHELL_PID}." >>"${POWERSHELL_OUTPUT_FILE}"
    kill "${POWERSHELL_PID}" 2>/dev/null || true
    wait "${POWERSHELL_PID}" 2>/dev/null || true
    POWERSHELL_STATUS=0
    break
  fi
  sleep 1
done
if [[ -z "${POWERSHELL_STATUS}" ]]; then
  echo "PowerShell start wrapper did not finish within ${READY_TIMEOUT_SECS}s." >>"${POWERSHELL_OUTPUT_FILE}"
  kill "${POWERSHELL_PID}" 2>/dev/null || true
  wait "${POWERSHELL_PID}" 2>/dev/null || true
  POWERSHELL_STATUS=124
fi
set -e
POWERSHELL_OUTPUT="$(cat "${POWERSHELL_OUTPUT_FILE}")"
rm -f "${POWERSHELL_OUTPUT_FILE}"
printf '%s\n' "${POWERSHELL_OUTPUT}"

stop_started_bridge_process() {
  local pid
  pid="$(printf '%s\n' "${POWERSHELL_OUTPUT}" | sed -n 's/^PID=//p' | tail -n 1 | tr -dc '0-9')"
  if [[ -z "${pid}" ]]; then
    return 0
  fi
  powershell.exe -NoLogo -NoProfile -Command "try { Stop-Process -Id ${pid} -Force -ErrorAction SilentlyContinue } catch {}; try { taskkill /F /PID ${pid} | Out-Null } catch {}" >/dev/null 2>&1 || true
}

if [[ "${POWERSHELL_STATUS}" -ne 0 ]]; then
  if grep -qi "couldn't bind HTTP server socket" <<<"${POWERSHELL_OUTPUT}"; then
    if [[ "${PORT_RETRY_COUNT}" -lt 12 ]]; then
      NEXT_PORT=$((PORT + 1))
      echo "Port ${PORT} failed to bind; retrying bridge startup on ${NEXT_PORT}."
      export BENSHU_WINDOWS_LLAMA_PORT="${NEXT_PORT}"
      export BENSHU_WINDOWS_LLAMA_PORT_RETRY_COUNT="$((PORT_RETRY_COUNT + 1))"
      exec bash "$0"
    fi
    echo "Port bind retries exhausted after ${PORT_RETRY_COUNT} attempts."
  fi
  exit "${POWERSHELL_STATUS}"
fi

echo "Waiting for bridge model registry to report alias '${ALIAS}'..."
READY_DEADLINE=$((SECONDS + READY_TIMEOUT_SECS))
while (( SECONDS < READY_DEADLINE )); do
  if probe_models; then
    echo "Bridge model registry is reachable."
    break
  fi
  sleep 1
done

if ! probe_models; then
  echo "Windows llama.cpp bridge never exposed alias '${ALIAS}' through /v1/models within ${READY_TIMEOUT_SECS}s."
  exit 1
fi

echo "Waiting for first lightweight text completion to succeed..."
READY_DEADLINE=$((SECONDS + READY_TIMEOUT_SECS))
while (( SECONDS < READY_DEADLINE )); do
  probe_text_ready
  probe_status=$?
  if [[ "${probe_status}" -eq 0 ]]; then
    sync_bridge_runtime_config
    echo "Windows llama.cpp bridge is ready at ${BRIDGE_BASE_URL}/v1"
    echo "Agent '${AGENT_ROLE}' should point to provider=openai base_url=${BRIDGE_BASE_URL}/v1 model=${ALIAS}"
    exit 0
  fi
  if [[ "${probe_status}" -eq 2 ]]; then
    stop_started_bridge_process
    exit 1
  fi
  sleep 1
done

echo "Windows llama.cpp bridge passed health and model registration, but never completed a lightweight text probe within ${READY_TIMEOUT_SECS}s."
exit 1

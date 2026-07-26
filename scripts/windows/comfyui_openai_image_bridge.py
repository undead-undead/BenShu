#!/usr/bin/env python3
import base64
import json
import os
import sys
import time
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


COMFYUI_BASE_URL = os.environ.get("BENSHU_COMFYUI_BASE_URL", "http://127.0.0.1:8188").rstrip("/")
LISTEN_HOST = os.environ.get("BENSHU_IMAGE_BRIDGE_HOST", "127.0.0.1")
LISTEN_PORT = int(os.environ.get("BENSHU_IMAGE_BRIDGE_PORT", "8022"))
MODEL_NAME = os.environ.get("BENSHU_IMAGE_BRIDGE_MODEL", "local-image-model")
CHECKPOINT_NAME = os.environ.get("BENSHU_COMFYUI_CHECKPOINT", "")
NEGATIVE_PROMPT = os.environ.get(
    "BENSHU_COMFYUI_NEGATIVE_PROMPT",
    "blurry, low quality, distorted, bad anatomy, deformed",
)
SAMPLER_NAME = os.environ.get("BENSHU_COMFYUI_SAMPLER", "euler")
SCHEDULER = os.environ.get("BENSHU_COMFYUI_SCHEDULER", "normal")
STEPS = int(os.environ.get("BENSHU_COMFYUI_STEPS", "4"))
CFG = float(os.environ.get("BENSHU_COMFYUI_CFG", "1.5"))
DENOISE = float(os.environ.get("BENSHU_COMFYUI_DENOISE", "1.0"))
POLL_INTERVAL = float(os.environ.get("BENSHU_COMFYUI_POLL_INTERVAL", "1.0"))
POLL_TIMEOUT = float(os.environ.get("BENSHU_COMFYUI_TIMEOUT", "180"))
CLIENT_ID = os.environ.get("BENSHU_COMFYUI_CLIENT_ID", "benshu-image-bridge")
MAX_REQUEST_BYTES = 128 * 1024
MAX_COMFY_RESPONSE_BYTES = 64 * 1024 * 1024


def comfy_request(path, method="GET", body=None):
    data = None
    headers = {}
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"

    req = urllib.request.Request(
        f"{COMFYUI_BASE_URL}{path}",
        data=data,
        headers=headers,
        method=method,
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        payload = resp.read(MAX_COMFY_RESPONSE_BYTES + 1)
        if len(payload) > MAX_COMFY_RESPONSE_BYTES:
            raise RuntimeError("ComfyUI response exceeded 64MB safety limit")
        content_type = resp.headers.get("Content-Type", "")
        if "application/json" in content_type:
            return json.loads(payload.decode("utf-8"))
        return payload


def build_workflow(prompt, width, height):
    if not CHECKPOINT_NAME:
        raise RuntimeError(
            "BENSHU_COMFYUI_CHECKPOINT is required. It should match a checkpoint filename visible to ComfyUI."
        )

    # Minimal text-to-image graph.
    return {
        "4": {
            "class_type": "CheckpointLoaderSimple",
            "inputs": {
                "ckpt_name": CHECKPOINT_NAME,
            },
        },
        "5": {
            "class_type": "EmptyLatentImage",
            "inputs": {
                "width": width,
                "height": height,
                "batch_size": 1,
            },
        },
        "6": {
            "class_type": "CLIPTextEncode",
            "inputs": {
                "text": prompt,
                "clip": ["4", 1],
            },
        },
        "7": {
            "class_type": "CLIPTextEncode",
            "inputs": {
                "text": NEGATIVE_PROMPT,
                "clip": ["4", 1],
            },
        },
        "10": {
            "class_type": "KSampler",
            "inputs": {
                "seed": int(time.time() * 1000) % 2147483647,
                "steps": STEPS,
                "cfg": CFG,
                "sampler_name": SAMPLER_NAME,
                "scheduler": SCHEDULER,
                "denoise": DENOISE,
                "model": ["4", 0],
                "positive": ["6", 0],
                "negative": ["7", 0],
                "latent_image": ["5", 0],
            },
        },
        "8": {
            "class_type": "VAEDecode",
            "inputs": {
                "samples": ["10", 0],
                "vae": ["4", 2],
            },
        },
        "9": {
            "class_type": "SaveImage",
            "inputs": {
                "filename_prefix": "benshu",
                "images": ["8", 0],
            },
        },
    }


def wait_for_history(prompt_id):
    deadline = time.time() + POLL_TIMEOUT
    while time.time() < deadline:
        history = comfy_request(f"/history/{prompt_id}")
        if prompt_id in history:
            return history[prompt_id]
        time.sleep(POLL_INTERVAL)
    raise TimeoutError(f"ComfyUI prompt did not finish within {POLL_TIMEOUT} seconds")


def extract_first_image_meta(history_entry):
    outputs = history_entry.get("outputs", {})
    for node_output in outputs.values():
        for image_info in node_output.get("images", []):
            filename = image_info.get("filename")
            if filename:
                return {
                    "filename": filename,
                    "subfolder": image_info.get("subfolder", ""),
                    "type": image_info.get("type", "output"),
                }
    raise RuntimeError("ComfyUI did not return any output images")


def fetch_image_bytes(meta):
    query = urllib.parse.urlencode(meta)
    return comfy_request(f"/view?{query}")


class Handler(BaseHTTPRequestHandler):
    server_version = "BenShu-ComfyUI-ImageBridge/0.1"

    def _send_json(self, status, payload):
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path in ("/health", "/v1/health"):
            ready = bool(CHECKPOINT_NAME)
            self._send_json(
                200,
                {
                    "ok": ready,
                    "backend": "comfyui",
                    "model": MODEL_NAME,
                    "comfyui_base_url": COMFYUI_BASE_URL,
                    "checkpoint": CHECKPOINT_NAME,
                },
            )
            return

        self._send_json(404, {"error": "not_found"})

    def do_POST(self):
        if self.path != "/v1/images/generations":
            self._send_json(404, {"error": "not_found"})
            return

        try:
            content_length = int(self.headers.get("Content-Length", "0"))
            if content_length > MAX_REQUEST_BYTES:
                raise ValueError("request body exceeded 128KB safety limit")
            raw = self.rfile.read(content_length)
            payload = json.loads(raw.decode("utf-8"))

            prompt = payload.get("prompt", "").strip()
            size = payload.get("size", "1024x1024")
            if not prompt:
                raise ValueError("prompt is required")
            try:
                width_str, height_str = size.lower().split("x", 1)
                width = int(width_str)
                height = int(height_str)
            except Exception as exc:
                raise ValueError(f"invalid size: {size}") from exc

            workflow = build_workflow(prompt, width, height)
            enqueue = comfy_request(
                "/prompt",
                method="POST",
                body={"prompt": workflow, "client_id": CLIENT_ID},
            )
            prompt_id = enqueue.get("prompt_id")
            if not prompt_id:
                raise RuntimeError("ComfyUI did not return prompt_id")

            history = wait_for_history(prompt_id)
            image_meta = extract_first_image_meta(history)
            image_bytes = fetch_image_bytes(image_meta)
            image_b64 = base64.b64encode(image_bytes).decode("ascii")

            self._send_json(
                200,
                {
                    "created": int(time.time()),
                    "data": [{"b64_json": image_b64}],
                },
            )
        except Exception as exc:
            self._send_json(
                500,
                {
                    "error": {
                        "message": str(exc),
                        "type": "image_bridge_error",
                    }
                },
            )


def main():
    httpd = ThreadingHTTPServer((LISTEN_HOST, LISTEN_PORT), Handler)
    print(
        json.dumps(
            {
                "status": "ready",
                "listen": f"http://{LISTEN_HOST}:{LISTEN_PORT}/v1",
                "comfyui": COMFYUI_BASE_URL,
                "model": MODEL_NAME,
            }
        ),
        flush=True,
    )
    httpd.serve_forever()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(0)

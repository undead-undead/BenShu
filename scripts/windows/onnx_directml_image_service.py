#!/usr/bin/env python3
import base64
import io
import json
import os
import inspect
import sys
import time
import traceback
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def read_env(primary: str, legacy: str, default: str = "") -> str:
    value = os.environ.get(primary)
    if value is not None and value.strip():
        return value.strip()
    value = os.environ.get(legacy)
    if value is not None and value.strip():
        return value.strip()
    return default


ONNX_MODEL_DIR = Path(read_env("BENSHU_ONNX_IMAGE_MODEL_DIR", "BENSHU_ONNX_DIFFUSION_MODEL_DIR"))
SOURCE_MODEL_DIR = Path(
    read_env("BENSHU_ONNX_IMAGE_SOURCE_MODEL_DIR", "BENSHU_ONNX_DIFFUSION_SOURCE_MODEL_DIR")
)
LISTEN_HOST = read_env("BENSHU_ONNX_IMAGE_HOST", "BENSHU_ONNX_DIFFUSION_HOST", "127.0.0.1")
LISTEN_PORT = int(read_env("BENSHU_ONNX_IMAGE_PORT", "BENSHU_ONNX_DIFFUSION_PORT", "8022"))
MODEL_NAME = read_env("BENSHU_ONNX_IMAGE_MODEL_NAME", "BENSHU_ONNX_DIFFUSION_MODEL_NAME", "local-image-model")
NUM_STEPS = int(read_env("BENSHU_ONNX_IMAGE_STEPS", "BENSHU_ONNX_DIFFUSION_STEPS", "4"))
GUIDANCE_SCALE = float(
    read_env("BENSHU_ONNX_IMAGE_GUIDANCE_SCALE", "BENSHU_ONNX_DIFFUSION_GUIDANCE_SCALE", "0.0")
)
NEGATIVE_PROMPT = read_env(
    "BENSHU_ONNX_IMAGE_NEGATIVE_PROMPT",
    "BENSHU_ONNX_DIFFUSION_NEGATIVE_PROMPT",
    "blurry, low quality, distorted, bad anatomy, deformed",
)
DEVICE_ID = int(read_env("BENSHU_ONNX_IMAGE_DEVICE_ID", "BENSHU_ONNX_DIFFUSION_DEVICE_ID", "0"))

PIPE = None
PIPE_INFO = {}


PIPELINE_ADAPTERS = {
    "ORTStableDiffusionPipeline": {
        "adapter": "diffusers_ort_stable_diffusion",
        "pipeline_family": "stable-diffusion",
        "loader": "stable_diffusion",
    },
    "ORTStableDiffusionXLPipeline": {
        "adapter": "diffusers_ort_stable_diffusion_xl",
        "pipeline_family": "stable-diffusion-xl",
        "loader": "stable_diffusion_xl",
    },
}


def read_model_class_name(path: Path) -> str:
    model_index = path / "model_index.json"
    if not model_index.exists():
        raise RuntimeError(f"model_index.json not found in ONNX bundle: {path}")
    payload = json.loads(model_index.read_text(encoding="utf-8"))
    return str(payload.get("_class_name", "")).strip()


def onnx_bundle_ready(path: Path) -> bool:
    return path.exists() and path.joinpath("model_index.json").exists() and any(
        path.rglob("*.onnx")
    )


def read_bundle_manifest(path: Path) -> dict:
    manifest_path = path / "benshu_image_bundle.json"
    if not manifest_path.exists():
        return {}
    payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    return payload if isinstance(payload, dict) else {}


def resolve_pipeline_adapter(path: Path) -> dict:
    manifest = read_bundle_manifest(path)
    runtime_pipeline_class = str(manifest.get("runtime_pipeline_class", "")).strip()
    if runtime_pipeline_class:
        adapter = PIPELINE_ADAPTERS.get(runtime_pipeline_class)
        if not adapter:
            raise RuntimeError(
                f"Unsupported ONNX image runtime pipeline class from manifest: {runtime_pipeline_class}"
            )
        return {
            **adapter,
            "runtime_pipeline_class": runtime_pipeline_class,
            "source_pipeline_class": str(manifest.get("source_pipeline_class", "")).strip(),
            "capabilities": manifest.get("capabilities", {}),
            "manifest": manifest,
        }

    model_class_name = read_model_class_name(path)
    adapter = PIPELINE_ADAPTERS.get(model_class_name)
    if not adapter:
        raise RuntimeError(
            f"Unsupported ONNX image pipeline class: {model_class_name}"
        )
    return {
        **adapter,
        "runtime_pipeline_class": model_class_name,
        "source_pipeline_class": "",
        "capabilities": {},
        "manifest": {},
    }


def detect_runtime_capabilities(pipe) -> dict:
    signature = inspect.signature(pipe.__call__)
    parameters = signature.parameters
    has_image = "image" in parameters
    has_mask_image = "mask_image" in parameters
    return {
        "text_to_image": True,
        "image_edit": has_image,
        "mask_edit": has_image and has_mask_image,
    }


def ensure_onnx_bundle_ready():
    if not ONNX_MODEL_DIR:
        raise RuntimeError("BENSHU_ONNX_IMAGE_MODEL_DIR is required")
    if onnx_bundle_ready(ONNX_MODEL_DIR):
        return
    raise RuntimeError(
        f"ONNX image bundle is not ready: {ONNX_MODEL_DIR}. Export it first with export_onnx_diffusion_model.py."
    )


def load_pipeline():
    global PIPE
    global PIPE_INFO

    ensure_onnx_bundle_ready()

    import onnxruntime as ort
    from optimum.onnxruntime import (
        ORTStableDiffusionPipeline,
        ORTStableDiffusionXLPipeline,
    )

    available = ort.get_available_providers()
    if "DmlExecutionProvider" not in available:
        raise RuntimeError(
            f"DmlExecutionProvider is unavailable in this Python environment. Providers={available}"
        )

    provider_options = {"device_id": DEVICE_ID}

    adapter = resolve_pipeline_adapter(ONNX_MODEL_DIR)
    runtime_pipeline_class = adapter["runtime_pipeline_class"]
    if adapter["loader"] == "stable_diffusion_xl":
        pipeline_cls = ORTStableDiffusionXLPipeline
    elif adapter["loader"] == "stable_diffusion":
        pipeline_cls = ORTStableDiffusionPipeline
    else:
        raise RuntimeError(
            f"Unsupported ONNX image adapter loader: {adapter['loader']}"
        )

    pipe = pipeline_cls.from_pretrained(
        str(ONNX_MODEL_DIR),
        provider="DmlExecutionProvider",
        provider_options=provider_options,
        local_files_only=True,
    )
    runtime_capabilities = detect_runtime_capabilities(pipe)
    manifest_capabilities = adapter.get("capabilities", {})
    capabilities = {
        "text_to_image": bool(
            manifest_capabilities.get("text_to_image", runtime_capabilities["text_to_image"])
        ),
        "image_edit": bool(
            manifest_capabilities.get("image_edit", runtime_capabilities["image_edit"])
        ),
        "mask_edit": bool(
            manifest_capabilities.get("mask_edit", runtime_capabilities["mask_edit"])
        ),
    }

    PIPE = pipe
    PIPE_INFO = {
        "providers": available,
        "active_provider": "DmlExecutionProvider",
        "provider_options": provider_options,
        "onnx_model_dir": str(ONNX_MODEL_DIR),
        "source_model_dir": str(SOURCE_MODEL_DIR) if SOURCE_MODEL_DIR else "",
        "adapter": adapter["adapter"],
        "pipeline_family": adapter["pipeline_family"],
        "pipeline_class": runtime_pipeline_class,
        "source_pipeline_class": adapter["source_pipeline_class"],
        "capabilities": capabilities,
        "editing_mode": "best_effort",
    }


def require_edit_capability(mask_image):
    capabilities = PIPE_INFO.get("capabilities", {})
    if mask_image is not None:
        if not capabilities.get("mask_edit"):
            raise RuntimeError(
                "Current ONNX image pipeline does not support masked editing yet."
            )
    else:
        if not capabilities.get("image_edit"):
            raise RuntimeError(
                "Current ONNX image pipeline does not support image editing yet."
            )


def decode_b64_image(payload: str):
    from PIL import Image

    raw = base64.b64decode(payload)
    return Image.open(io.BytesIO(raw)).convert("RGBA")


def generate_image(
    prompt,
    width,
    height,
    steps=None,
    guidance_scale=None,
    initial_image=None,
    mask_image=None,
):
    if PIPE is None:
        raise RuntimeError("Pipeline not initialized")

    kwargs = {
        "prompt": prompt,
        "negative_prompt": NEGATIVE_PROMPT,
        "height": height,
        "width": width,
        "num_inference_steps": steps or NUM_STEPS,
        "guidance_scale": GUIDANCE_SCALE if guidance_scale is None else guidance_scale,
        "output_type": "pil",
    }

    if initial_image is not None:
        require_edit_capability(mask_image)
        kwargs["image"] = initial_image
    if mask_image is not None:
        kwargs["mask_image"] = mask_image

    try:
        result = PIPE(**kwargs)
    except TypeError as exc:
        if initial_image is not None:
            raise RuntimeError(
                f"Current ONNX image pipeline does not expose editing arguments compatible with this request: {exc}"
            ) from exc
        raise
    return result.images[0]


class Handler(BaseHTTPRequestHandler):
    server_version = "BenShu-ONNX-DirectML/0.1"

    def _send_json(self, status, payload):
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path in ("/health", "/v1/health"):
            self._send_json(
                200,
                {
                    "ok": PIPE is not None,
                    "backend": "onnx-directml",
                    "model": MODEL_NAME,
                    "pipe": PIPE_INFO,
                },
            )
            return
        self._send_json(404, {"error": "not_found"})

    def do_POST(self):
        if self.path not in ("/v1/images/generations", "/v1/images/edits"):
            self._send_json(404, {"error": "not_found"})
            return

        try:
            content_length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(content_length)
            payload = json.loads(raw.decode("utf-8"))

            prompt = payload.get("prompt", "").strip()
            if not prompt:
                raise ValueError("prompt is required")

            size = payload.get("size", "1024x1024")
            try:
                width_str, height_str = size.lower().split("x", 1)
                width = int(width_str)
                height = int(height_str)
            except Exception as exc:
                raise ValueError(f"invalid size: {size}") from exc

            steps = payload.get("num_inference_steps")
            guidance_scale = payload.get("guidance_scale")

            initial_image = None
            mask_image = None
            if self.path == "/v1/images/edits":
                image_b64 = payload.get("image_b64", "").strip()
                if not image_b64:
                    raise ValueError("image_b64 is required for /v1/images/edits")
                initial_image = decode_b64_image(image_b64)

                mask_b64 = str(payload.get("mask_b64", "") or "").strip()
                if mask_b64:
                    mask_image = decode_b64_image(mask_b64)

            image = generate_image(
                prompt,
                width,
                height,
                steps=steps,
                guidance_scale=guidance_scale,
                initial_image=initial_image,
                mask_image=mask_image,
            )
            buffer = io.BytesIO()
            image.save(buffer, format="PNG")
            image_b64 = base64.b64encode(buffer.getvalue()).decode("ascii")

            self._send_json(
                200,
                {
                    "created": int(time.time()),
                    "data": [{"b64_json": image_b64}],
                },
            )
        except Exception as exc:
            traceback.print_exc()
            self._send_json(
                500,
                {
                    "error": {
                        "message": str(exc),
                        "type": "onnx_directml_image_error",
                    }
                },
            )


def main():
    load_pipeline()
    httpd = ThreadingHTTPServer((LISTEN_HOST, LISTEN_PORT), Handler)
    print(
        json.dumps(
            {
                "status": "ready",
                "listen": f"http://{LISTEN_HOST}:{LISTEN_PORT}/v1",
                "model": MODEL_NAME,
                "pipe": PIPE_INFO,
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
    except Exception:
        traceback.print_exc()
        sys.exit(1)

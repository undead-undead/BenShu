#!/usr/bin/env python3
import argparse
import base64
import io
import json
import os
import sys
import time
import traceback
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


MODEL_DIR = os.environ.get("BENSHU_DIFFUSION_MODEL_DIR", "").strip()
LISTEN_HOST = os.environ.get("BENSHU_DIFFUSION_HOST", "127.0.0.1")
LISTEN_PORT = int(os.environ.get("BENSHU_DIFFUSION_PORT", "8022"))
MODEL_NAME = os.environ.get("BENSHU_DIFFUSION_MODEL_NAME", "local-image-model")
NUM_STEPS = int(os.environ.get("BENSHU_DIFFUSION_STEPS", "4"))
GUIDANCE_SCALE = float(os.environ.get("BENSHU_DIFFUSION_GUIDANCE_SCALE", "0.0"))
NEGATIVE_PROMPT = os.environ.get(
    "BENSHU_DIFFUSION_NEGATIVE_PROMPT",
    "blurry, low quality, distorted, bad anatomy, deformed",
)
DTYPE_NAME = os.environ.get("BENSHU_DIFFUSION_DTYPE", "float16").lower()

TEXT_PIPE = None
IMAGE_PIPE = None
INPAINT_PIPE = None
PIPE_INFO = {}


def parse_args():
    parser = argparse.ArgumentParser(
        description="Serve a local Diffusers text-to-image pipeline over an OpenAI-compatible bridge."
    )
    parser.add_argument("--model-dir", default=MODEL_DIR)
    parser.add_argument("--listen-host", default=LISTEN_HOST)
    parser.add_argument("--listen-port", type=int, default=LISTEN_PORT)
    parser.add_argument("--model-name", default=MODEL_NAME)
    parser.add_argument("--steps", type=int, default=NUM_STEPS)
    parser.add_argument("--guidance-scale", type=float, default=GUIDANCE_SCALE)
    parser.add_argument("--negative-prompt", default=NEGATIVE_PROMPT)
    parser.add_argument("--dtype", default=DTYPE_NAME)
    return parser.parse_args()


def apply_args(args):
    global MODEL_DIR
    global LISTEN_HOST
    global LISTEN_PORT
    global MODEL_NAME
    global NUM_STEPS
    global GUIDANCE_SCALE
    global NEGATIVE_PROMPT
    global DTYPE_NAME

    MODEL_DIR = args.model_dir
    LISTEN_HOST = args.listen_host
    LISTEN_PORT = args.listen_port
    MODEL_NAME = args.model_name
    NUM_STEPS = args.steps
    GUIDANCE_SCALE = args.guidance_scale
    NEGATIVE_PROMPT = args.negative_prompt
    DTYPE_NAME = args.dtype.lower()


def _move_component(component, device, dtype):
    if component is None:
        return None
    try:
        return component.to(device=device, dtype=dtype)
    except TypeError:
        return component.to(device)


def _read_model_class_name(model_dir: str) -> str:
    model_index = Path(model_dir) / "model_index.json"
    if not model_index.exists():
        return ""
    try:
        payload = json.loads(model_index.read_text(encoding="utf-8"))
    except Exception:
        return ""
    return str(payload.get("_class_name", "")).strip()


def _summarize_pipe_devices(pipe):
    placements = {}
    for name in ("transformer", "unet", "vae", "text_encoder", "text_encoder_2"):
        component = getattr(pipe, name, None)
        if component is None:
            continue
        placements[name] = str(getattr(component, "device", "unknown"))
    return placements


def _configure_pipe_for_directml(pipe, dml_device, current_dtype):
    import torch
    from diffusers import DDIMScheduler
    from diffusers.models.attention_processor import AttnProcessor

    heavy_components = []
    if getattr(pipe, "transformer", None) is not None:
        heavy_components.append("transformer")
    if getattr(pipe, "unet", None) is not None:
        heavy_components.append("unet")
    if getattr(pipe, "vae", None) is not None:
        heavy_components.append("vae")

    for name in heavy_components:
        component = getattr(pipe, name, None)
        if component is not None:
            setattr(pipe, name, _move_component(component, dml_device, current_dtype))

    has_dual_text_encoders = getattr(pipe, "text_encoder_2", None) is not None
    if getattr(pipe, "text_encoder", None) is not None:
        if has_dual_text_encoders:
            pipe.text_encoder = pipe.text_encoder.to(device="cpu", dtype=torch.float32)
        else:
            pipe.text_encoder = _move_component(pipe.text_encoder, dml_device, current_dtype)
    if getattr(pipe, "text_encoder_2", None) is not None:
        pipe.text_encoder_2 = pipe.text_encoder_2.to(device="cpu", dtype=torch.float32)

    if getattr(pipe, "scheduler", None) is not None:
        pipe.scheduler = DDIMScheduler.from_config(pipe.scheduler.config)

    for name in ("unet", "transformer"):
        component = getattr(pipe, name, None)
        if component is not None and hasattr(component, "set_attn_processor"):
            component.set_attn_processor(AttnProcessor())

    if hasattr(pipe, "enable_attention_slicing"):
        pipe.enable_attention_slicing()

    if hasattr(pipe, "set_progress_bar_config"):
        pipe.set_progress_bar_config(disable=True)

    return pipe


def _load_text_pipeline():
    import torch
    import torch_directml
    from diffusers import AutoPipelineForText2Image

    dml_device = torch_directml.device()
    dtype = torch.float16 if DTYPE_NAME == "float16" else torch.float32
    last_error = None

    for current_dtype in [torch.float32, dtype]:
        try:
            pipe = AutoPipelineForText2Image.from_pretrained(
                MODEL_DIR,
                torch_dtype=current_dtype,
                use_safetensors=True,
                local_files_only=True,
            )
            pipe = _configure_pipe_for_directml(pipe, dml_device, current_dtype)
            return pipe, str(dml_device), str(current_dtype)
        except Exception as exc:
            traceback.print_exc()
            last_error = exc

    raise RuntimeError(f"Failed to load text-to-image pipeline on DirectML: {last_error}")


def load_pipeline():
    global TEXT_PIPE
    global PIPE_INFO

    if not MODEL_DIR:
        raise RuntimeError("BENSHU_DIFFUSION_MODEL_DIR is required")
    if not os.path.isdir(MODEL_DIR):
        raise RuntimeError(f"Model directory not found: {MODEL_DIR}")

    text_pipe, device_name, dtype_name = _load_text_pipeline()
    TEXT_PIPE = text_pipe
    model_class_name = _read_model_class_name(MODEL_DIR)
    PIPE_INFO = {
        "device": device_name,
        "dtype": dtype_name,
        "model_dir": MODEL_DIR,
        "model_class": model_class_name,
        "placement": _summarize_pipe_devices(text_pipe),
        "capabilities": {
            "text_to_image": True,
            "image_edit": "lazy",
            "mask_edit": "lazy",
        },
    }


def _reset_scheduler_timesteps(pipe):
    if getattr(pipe, "scheduler", None) is not None and hasattr(
        pipe.scheduler, "set_timesteps"
    ):
        pipe.scheduler.set_timesteps(NUM_STEPS)
        if hasattr(pipe.scheduler, "timesteps"):
            try:
                pipe.scheduler.timesteps = pipe.scheduler.timesteps.to("cpu")
            except Exception:
                pass

def _decode_b64_image(payload: str):
    from PIL import Image

    raw = base64.b64decode(payload)
    return Image.open(io.BytesIO(raw)).convert("RGBA")


def _ensure_image_pipe():
    global IMAGE_PIPE
    if IMAGE_PIPE is not None:
        return IMAGE_PIPE

    import torch
    import torch_directml
    from diffusers import AutoPipelineForImage2Image

    dml_device = torch_directml.device()
    dtype = torch.float16 if DTYPE_NAME == "float16" else torch.float32
    last_error = None
    for current_dtype in [torch.float32, dtype]:
        try:
            if hasattr(AutoPipelineForImage2Image, "from_pipe") and TEXT_PIPE is not None:
                pipe = AutoPipelineForImage2Image.from_pipe(TEXT_PIPE)
            else:
                pipe = AutoPipelineForImage2Image.from_pretrained(
                    MODEL_DIR,
                    torch_dtype=current_dtype,
                    use_safetensors=True,
                    local_files_only=True,
                )
            IMAGE_PIPE = _configure_pipe_for_directml(pipe, dml_device, current_dtype)
            PIPE_INFO.setdefault("capabilities", {})["image_edit"] = True
            return IMAGE_PIPE
        except Exception as exc:
            last_error = exc

    PIPE_INFO.setdefault("capabilities", {})["image_edit"] = False
    raise RuntimeError(f"Image editing pipeline is unavailable for this model: {last_error}")


def _ensure_inpaint_pipe():
    global INPAINT_PIPE
    if INPAINT_PIPE is not None:
        return INPAINT_PIPE

    import torch
    import torch_directml
    from diffusers import AutoPipelineForInpainting

    dml_device = torch_directml.device()
    dtype = torch.float16 if DTYPE_NAME == "float16" else torch.float32
    last_error = None
    for current_dtype in [torch.float32, dtype]:
        try:
            if hasattr(AutoPipelineForInpainting, "from_pipe") and TEXT_PIPE is not None:
                pipe = AutoPipelineForInpainting.from_pipe(TEXT_PIPE)
            else:
                pipe = AutoPipelineForInpainting.from_pretrained(
                    MODEL_DIR,
                    torch_dtype=current_dtype,
                    use_safetensors=True,
                    local_files_only=True,
                )
            INPAINT_PIPE = _configure_pipe_for_directml(pipe, dml_device, current_dtype)
            PIPE_INFO.setdefault("capabilities", {})["mask_edit"] = True
            return INPAINT_PIPE
        except Exception as exc:
            last_error = exc

    PIPE_INFO.setdefault("capabilities", {})["mask_edit"] = False
    raise RuntimeError(f"Masked editing pipeline is unavailable for this model: {last_error}")


def generate_image(prompt, width, height, image_b64=None, mask_b64=None):
    if TEXT_PIPE is None:
        raise RuntimeError("Pipeline not initialized")

    pipe = TEXT_PIPE
    kwargs = {}
    if image_b64:
        kwargs["image"] = _decode_b64_image(image_b64)
        if mask_b64:
            pipe = _ensure_inpaint_pipe()
            kwargs["mask_image"] = _decode_b64_image(mask_b64)
        else:
            pipe = _ensure_image_pipe()

    _reset_scheduler_timesteps(pipe)

    result = pipe(
        prompt=prompt,
        negative_prompt=NEGATIVE_PROMPT,
        width=width,
        height=height,
        num_inference_steps=NUM_STEPS,
        guidance_scale=GUIDANCE_SCALE,
        **kwargs,
    )
    image = result.images[0]
    return image


class Handler(BaseHTTPRequestHandler):
    server_version = "BenShu-DirectML-Diffusion/0.1"

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
                    "ok": TEXT_PIPE is not None,
                    "backend": "diffusers-directml",
                    "model": MODEL_NAME,
                    "model_dir": MODEL_DIR,
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
            size = payload.get("size", "1024x1024")
            if not prompt:
                raise ValueError("prompt is required")

            try:
                width_str, height_str = size.lower().split("x", 1)
                width = int(width_str)
                height = int(height_str)
            except Exception as exc:
                raise ValueError(f"invalid size: {size}") from exc

            image_b64 = None
            mask_b64 = None
            if self.path == "/v1/images/edits":
                image_b64 = str(payload.get("image_b64", "") or "").strip()
                if not image_b64:
                    raise ValueError("image_b64 is required for /v1/images/edits")
                mask_b64 = str(payload.get("mask_b64", "") or "").strip() or None

            image = generate_image(prompt, width, height, image_b64=image_b64, mask_b64=mask_b64)
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
                        "type": "diffusion_service_error",
                    }
                },
            )


def main():
    args = parse_args()
    apply_args(args)
    load_pipeline()
    httpd = ThreadingHTTPServer((LISTEN_HOST, LISTEN_PORT), Handler)
    print(
        json.dumps(
            {
                "status": "ready",
                "listen": f"http://{LISTEN_HOST}:{LISTEN_PORT}/v1",
                "model": MODEL_NAME,
                "model_dir": MODEL_DIR,
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

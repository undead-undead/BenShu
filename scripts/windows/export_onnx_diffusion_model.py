#!/usr/bin/env python3
import argparse
import json
from pathlib import Path


PIPELINE_ADAPTERS = {
    "StableDiffusionPipeline": {
        "adapter": "diffusers_ort_stable_diffusion",
        "pipeline_family": "stable-diffusion",
        "ort_class_name": "ORTStableDiffusionPipeline",
        "supports_text_to_image": True,
        "supports_image_edit": False,
        "supports_mask_edit": False,
    },
    "StableDiffusionXLPipeline": {
        "adapter": "diffusers_ort_stable_diffusion_xl",
        "pipeline_family": "stable-diffusion-xl",
        "ort_class_name": "ORTStableDiffusionXLPipeline",
        "supports_text_to_image": True,
        "supports_image_edit": False,
        "supports_mask_edit": False,
    },
}


def read_model_class_name(path: Path) -> str:
    model_index = path / "model_index.json"
    if not model_index.exists():
        raise SystemExit(f"model_index.json not found in source model directory: {path}")
    payload = json.loads(model_index.read_text(encoding="utf-8"))
    return str(payload.get("_class_name", "")).strip()


def onnx_bundle_ready(path: Path) -> bool:
    return path.exists() and path.joinpath("model_index.json").exists() and any(
        path.rglob("*.onnx")
    )


def write_bundle_manifest(output_dir: Path, source_dir: Path, task: str, adapter_spec: dict) -> None:
    manifest = {
        "format_version": 1,
        "bundle_kind": "onnx-image",
        "source_model_dir": str(source_dir),
        "task": task,
        "adapter": adapter_spec["adapter"],
        "pipeline_family": adapter_spec["pipeline_family"],
        "source_pipeline_class": adapter_spec["source_class_name"],
        "runtime_pipeline_class": adapter_spec["ort_class_name"],
        "capabilities": {
            "text_to_image": adapter_spec["supports_text_to_image"],
            "image_edit": adapter_spec["supports_image_edit"],
            "mask_edit": adapter_spec["supports_mask_edit"],
        },
    }
    (output_dir / "benshu_image_bundle.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Export a local image diffusers model into an ONNX bundle for ORT."
    )
    parser.add_argument("--source-model-dir", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--task", default="text-to-image")
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()

    source_dir = Path(args.source_model_dir).resolve()
    output_dir = Path(args.output_dir).resolve()

    if not source_dir.exists():
        raise SystemExit(f"Source model directory not found: {source_dir}")

    if onnx_bundle_ready(output_dir) and not args.force:
        print(
            json.dumps(
                {
                    "status": "ready",
                    "source_model_dir": str(source_dir),
                    "output_dir": str(output_dir),
                    "task": args.task,
                    "exported": False,
                }
            ),
            flush=True,
        )
        return 0

    output_dir.mkdir(parents=True, exist_ok=True)

    from optimum.onnxruntime import (
        ORTStableDiffusionPipeline,
        ORTStableDiffusionXLPipeline,
    )

    model_class_name = read_model_class_name(source_dir)
    adapter_spec = PIPELINE_ADAPTERS.get(model_class_name)
    if not adapter_spec:
        raise SystemExit(
            f"Unsupported image pipeline class for ONNX export: {model_class_name}"
        )
    adapter_spec = {
        **adapter_spec,
        "source_class_name": model_class_name,
    }

    if adapter_spec["ort_class_name"] == "ORTStableDiffusionXLPipeline":
        pipeline_cls = ORTStableDiffusionXLPipeline
    elif adapter_spec["ort_class_name"] == "ORTStableDiffusionPipeline":
        pipeline_cls = ORTStableDiffusionPipeline
    else:
        raise SystemExit(
            f"Unsupported ORT adapter pipeline class: {adapter_spec['ort_class_name']}"
        )

    pipe = pipeline_cls.from_pretrained(
        str(source_dir),
        export=True,
        provider="CPUExecutionProvider",
        local_files_only=True,
    )
    pipe.save_pretrained(str(output_dir))
    write_bundle_manifest(output_dir, source_dir, args.task, adapter_spec)

    print(
        json.dumps(
            {
                "status": "exported",
                "source_model_dir": str(source_dir),
                "output_dir": str(output_dir),
                "task": args.task,
                "exported": True,
                "adapter": adapter_spec["adapter"],
                "pipeline_family": adapter_spec["pipeline_family"],
                "pipeline_class": model_class_name,
                "onnx_files": sorted(str(path.relative_to(output_dir)) for path in output_dir.rglob("*.onnx")),
            }
        ),
        flush=True,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

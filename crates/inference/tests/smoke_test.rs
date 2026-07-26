use benshu_inference::backend::audio_candle::WhisperCandleBackend;
use benshu_inference::backend::audio_external::PiperBackend;
use benshu_inference::backend::embeddings::BertEmbeddingBackend;
use benshu_inference::backend::rerank::CandleRerankBackend;
use benshu_inference::backend::{
    AudioModelBackend, EmbeddingBackend, RerankBackend, SttBackend, TtsBackend, VisionModelBackend,
    VisionTask,
};
#[cfg(all(feature = "llama_cpp", feature = "rocm"))]
use benshu_inference::HardwareStatus;
#[cfg(feature = "llama_cpp")]
use benshu_inference::LlamaCppBackend;
use benshu_inference::{CandleBackend, GenerationConfig, InferenceConfig, KvEngine, ModelBackend};
use candle_core::Device;
use hf_hub::{Repo, RepoType};
use parking_lot::RwLock;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tempfile::TempDir;

#[cfg(feature = "llama_cpp")]
const DEFAULT_SMOKE_GGUF_REPO: &str = "Qwen/Qwen2.5-0.5B-Instruct-GGUF";
#[cfg(feature = "llama_cpp")]
const DEFAULT_SMOKE_GGUF_REVISION: &str = "main";
#[cfg(feature = "llama_cpp")]
const DEFAULT_SMOKE_GGUF_FILE: &str = "qwen2.5-0.5b-instruct-q4_k_m.gguf";
#[cfg(feature = "llama_cpp")]
const DEFAULT_LIVE_GGUF_FILE: &str = "qwen2.5-3b-instruct-q4_k_m.gguf";
const DEFAULT_SMOKE_EMBED_REPO: &str = "BAAI/bge-small-en-v1.5";
const DEFAULT_SMOKE_RERANK_REPO: &str = "BAAI/bge-reranker-base";
const DEFAULT_SMOKE_STT_REPO: &str = "openai/whisper-tiny";
const DEFAULT_SMOKE_TTS_ID: &str = "piper-en_US-lessac-medium";
const DEFAULT_MODEL_REVISION: &str = "main";

type SmokeResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct A0SmokeProfile {
    #[cfg(feature = "llama_cpp")]
    text_agent_repo: &'static str,
    #[cfg(feature = "llama_cpp")]
    text_agent_file: &'static str,
    embedding_repo: &'static str,
    rerank_repo: &'static str,
    stt_repo: &'static str,
    tts_id: &'static str,
}

impl Default for A0SmokeProfile {
    fn default() -> Self {
        Self {
            #[cfg(feature = "llama_cpp")]
            text_agent_repo: DEFAULT_SMOKE_GGUF_REPO,
            #[cfg(feature = "llama_cpp")]
            text_agent_file: DEFAULT_SMOKE_GGUF_FILE,
            embedding_repo: DEFAULT_SMOKE_EMBED_REPO,
            rerank_repo: DEFAULT_SMOKE_RERANK_REPO,
            stt_repo: DEFAULT_SMOKE_STT_REPO,
            tts_id: DEFAULT_SMOKE_TTS_ID,
        }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn repo_logo_path() -> PathBuf {
    repo_root().join("logo.png")
}

#[cfg(feature = "llama_cpp")]
fn default_repo_smoke_model_path() -> PathBuf {
    repo_root()
        .join("models")
        .join("smoke")
        .join(DEFAULT_SMOKE_GGUF_FILE)
}

#[cfg(feature = "llama_cpp")]
fn default_repo_live_model_path() -> PathBuf {
    repo_root()
        .join("models")
        .join("live")
        .join(DEFAULT_LIVE_GGUF_FILE)
}

fn default_repo_snapshot_dir(kind: &str, repo_slug: &str) -> PathBuf {
    repo_root()
        .join("models")
        .join("smoke")
        .join(kind)
        .join(repo_slug.replace('/', "--"))
}

async fn resolve_hf_snapshot_dir(
    env_var: &str,
    repo_id: &str,
    repo_dir: PathBuf,
    required_files: &[&str],
) -> SmokeResult<PathBuf> {
    if let Ok(path) = std::env::var(env_var) {
        return Ok(PathBuf::from(path));
    }

    if required_files
        .iter()
        .all(|name| repo_dir.join(name).exists())
    {
        return Ok(repo_dir);
    }

    let api = hf_hub::api::tokio::ApiBuilder::new().build()?;
    let repo = api.repo(Repo::with_revision(
        repo_id.to_string(),
        RepoType::Model,
        DEFAULT_MODEL_REVISION.to_string(),
    ));

    let mut snapshot_dir = None;
    for file in required_files {
        let path = repo.get(file).await?;
        snapshot_dir = path.parent().map(|p| p.to_path_buf());
    }

    snapshot_dir.ok_or_else(|| "missing snapshot dir".into())
}

#[cfg(feature = "llama_cpp")]
async fn resolve_smoke_gguf_path() -> SmokeResult<PathBuf> {
    if let Ok(path) = std::env::var("GGUF_MODEL_PATH") {
        return Ok(PathBuf::from(path));
    }

    let repo_path = default_repo_smoke_model_path();
    if repo_path.exists() {
        return Ok(repo_path);
    }

    let api = hf_hub::api::tokio::ApiBuilder::new().build()?;
    let repo = api.repo(Repo::with_revision(
        DEFAULT_SMOKE_GGUF_REPO.to_string(),
        RepoType::Model,
        DEFAULT_SMOKE_GGUF_REVISION.to_string(),
    ));

    let cached = repo.get(DEFAULT_SMOKE_GGUF_FILE).await?;
    Ok(cached)
}

#[cfg(feature = "llama_cpp")]
async fn resolve_live_gguf_path() -> SmokeResult<PathBuf> {
    if let Ok(path) = std::env::var("BENSHU_LIVE_GGUF_PATH") {
        return Ok(PathBuf::from(path));
    }

    let repo_path = default_repo_live_model_path();
    if repo_path.exists() {
        return Ok(repo_path);
    }

    resolve_smoke_gguf_path().await
}

#[cfg(feature = "llama_cpp")]
async fn resolve_live_multimodal_gguf_path() -> SmokeResult<PathBuf> {
    if let Ok(path) = std::env::var("BENSHU_LIVE_MULTIMODAL_GGUF_PATH") {
        return Ok(PathBuf::from(path));
    }

    let live_root = repo_root().join("models").join("live");
    if live_root.exists() {
        let mut stack = vec![live_root];
        while let Some(dir) = stack.pop() {
            let mut entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries
                    .filter_map(std::result::Result::ok)
                    .collect::<Vec<_>>(),
                Err(_) => continue,
            };
            entries.sort_by_key(|entry| entry.path());

            let has_mmproj = entries.iter().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name.to_lowercase().contains("mmproj"))
                    .unwrap_or(false)
            });

            if has_mmproj {
                if let Some(model_path) = entries.iter().find_map(|entry| {
                    let path = entry.path();
                    let name = entry.file_name();
                    let name = name.to_str()?.to_lowercase();
                    let ext = path.extension().and_then(|e| e.to_str())?;
                    if ext == "gguf" && !name.contains("mmproj") {
                        Some(path)
                    } else {
                        None
                    }
                }) {
                    return Ok(model_path);
                }
            }

            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                }
            }
        }
    }

    resolve_live_gguf_path().await
}

#[cfg(feature = "llama_cpp")]
fn load_benshu_frontend_prompt(user_message: &str) -> SmokeResult<String> {
    let base = repo_root().join("data").join("agents").join("benshu");
    let agent = std::fs::read_to_string(base.join("AGENT.md"))?;
    let identity = std::fs::read_to_string(base.join("IDENTITY.md"))?;
    Ok(format!(
        "{agent}\n\n{identity}\n\n## Output Contract\n- Answer the user directly.\n- Do not emit critique tags, template markers, or hidden internal reasoning.\n- Keep the reply short and user-facing.\n\nUser: {user_message}\nAssistant:"
    ))
}

#[cfg(feature = "llama_cpp")]
fn frontend_stop_sequences() -> Vec<String> {
    [
        "\nUser:",
        "\nAssistant:",
        "\nSystem:",
        "\n---",
        "<|end|>",
        "<|im_end|>",
        "<|user|>",
        "<|assistant|>",
        "[CRITIQUE",
        "Final Answer:",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[cfg(feature = "llama_cpp")]
fn assert_clean_frontend_reply(reply: &str) {
    let lowered = reply.to_lowercase();
    for marker in [
        "<|end|>",
        "<|user|>",
        "<|assistant|>",
        "[critique]",
        "### reflexion critique",
        "final answer:",
        "\nuser:",
        "\nassistant:",
        "\n---",
    ] {
        assert!(
            !lowered.contains(&marker.to_lowercase()),
            "frontend reply leaked internal marker: {reply}"
        );
    }
}

#[cfg(all(feature = "llama_cpp", feature = "rocm"))]
fn assert_clean_multimodal_reply(reply: &str) {
    let trimmed = reply.trim();
    assert!(!trimmed.is_empty(), "multimodal reply should not be empty");

    for marker in [
        "请用一句中文简要描述这张图片里最主要的视觉内容。",
        "不要复述用户指令",
        "Output Contract",
        "User:",
        "Assistant:",
        "<|image|>",
        "<|channel>",
        "<channel|>",
        "thought\n",
        "<|end|>",
        "<|im_end|>",
    ] {
        assert!(
            !trimmed.contains(marker),
            "multimodal reply leaked prompt marker `{marker}`: {trimmed}"
        );
    }
}

async fn resolve_smoke_embed_dir() -> SmokeResult<PathBuf> {
    resolve_hf_snapshot_dir(
        "BENSHU_EMBED_MODEL_DIR",
        DEFAULT_SMOKE_EMBED_REPO,
        default_repo_snapshot_dir("embedding", DEFAULT_SMOKE_EMBED_REPO),
        &["config.json", "tokenizer.json", "model.safetensors"],
    )
    .await
}

async fn resolve_smoke_rerank_dir() -> SmokeResult<PathBuf> {
    resolve_hf_snapshot_dir(
        "BENSHU_RERANK_MODEL_DIR",
        DEFAULT_SMOKE_RERANK_REPO,
        default_repo_snapshot_dir("rerank", DEFAULT_SMOKE_RERANK_REPO),
        &["config.json", "tokenizer.json", "model.safetensors"],
    )
    .await
}

async fn resolve_smoke_stt_dir() -> SmokeResult<PathBuf> {
    resolve_hf_snapshot_dir(
        "BENSHU_STT_MODEL_DIR",
        DEFAULT_SMOKE_STT_REPO,
        default_repo_snapshot_dir("stt", DEFAULT_SMOKE_STT_REPO),
        &[
            "config.json",
            "tokenizer.json",
            "model.safetensors",
            "preprocessor_config.json",
        ],
    )
    .await
}

fn load_whisper_mel_filters(dir: &Path) -> SmokeResult<Vec<f32>> {
    let config = std::fs::read_to_string(dir.join("preprocessor_config.json"))?;
    let value: serde_json::Value = serde_json::from_str(&config)?;
    let rows = value["mel_filters"]
        .as_array()
        .ok_or("mel_filters missing")?;
    let mut flattened = Vec::new();
    for row in rows {
        let cols = row.as_array().ok_or("mel_filters row invalid")?;
        for item in cols {
            flattened.push(item.as_f64().ok_or("mel_filters value invalid")? as f32);
        }
    }
    Ok(flattened)
}

fn synthetic_pcm_16khz() -> Vec<f32> {
    let sample_rate = 16_000.0f32;
    let duration_secs = 1.0f32;
    let samples = (sample_rate * duration_secs) as usize;
    let freq = 440.0f32;
    (0..samples)
        .map(|i| {
            let t = i as f32 / sample_rate;
            (2.0 * std::f32::consts::PI * freq * t).sin() * 0.05
        })
        .collect()
}

#[cfg(unix)]
fn create_fake_piper_runtime() -> SmokeResult<TempDir> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("model.onnx"), b"fake-onnx")?;
    let script = dir.path().join("piper");
    std::fs::write(
        &script,
        b"#!/usr/bin/env sh\ncat >/dev/null\nprintf 'FAKE_PIPER_AUDIO_STREAM'\n",
    )?;
    let mut perms = std::fs::metadata(&script)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms)?;
    Ok(dir)
}

#[cfg(feature = "llama_cpp")]
async fn run_text_agent_smoke(profile: &A0SmokeProfile) -> SmokeResult<()> {
    let path = resolve_smoke_gguf_path().await?;
    let backend = LlamaCppBackend::new(&path, None)?;
    let kv_engine = Arc::new(RwLock::new(KvEngine::new(InferenceConfig::default())));
    let config = GenerationConfig {
        max_new_tokens: 24,
        session_id: Some("a0-text-agent-session".to_string()),
        ..Default::default()
    };

    let res = backend
        .generate(
            "a0-text-agent-req",
            "Reply with one short sentence confirming the A0 text agent smoke passed.",
            None,
            config,
            kv_engine,
        )
        .await?;

    #[cfg(feature = "rocm")]
    {
        let hw = HardwareStatus::detect();
        println!(
            "✅ A0 text agent smoke loaded: {} | repo={} file={} gpu={:?} rocm={}",
            backend.model_info(),
            profile.text_agent_repo,
            profile.text_agent_file,
            hw.gpu_vendor,
            hw.rocm_available
        );
    }
    #[cfg(not(feature = "rocm"))]
    {
        println!(
            "✅ A0 text agent smoke loaded: {} | repo={} file={}",
            backend.model_info(),
            profile.text_agent_repo,
            profile.text_agent_file
        );
    }

    assert!(!res.trim().is_empty());
    Ok(())
}

async fn run_embedding_smoke(profile: &A0SmokeProfile) -> SmokeResult<()> {
    let model_dir = resolve_smoke_embed_dir().await?;
    let backend = BertEmbeddingBackend::load(&model_dir, profile.embedding_repo.to_string())?;
    let embedding = backend.embed("BenShu embedding smoke").await?;

    println!(
        "✅ Embedding smoke loaded: {} | dim={}",
        backend.model_info(),
        backend.dimension()
    );

    assert_eq!(embedding.len(), backend.dimension());
    assert!(embedding.iter().any(|v| *v != 0.0));
    Ok(())
}

async fn run_rerank_smoke(profile: &A0SmokeProfile) -> SmokeResult<()> {
    let model_dir = resolve_smoke_rerank_dir().await?;
    let backend = CandleRerankBackend::load(&model_dir, profile.rerank_repo.to_string())?;
    let scores = backend
        .rerank(
            "Which option best matches GPU inference smoke?",
            &[
                "ROCm smoke validates AMD GPU inference.".to_string(),
                "This sentence is unrelated to inference.".to_string(),
            ],
        )
        .await?;

    println!("✅ Rerank smoke loaded: {}", backend.model_info());

    assert_eq!(scores.len(), 2);
    assert!(scores.iter().all(|score| *score >= 0.0 && *score <= 1.0));
    Ok(())
}

async fn run_stt_smoke(profile: &A0SmokeProfile) -> SmokeResult<()> {
    let model_dir = resolve_smoke_stt_dir().await?;
    let backend = WhisperCandleBackend::new(&model_dir, profile.stt_repo.to_string())?;
    backend
        .set_mel_filters(load_whisper_mel_filters(&model_dir)?)
        .await?;
    let transcript = backend.transcribe(&synthetic_pcm_16khz()).await?;

    println!("✅ STT smoke loaded: {}", backend.model_info());
    println!("🎧 STT smoke transcript: {}", transcript);

    assert!(transcript.len() < 512);
    Ok(())
}

#[cfg(unix)]
async fn run_tts_contract_smoke(profile: &A0SmokeProfile) -> SmokeResult<()> {
    let runtime = create_fake_piper_runtime()?;
    let backend = PiperBackend::new(runtime.path(), profile.tts_id.to_string())?;
    let audio = backend.synthesize("BenShu TTS smoke").await?;

    println!("✅ TTS contract smoke loaded: {}", backend.model_info());

    assert!(!audio.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_native_inference_session_reuse() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup Engine and Backend
    // Auto-detect the best device (prefers GPU if available)
    let device = if candle_core::utils::cuda_is_available() {
        println!("🎮 NVIDIA GPU detected via CUDA.");
        Device::new_cuda(0).unwrap_or(Device::Cpu)
    } else if candle_core::utils::metal_is_available() {
        println!("🎮 MacOS GPU detected via Metal.");
        Device::new_metal(0).unwrap_or(Device::Cpu)
    } else {
        println!("💻 Running on CPU (Optimized SIMD enabled).");
        Device::Cpu
    };

    let model_id = "TinyLlama/TinyLlama-1.1B-Chat-v1.0";
    let start_load = Instant::now();
    let backend = CandleBackend::new_llama(model_id, "main", device).await?;
    println!("✅ Model loaded in {:?}", start_load.elapsed());

    let kv_config = InferenceConfig::default();
    let kv_engine = Arc::new(RwLock::new(KvEngine::new(kv_config)));

    let session_id = "test-session-001";

    // 2. Turn 1: Initial Prompt (Cold Start)
    let prompt1 = "Hello, who are you?";
    println!("\n💬 Turn 1: {}", prompt1);

    let config1 = GenerationConfig {
        max_new_tokens: 20,
        session_id: Some(session_id.to_string()),
        ..Default::default()
    };

    let start1 = Instant::now();
    let res1 = backend
        .generate("req-1", prompt1, None, config1, kv_engine.clone())
        .await?;
    let elapsed1 = start1.elapsed();
    println!("🤖 Assistant: {}", res1);
    println!("⏱ Turn 1 took: {:?}", elapsed1);

    // 3. Turn 2: Follow-up (Should reuse KV Cache)
    // We append the previous conversation to simulate context building in the prompt
    let prompt2 = format!("{} {} What is your primary mission?", prompt1, res1);
    println!("\n💬 Turn 2 (Follow-up): What is your primary mission?");

    let config2 = GenerationConfig {
        max_new_tokens: 20,
        session_id: Some(session_id.to_string()),
        ..Default::default()
    };

    let start2 = Instant::now();
    let res2 = backend
        .generate("req-2", &prompt2, None, config2, kv_engine.clone())
        .await?;
    let elapsed2 = start2.elapsed();
    println!("🤖 Assistant: {}", res2);
    println!("⏱ Turn 2 took: {:?}", elapsed2);

    // In Llama models, if the prefix is cached, Turn 2 should be significantly faster
    // for the 'prefill' phase.
    println!("\n📊 Performance Info: {}", backend.model_info());

    assert!(!res1.is_empty());
    assert!(!res2.is_empty());

    Ok(())
}

#[cfg(feature = "llama_cpp")]
#[tokio::test]
async fn test_native_llamacpp_inference() {
    run_text_agent_smoke(&A0SmokeProfile::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn test_local_embedding_smoke() -> SmokeResult<()> {
    run_embedding_smoke(&A0SmokeProfile::default()).await
}

#[tokio::test]
async fn test_local_rerank_smoke() -> SmokeResult<()> {
    run_rerank_smoke(&A0SmokeProfile::default()).await
}

#[tokio::test]
async fn test_local_stt_smoke() -> SmokeResult<()> {
    run_stt_smoke(&A0SmokeProfile::default()).await
}

#[cfg(unix)]
#[tokio::test]
async fn test_local_tts_contract_smoke() -> SmokeResult<()> {
    run_tts_contract_smoke(&A0SmokeProfile::default()).await
}

#[cfg(all(feature = "llama_cpp", feature = "rocm"))]
#[tokio::test]
async fn test_native_llamacpp_rocm_inference_smoke() {
    let hw = HardwareStatus::detect();
    if !hw.rocm_available {
        println!("⏭ Skipping ROCm smoke (ROCm runtime not available)");
        return;
    }

    let model_path = resolve_smoke_gguf_path().await.unwrap();

    let backend = LlamaCppBackend::new(&model_path, None).unwrap();
    println!(
        "✅ ROCm smoke backend loaded: {} | gpu={:?} probe={:?} rocm={}",
        backend.model_info(),
        hw.gpu_vendor,
        hw.gpu_probe_source,
        hw.rocm_available
    );

    let kv_engine = Arc::new(RwLock::new(KvEngine::new(InferenceConfig::default())));
    let config = GenerationConfig {
        max_new_tokens: 24,
        session_id: Some("test-rocm-session".to_string()),
        ..Default::default()
    };

    let start = Instant::now();
    let res = backend
        .generate(
            "rocm-req-1",
            "Reply with one short sentence proving the ROCm smoke path works.",
            None,
            config,
            kv_engine,
        )
        .await
        .unwrap();

    println!("🤖 Assistant (ROCm smoke): {}", res);
    println!("⏱ ROCm smoke generation took: {:?}", start.elapsed());

    assert!(!res.trim().is_empty());
}

#[cfg(all(feature = "llama_cpp", unix))]
#[tokio::test]
async fn test_a0_model_profile_smoke() {
    let profile = A0SmokeProfile::default();

    run_text_agent_smoke(&profile).await.unwrap();
    run_embedding_smoke(&profile).await.unwrap();
    run_rerank_smoke(&profile).await.unwrap();
    run_stt_smoke(&profile).await.unwrap();
    run_tts_contract_smoke(&profile).await.unwrap();

    println!("✅ A0 model profile smoke passed");
}

#[cfg(all(feature = "llama_cpp", feature = "rocm"))]
#[tokio::test]
async fn test_live_frontend_baseline_smoke() {
    let hw = HardwareStatus::detect();
    if !hw.rocm_available {
        println!("⏭ Skipping live frontend baseline smoke (ROCm runtime not available)");
        return;
    }

    let model_path = resolve_live_gguf_path().await.unwrap();
    let backend = LlamaCppBackend::new(&model_path, None).unwrap();
    let kv_engine = Arc::new(RwLock::new(KvEngine::new(InferenceConfig::default())));

    let zh_prompt = load_benshu_frontend_prompt("你是谁？请用一句中文简要介绍自己。").unwrap();
    let zh_config = GenerationConfig {
        max_new_tokens: 48,
        temperature: 0.2,
        top_p: 0.8,
        timeout_secs: Some(90),
        stop_sequences: frontend_stop_sequences(),
        session_id: Some("live-frontend-zh".to_string()),
        ..Default::default()
    };
    let zh_start = Instant::now();
    let zh = backend
        .generate(
            "live-frontend-zh",
            &zh_prompt,
            None,
            zh_config,
            kv_engine.clone(),
        )
        .await
        .unwrap();
    println!("🤖 Frontend baseline zh: {}", zh);
    println!("⏱ Frontend baseline zh took: {:?}", zh_start.elapsed());
    assert!(!zh.trim().is_empty());
    assert_clean_frontend_reply(&zh);

    let en_prompt =
        load_benshu_frontend_prompt("Who are you? Reply in one short English sentence.").unwrap();
    let en_config = GenerationConfig {
        max_new_tokens: 48,
        temperature: 0.2,
        top_p: 0.8,
        timeout_secs: Some(90),
        stop_sequences: frontend_stop_sequences(),
        session_id: Some("live-frontend-en".to_string()),
        ..Default::default()
    };
    let en_start = Instant::now();
    let en = backend
        .generate("live-frontend-en", &en_prompt, None, en_config, kv_engine)
        .await
        .unwrap();
    println!("🤖 Frontend baseline en: {}", en);
    println!("⏱ Frontend baseline en took: {:?}", en_start.elapsed());
    assert!(!en.trim().is_empty());
    assert_clean_frontend_reply(&en);
}

#[cfg(all(feature = "llama_cpp", feature = "rocm"))]
#[tokio::test]
async fn test_live_multimodal_baseline_smoke() {
    let hw = HardwareStatus::detect();
    if !hw.rocm_available {
        println!("⏭ Skipping live multimodal smoke (ROCm runtime not available)");
        return;
    }

    let model_path = resolve_live_multimodal_gguf_path().await.unwrap();
    let backend = LlamaCppBackend::new(&model_path, None).unwrap();
    let image = image::open(repo_logo_path()).expect("repo logo should load");

    let config = GenerationConfig {
        max_new_tokens: 64,
        temperature: 0.1,
        top_p: 0.8,
        timeout_secs: Some(120),
        session_id: Some("live-multimodal-zh".to_string()),
        ..Default::default()
    };

    let start = Instant::now();
    let response = backend
        .vision_analyze(
            &image,
            VisionTask::Describe,
            Some("请用一句中文简要描述这张图片里最主要的视觉内容。"),
            Some(config),
        )
        .await
        .unwrap();

    println!("🤖 Live multimodal response: {}", response);
    println!("⏱ Live multimodal smoke took: {:?}", start.elapsed());

    assert!(
        !response.trim().is_empty(),
        "multimodal smoke should produce a non-empty response"
    );
    assert_clean_multimodal_reply(&response);
}

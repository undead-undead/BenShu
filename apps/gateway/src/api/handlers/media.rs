use crate::api::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct TranscribeResponse {
    pub text: String,
}

#[derive(Deserialize)]
pub struct SynthesizeRequest {
    pub text: String,
    pub voice_id: Option<String>,
}

pub async fn handle_transcribe(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let audio_data = body.to_vec();

    if audio_data.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No audio file provided".into()));
    }

    use benshu_sensory::{SensoryInput, SensoryOutput, SensoryRequest};

    let input = SensoryInput::Audio(body.to_vec());
    let stt_hint = state.app_config.read().sensory.stt_model.clone();
    let req = SensoryRequest::Audio {
        input,
        plugin_hint: stt_hint,
    };

    match state.kernel.sensory().dispatch(req).await {
        Ok(SensoryOutput::Text(text)) => Ok(Json(TranscribeResponse { text })),
        Ok(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Unexpected sensory output type".into(),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Transcription failed: {}", e),
        )),
    }
}

pub async fn handle_synthesize(
    State(state): State<AppState>,
    Json(payload): Json<SynthesizeRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if payload.text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Text cannot be empty".into()));
    }

    use benshu_sensory::{SensoryOutput, SensoryRequest};

    let tts_hint = state.app_config.read().sensory.tts_model.clone();
    let req = SensoryRequest::Speak {
        text: payload.text,
        plugin_hint: tts_hint,
    };

    match state.kernel.sensory().dispatch(req).await {
        Ok(SensoryOutput::Audio(audio)) => {
            Ok(([(axum::http::header::CONTENT_TYPE, "audio/mpeg")], audio))
        }
        Ok(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Unexpected sensory output type".into(),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Synthesis failed: {}", e),
        )),
    }
}

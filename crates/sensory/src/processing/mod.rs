/// Simple energy-based Voice Activity Detection (VAD)
pub fn detect_voice_activity(pcm: &[f32], threshold: f32) -> bool {
    if pcm.is_empty() {
        return false;
    }

    // Calculate RMS energy
    let energy = (pcm.iter().map(|&x| x * x).sum::<f32>() / pcm.len() as f32).sqrt();

    // Typical silence threshold is around 0.01 for normalized float pcm
    energy > threshold
}

/// Simple visual diffing to detect state changes.
/// Returns a score from 0.0 (identical) to 1.0 (completely different).
pub fn visual_diff(before: &image::DynamicImage, after: &image::DynamicImage) -> f32 {
    let b_gray = before.to_luma8();
    let a_gray = after.to_luma8();

    if b_gray.dimensions() != a_gray.dimensions() {
        return 1.0;
    }

    let mut diff_sum = 0u64;
    let (w, h) = b_gray.dimensions();

    for (p1, p2) in b_gray.pixels().zip(a_gray.pixels()) {
        diff_sum += (p1.0[0] as i32 - p2.0[0] as i32).abs() as u64;
    }

    let max_diff = w as u64 * h as u64 * 255;
    diff_sum as f32 / max_diff as f32
}

/// Enhance contrast for better OCR recognition.
pub fn enhance_contrast(img: &image::DynamicImage) -> image::DynamicImage {
    // Basic thresholding/stretching
    img.adjust_contrast(20.0) // Increase contrast by 20%
}

/// Resample audio to 16kHz Mono (Whisper standard).
pub fn resample_to_whisper(pcm: Vec<f32>, from_hz: u32) -> Vec<f32> {
    if from_hz == 16000 {
        return pcm;
    }
    // Very naive linear interpolation resampler
    let ratio = 16000.0 / from_hz as f32;
    let new_len = (pcm.len() as f32 * ratio) as usize;
    let mut out = Vec::with_capacity(new_len);
    for i in 0..new_len {
        let pos = i as f32 / ratio;
        let idx = pos as usize;
        if idx + 1 < pcm.len() {
            let frac = pos - idx as f32;
            out.push(pcm[idx] * (1.0 - frac) + pcm[idx + 1] * frac);
        } else {
            out.push(pcm[idx]);
        }
    }
    out
}

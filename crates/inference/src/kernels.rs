#[cfg(target_arch = "x86_64")]
use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(target_arch = "x86_64")]
static FEATURE_STATE: AtomicU8 = AtomicU8::new(0); // 0=Undetected, 1=AVX2+FMA, 2=AVX2-Only, 3=Scalar

#[cfg(target_arch = "x86_64")]
fn get_feature_state() -> u8 {
    let current = FEATURE_STATE.load(Ordering::Relaxed);
    if current != 0 {
        return current;
    }
    let state = if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
        1
    } else if is_x86_feature_detected!("avx2") {
        2
    } else {
        3
    };
    FEATURE_STATE.store(state, Ordering::Relaxed);
    state
}

/// Dot product for INT4 quantized vectors (Cold level)
/// Uses per-dimension scales and offsets provided by the quantizer.
/// Optimized: Runtime dispatch with tiered SIMD support and state caching.
pub fn dot_product_int4_f32(
    codes: &[u8],
    f32_vec: &[f32],
    min_vals: &[f32],
    max_vals: &[f32],
) -> f32 {
    #[cfg(target_arch = "x86_64")]
    {
        let features = get_feature_state();
        if is_x86_feature_detected!("avx512f") {
            return unsafe { dot_product_int4_avx512(codes, f32_vec, min_vals, max_vals) };
        }
        match features {
            1 => return unsafe { dot_product_int4_avx2_fma(codes, f32_vec, min_vals, max_vals) },
            2 => {
                return unsafe { dot_product_int4_avx2_no_fma(codes, f32_vec, min_vals, max_vals) }
            }
            _ => (),
        }
    }

    dot_product_int4_scalar(codes, f32_vec, min_vals, max_vals)
}

/// Internal scalar implementation of INT4 dot product
fn dot_product_int4_scalar(
    codes: &[u8],
    f32_vec: &[f32],
    min_vals: &[f32],
    max_vals: &[f32],
) -> f32 {
    let mut sum = 0.0;
    let dim = f32_vec.len();

    for (i, &byte) in codes.iter().enumerate() {
        let q1 = (byte & 0x0F) as f32;
        let q2 = ((byte >> 4) & 0x0F) as f32;

        let d1 = i * 2;
        let d2 = d1 + 1;

        if d2 >= dim {
            if d1 < dim {
                let range1 = (max_vals[d1] - min_vals[d1]).max(1e-6);
                sum += (q1 / 15.0 * range1 + min_vals[d1]) * f32_vec[d1];
            }
            break;
        }

        let range1 = (max_vals[d1] - min_vals[d1]).max(1e-6);
        sum += (q1 / 15.0 * range1 + min_vals[d1]) * f32_vec[d1];

        let range2 = (max_vals[d2] - min_vals[d2]).max(1e-6);
        sum += (q2 / 15.0 * range2 + min_vals[d2]) * f32_vec[d2];
    }
    sum
}

/// AVX2+FMA implementation of INT4 dot product.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_product_int4_avx2_fma(
    codes: &[u8],
    f32_vec: &[f32],
    min_vals: &[f32],
    max_vals: &[f32],
) -> f32 {
    use std::arch::x86_64::*;
    let dim = f32_vec.len();
    let mut sum_vec = _mm256_setzero_ps();
    let low_bits_mask = _mm256_set1_epi8(0x0F);
    let inv_15 = _mm256_set1_ps(1.0 / 15.0);

    let mut i = 0;
    while i + 15 < codes.len() && (i * 2 + 31) < dim {
        let chunk = _mm_loadu_si128(codes.as_ptr().add(i) as *const __m128i);
        let chunk_256 = _mm256_cvtepu8_epi16(chunk);

        let q1_i16 = _mm256_and_si256(chunk_256, low_bits_mask);
        let q2_i16 = _mm256_and_si256(_mm256_srli_epi16(chunk_256, 4), low_bits_mask);

        let q12_low = _mm256_unpacklo_epi16(q1_i16, q2_i16);
        let q12_high = _mm256_unpackhi_epi16(q1_i16, q2_i16);

        let q_blocks = [
            _mm256_castsi256_si128(q12_low),
            _mm256_extracti128_si256(q12_low, 1),
            _mm256_castsi256_si128(q12_high),
            _mm256_extracti128_si256(q12_high, 1),
        ];

        for (b_idx, &q_block_128) in q_blocks.iter().enumerate() {
            let offset = i * 2 + b_idx * 8;
            let q_f = _mm256_cvtepi32_ps(_mm256_cvtepi16_epi32(q_block_128));

            let min_vec = _mm256_loadu_ps(min_vals.as_ptr().add(offset));
            let max_vec = _mm256_loadu_ps(max_vals.as_ptr().add(offset));
            let q_vec = _mm256_loadu_ps(f32_vec.as_ptr().add(offset));

            let range_vec = _mm256_sub_ps(max_vec, min_vec);
            let scale_vec = _mm256_mul_ps(range_vec, inv_15);

            let decoded_v = _mm256_fmadd_ps(q_f, scale_vec, min_vec);
            sum_vec = _mm256_fmadd_ps(decoded_v, q_vec, sum_vec);
        }
        i += 16;
    }

    let mut f_buf = [0.0f32; 8];
    _mm256_storeu_ps(f_buf.as_mut_ptr(), sum_vec);
    let mut sum: f32 = f_buf.iter().sum();
    sum += dot_product_int4_scalar(
        &codes[i..],
        &f32_vec[i * 2..],
        &min_vals[i * 2..],
        &max_vals[i * 2..],
    );
    sum
}

/// Pure AVX2 (No-FMA) implementation of INT4 dot product.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn dot_product_int4_avx2_no_fma(
    codes: &[u8],
    f32_vec: &[f32],
    min_vals: &[f32],
    max_vals: &[f32],
) -> f32 {
    use std::arch::x86_64::*;
    let dim = f32_vec.len();
    let mut sum_vec = _mm256_setzero_ps();
    let low_bits_mask = _mm256_set1_epi8(0x0F);
    let inv_15 = _mm256_set1_ps(1.0 / 15.0);

    let mut i = 0;
    while i + 15 < codes.len() && (i * 2 + 31) < dim {
        let chunk = _mm_loadu_si128(codes.as_ptr().add(i) as *const __m128i);
        let chunk_256 = _mm256_cvtepu8_epi16(chunk);

        let q1_i16 = _mm256_and_si256(chunk_256, low_bits_mask);
        let q2_i16 = _mm256_and_si256(_mm256_srli_epi16(chunk_256, 4), low_bits_mask);

        let q12_low = _mm256_unpacklo_epi16(q1_i16, q2_i16);
        let q12_high = _mm256_unpackhi_epi16(q1_i16, q2_i16);

        let q_blocks = [
            _mm256_castsi256_si128(q12_low),
            _mm256_extracti128_si256(q12_low, 1),
            _mm256_castsi256_si128(q12_high),
            _mm256_extracti128_si256(q12_high, 1),
        ];

        for (b_idx, &q_block_128) in q_blocks.iter().enumerate() {
            let offset = i * 2 + b_idx * 8;
            let q_f = _mm256_cvtepi32_ps(_mm256_cvtepi16_epi32(q_block_128));

            let min_vec = _mm256_loadu_ps(min_vals.as_ptr().add(offset));
            let max_vec = _mm256_loadu_ps(max_vals.as_ptr().add(offset));
            let q_vec = _mm256_loadu_ps(f32_vec.as_ptr().add(offset));

            let range_vec = _mm256_sub_ps(max_vec, min_vec);
            let scale_vec = _mm256_mul_ps(range_vec, inv_15);

            // Using Mul + Add as fallback for non-FMA hardware
            let decoded_v = _mm256_add_ps(_mm256_mul_ps(q_f, scale_vec), min_vec);
            sum_vec = _mm256_add_ps(sum_vec, _mm256_mul_ps(decoded_v, q_vec));
        }
        i += 16;
    }

    let mut f_buf = [0.0f32; 8];
    _mm256_storeu_ps(f_buf.as_mut_ptr(), sum_vec);
    let mut sum: f32 = f_buf.iter().sum();
    sum += dot_product_int4_scalar(
        &codes[i..],
        &f32_vec[i * 2..],
        &min_vals[i * 2..],
        &max_vals[i * 2..],
    );
    sum
}

/// Dot product for Ternary quantized vectors (Background level)
/// Uses a tiered SIMD dispatch with a multiplication-free AVX2 kernel.
pub fn dot_product_ternary_f32(q_data: &[u8], f32_vec: &[f32]) -> f32 {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512vpopcntdq") {
            return unsafe { dot_product_ternary_avx512(q_data, f32_vec) };
        }
        if get_feature_state() <= 2 {
            return unsafe { dot_product_ternary_avx2(q_data, f32_vec) };
        }
    }
    dot_product_ternary_scalar(q_data, f32_vec)
}

fn dot_product_ternary_scalar(q_data: &[u8], f32_vec: &[f32]) -> f32 {
    let dim = f32_vec.len();
    if q_data.len() < 2 {
        return 0.0;
    }
    let scale = f16_to_f32(u16::from_le_bytes([q_data[0], q_data[1]])).max(1e-7);
    let mut dot = 0.0;
    let mut idx = 0;
    for &byte in &q_data[2..] {
        for j in 0..4 {
            if idx < dim {
                let q = ((byte >> (j * 2)) & 0x03) as i8 - 1;
                dot += (q as f32 * scale) * f32_vec[idx];
                idx += 1;
            } else {
                break;
            }
        }
    }
    dot
}

/// Optimized Ternary (1.58-bit) AVX2 kernel.
/// Implements BitNet-style addition-only logic: sum = scale * (sum(v where q=1) - sum(v where q=-1)).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn dot_product_ternary_avx2(q_data: &[u8], f32_vec: &[f32]) -> f32 {
    use std::arch::x86_64::*;
    let dim = f32_vec.len();
    if q_data.len() < 2 {
        return 0.0;
    }

    let scale = f16_to_f32(u16::from_le_bytes([q_data[0], q_data[1]])).max(1e-7);
    let mut acc_pos = _mm256_setzero_ps();
    let mut acc_neg = _mm256_setzero_ps();

    let codes = &q_data[2..];
    let mut i = 0;
    let mut v_idx = 0;

    // Process 8 bytes (32 ternary elements) per loop.
    // Each byte has 4 elements. 8 bytes * 4 = 32 elements = 1 full AVX2 register of f32.
    while i + 7 < codes.len() && v_idx + 31 < dim {
        // Load 8 bytes of code bits.
        // We need to unpack these 64 bits into 32 elements of 2 bits each.
        let c = _mm_loadu_si128(codes.as_ptr().add(i) as *const __m128i);
        let c_256 = _mm256_cvtepu8_epi16(c); // Unpack 8 bytes to 8 i16s (only 8 bytes used)

        for bit_offset in (0..8).step_by(2) {
            let shift = _mm_cvtsi32_si128(bit_offset as i32);
            let bits = _mm256_and_si256(_mm256_srl_epi16(c_256, shift), _mm256_set1_epi16(0x03));

            // bits: 00 -> -1, 01 -> 0, 10 -> 1
            let mask_pos_i16 = _mm256_cmpeq_epi16(bits, _mm256_set1_epi16(2));
            let mask_neg_i16 = _mm256_cmpeq_epi16(bits, _mm256_set1_epi16(0));

            // Convert i16 masks to f32 masks (32-bit)
            let m_pos_low = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(mask_pos_i16));
            let m_neg_low = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(mask_neg_i16));

            let v = _mm256_loadu_ps(f32_vec.as_ptr().add(v_idx));

            // Multiplication-free accumulation using blending/masking
            acc_pos = _mm256_add_ps(acc_pos, _mm256_and_ps(_mm256_castsi256_ps(m_pos_low), v));
            acc_neg = _mm256_add_ps(acc_neg, _mm256_and_ps(_mm256_castsi256_ps(m_neg_low), v));

            v_idx += 8;
        }
        i += 8;
    }

    let mut f_pos = [0.0f32; 8];
    let mut f_neg = [0.0f32; 8];
    _mm256_storeu_ps(f_pos.as_mut_ptr(), acc_pos);
    _mm256_storeu_ps(f_neg.as_mut_ptr(), acc_neg);

    let mut res: f32 = f_pos.iter().sum::<f32>() - f_neg.iter().sum::<f32>();

    // Scale at the very end to maximize precision and minimize ops
    res *= scale;

    // Remaining elements
    res += dot_product_ternary_scalar(&q_data[i + 2..], &f32_vec[v_idx..]);
    res
}

pub fn cosine_int4_f32(codes: &[u8], f32_vec: &[f32], min_vals: &[f32], max_vals: &[f32]) -> f32 {
    let dot = dot_product_int4_f32(codes, f32_vec, min_vals, max_vals);
    let norm_q = compute_norm_int4(codes, min_vals, max_vals);
    let norm_v = compute_norm_f32(f32_vec);
    dot / (norm_q * norm_v + 1e-8)
}

fn compute_norm_int4(codes: &[u8], min_vals: &[f32], max_vals: &[f32]) -> f32 {
    let mut sum_sq = 0.0;
    let dim = min_vals.len();
    for (i, &byte) in codes.iter().enumerate() {
        let q1 = (byte & 0x0F) as f32;
        let q2 = ((byte >> 4) & 0x0F) as f32;
        let (d1, d2) = (i * 2, i * 2 + 1);
        if d1 < dim {
            let v1 = q1 / 15.0 * (max_vals[d1] - min_vals[d1]).max(1e-6) + min_vals[d1];
            sum_sq += v1 * v1;
        }
        if d2 < dim {
            let v2 = q2 / 15.0 * (max_vals[d2] - min_vals[d2]).max(1e-6) + min_vals[d2];
            sum_sq += v2 * v2;
        }
    }
    sum_sq.sqrt().max(1e-8)
}

fn compute_norm_f32(vec: &[f32]) -> f32 {
    vec.iter().map(|&v| v * v).sum::<f32>().sqrt().max(1e-8)
}

pub fn quantize_int4(src: &[f32], min_vals: &[f32], max_vals: &[f32]) -> Vec<u8> {
    let dim = src.len();
    let mut codes = Vec::with_capacity(dim / 2 + dim % 2);
    for i in (0..dim).step_by(2) {
        let r1 = (max_vals[i] - min_vals[i]).max(1e-6);
        let q1 = (((src[i] - min_vals[i]) / r1).clamp(0.0, 1.0) * 15.0).round() as u8;

        let q2 = if i + 1 < dim {
            let r2 = (max_vals[i + 1] - min_vals[i + 1]).max(1e-6);
            (((src[i + 1] - min_vals[i + 1]) / r2).clamp(0.0, 1.0) * 15.0).round() as u8
        } else {
            0
        };
        codes.push((q2 << 4) | q1);
    }
    codes
}

pub fn quantize_ternary(src: &[f32]) -> Vec<u8> {
    let dim = src.len();
    let mut abs_sum = 0.0;
    for &v in src {
        abs_sum += v.abs();
    }
    let scale = abs_sum / dim as f32;

    let mut codes = Vec::with_capacity(2 + dim / 4 + (if dim % 4 != 0 { 1 } else { 0 }));
    let scale_bits = f16_to_f32_bits(scale); // Using internal conversion
    codes.extend_from_slice(&scale_bits.to_le_bytes());

    for i in (0..dim).step_by(4) {
        let mut byte = 0u8;
        for j in 0..4 {
            if i + j < dim {
                let v = src[i + j];
                let q = if scale > 0.0 {
                    (v / scale).round().clamp(-1.0, 1.0) as i8
                } else {
                    0
                };
                let bits = (q + 1) as u8; // {-1,0,1} -> {0,1,2}
                byte |= bits << (j * 2);
            }
        }
        codes.push(byte);
    }
    codes
}

pub fn f16_to_f32_bits(val: f32) -> u16 {
    let bits = val.to_bits();
    let sign = (bits >> 31) & 1;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let mant = bits & 0x7FFFFF;
    let mut res_exp = exp - 127 + 15;
    let mut res_mant = mant >> 13;
    if res_exp <= 0 {
        res_exp = 0;
        res_mant = 0;
    } else if res_exp >= 31 {
        res_exp = 31;
        res_mant = 0;
    }
    ((sign << 15) | ((res_exp as u32) << 10) | (res_mant as u32)) as u16
}

pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = (bits >> 15) & 1;
    let exp = (bits >> 10) & 0x1F;
    let mant = bits & 0x03FF;

    if exp == 0 {
        if mant == 0 {
            return if sign == 0 { 0.0 } else { -0.0 };
        }
        return (if sign == 0 { 1.0 } else { -1.0 }) * (mant as f32) * 2.0f32.powi(-24);
    } else if exp == 0x1F {
        return if mant == 0 { f32::INFINITY } else { f32::NAN };
    }

    (if sign == 0 { 1.0 } else { -1.0 })
        * ((1 << 10 | mant) as f32)
        * 2.0f32.powi(exp as i32 - 15 - 10)
}

pub fn fp16_bytes_to_f32(src: &[u8]) -> Vec<f32> {
    src.chunks_exact(2)
        .map(|c| f16_to_f32(u16::from_le_bytes([c[0], c[1]])))
        .collect()
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,avx512vl")]
pub unsafe fn dot_product_int4_avx512(
    codes: &[u8],
    f32_vec: &[f32],
    min_vals: &[f32],
    max_vals: &[f32],
) -> f32 {
    use std::arch::x86_64::*;
    let dim = f32_vec.len();
    let mut sum_vec = _mm512_setzero_ps();
    let low_bits_mask = _mm512_set1_epi8(0x0F);
    let inv_15 = _mm512_set1_ps(1.0 / 15.0);

    let mut i = 0;
    // Process 32 bytes of codes = 64 elements of INT4
    while i + 31 < codes.len() && (i * 2 + 63) < dim {
        let chunk = _mm256_loadu_si256(codes.as_ptr().add(i) as *const __m256i);
        let chunk_512 = _mm512_cvtepu8_epi16(chunk);

        let q1_i16 = _mm512_and_si512(chunk_512, low_bits_mask);
        let q2_i16 = _mm512_and_si512(_mm512_srli_epi16(chunk_512, 4), low_bits_mask);

        // Interleave q1 and q2 to get linear sequence
        let q_linear_low = _mm512_unpacklo_epi16(q1_i16, q2_i16);
        let q_linear_high = _mm512_unpackhi_epi16(q1_i16, q2_i16);

        let q_quads = [
            _mm512_extracti64x4_epi64(q_linear_low, 0),
            _mm512_extracti64x4_epi64(q_linear_low, 1),
            _mm512_extracti64x4_epi64(q_linear_high, 0),
            _mm512_extracti64x4_epi64(q_linear_high, 1),
        ];

        for (q_idx, &q_128) in q_quads.iter().enumerate() {
            let offset = i * 2 + q_idx * 16;
            // Unpack 16 i16 to 16 f32 in a single 512-bit register
            let q_f = _mm512_cvtepi32_ps(_mm512_cvtepi16_epi32(q_128));

            let min_v = _mm512_loadu_ps(min_vals.as_ptr().add(offset));
            let max_v = _mm512_loadu_ps(max_vals.as_ptr().add(offset));
            let query_v = _mm512_loadu_ps(f32_vec.as_ptr().add(offset));

            let range = _mm512_sub_ps(max_v, min_v);
            let scale = _mm512_mul_ps(range, inv_15);

            let decoded = _mm512_fmadd_ps(q_f, scale, min_v);
            sum_vec = _mm512_fmadd_ps(decoded, query_v, sum_vec);
        }
        i += 32;
    }

    let mut sum = _mm512_reduce_add_ps(sum_vec);
    sum += dot_product_int4_scalar(
        &codes[i..],
        &f32_vec[i * 2..],
        &min_vals[i * 2..],
        &max_vals[i * 2..],
    );
    sum
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512vpopcntdq,avx512bw")]
pub unsafe fn dot_product_ternary_avx512(q_data: &[u8], f32_vec: &[f32]) -> f32 {
    use std::arch::x86_64::*;
    let dim = f32_vec.len();
    if q_data.len() < 2 {
        return 0.0;
    }
    let scale = f16_to_f32(u16::from_le_bytes([q_data[0], q_data[1]])).max(1e-7);

    let mut acc_pos = _mm512_setzero_ps();
    let mut acc_neg = _mm512_setzero_ps();
    let codes = &q_data[2..];
    let mut v_idx = 0;
    let mut i = 0;

    while i + 31 < codes.len() && v_idx + 127 < dim {
        let c_256 = _mm256_loadu_si256(codes.as_ptr().add(i) as *const __m256i);
        let c_512 = _mm512_cvtepu8_epi16(c_256);

        for bit_offset in (0..8).step_by(2) {
            let shift = _mm_cvtsi32_si128(bit_offset as i32);
            let bits = _mm512_and_si512(_mm512_srl_epi16(c_512, shift), _mm512_set1_epi16(0x03));

            let mask_pos_all = _mm512_cmpeq_epi16_mask(bits, _mm512_set1_epi16(2));
            let mask_neg_all = _mm512_cmpeq_epi16_mask(bits, _mm512_set1_epi16(0));

            // Split 32-bit mask into two 16-bit masks for two 16-f32 operations
            let m_pos_l = (mask_pos_all & 0xFFFF) as u16;
            let m_neg_l = (mask_neg_all & 0xFFFF) as u16;
            let m_pos_h = (mask_pos_all >> 16) as u16;
            let m_neg_h = (mask_neg_all >> 16) as u16;

            let v_l = _mm512_loadu_ps(f32_vec.as_ptr().add(v_idx));
            acc_pos = _mm512_mask_add_ps(acc_pos, m_pos_l, acc_pos, v_l);
            acc_neg = _mm512_mask_add_ps(acc_neg, m_neg_l, acc_neg, v_l);

            let v_h = _mm512_loadu_ps(f32_vec.as_ptr().add(v_idx + 16));
            acc_pos = _mm512_mask_add_ps(acc_pos, m_pos_h, acc_pos, v_h);
            acc_neg = _mm512_mask_add_ps(acc_neg, m_neg_h, acc_neg, v_h);
            v_idx += 32;
        }
        i += 32;
    }

    let res = (_mm512_reduce_add_ps(acc_pos) - _mm512_reduce_add_ps(acc_neg)) * scale;
    res + dot_product_ternary_scalar(&q_data[i + 2..], &f32_vec[v_idx..])
}

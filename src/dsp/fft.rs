use crate::dsp::spectrum::SharedPipe;
use once_cell::sync::Lazy;
use rustfft::{Fft, num_complex::Complex};
use std::sync::Mutex;

const FFT_SIZE: usize = 4096;
const BANDS: usize = 64;

static BAND_INDEX_CACHE: Lazy<Mutex<Vec<(usize, usize)>>> = Lazy::new(|| Mutex::new(Vec::new()));
static BAND_GAINS_CACHE: Lazy<Mutex<Vec<f32>>> = Lazy::new(|| Mutex::new(Vec::new()));
#[inline(always)]
fn compute_magnitudes(spectrum: &[Complex<f32>], start_idx: usize, end_idx: usize) -> f32 {
    let mut sum_squares = 0.0f32;
    let iter_end = start_idx + ((end_idx - start_idx) / 4) * 4;
    for i in (start_idx..iter_end).step_by(4) {
        let mag_sq1 = spectrum[i].re * spectrum[i].re + spectrum[i].im * spectrum[i].im;
        let mag_sq2 =
            spectrum[i + 1].re * spectrum[i + 1].re + spectrum[i + 1].im * spectrum[i + 1].im;
        let mag_sq3 =
            spectrum[i + 2].re * spectrum[i + 2].re + spectrum[i + 2].im * spectrum[i + 2].im;
        let mag_sq4 =
            spectrum[i + 3].re * spectrum[i + 3].re + spectrum[i + 3].im * spectrum[i + 3].im;
        sum_squares += mag_sq1 + mag_sq2 + mag_sq3 + mag_sq4;
    }
    for i in iter_end..end_idx {
        let mag_sq = spectrum[i].re * spectrum[i].re + spectrum[i].im * spectrum[i].im;
        sum_squares += mag_sq;
    }
    sum_squares
}
fn init_band_indices_cache() {
    let mut cache = BAND_INDEX_CACHE.lock().unwrap();
    if !cache.is_empty() {
        return;
    }
    let sample_rate = 48000.0;
    let freq_resolution = sample_rate / FFT_SIZE as f32;
    let min_freq: f32 = 20.0;
    let max_freq: f32 = 20000.0;
    let log_min = min_freq.log10();
    let log_max = max_freq.log10();
    let log_range = log_max - log_min;
    for i in 0..BANDS {
        let log_pos = log_min + log_range * (i as f32 / BANDS as f32);
        let freq_start = 10_f32.powf(log_pos);
        let log_pos_end = log_min + log_range * ((i + 1) as f32 / BANDS as f32);
        let freq_end = 10_f32.powf(log_pos_end);
        let start_idx = (freq_start / freq_resolution) as usize;
        let end_idx = (freq_end / freq_resolution) as usize;
        let start_idx = start_idx.max(1).min(FFT_SIZE / 2 - 1);
        let end_idx = end_idx.max(start_idx + 1).min(FFT_SIZE / 2);
        cache.push((start_idx, end_idx));
    }
    let mut gains_cache = BAND_GAINS_CACHE.lock().unwrap();
    *gains_cache = vec![1.0; BANDS];
}
pub fn run_fft(
    samples: &mut [f32],
    fft_input: &mut [Complex<f32>],
    fft: &dyn Fft<f32>,
    spectrum_pipe: &SharedPipe,
) {
    if BAND_INDEX_CACHE.lock().unwrap().is_empty() {
        init_band_indices_cache();
    }
    let band_index = BAND_INDEX_CACHE.lock().unwrap();
    let band_gains = BAND_GAINS_CACHE.lock().unwrap();
    let samples_len = samples.len().min(FFT_SIZE);
    let mut windowed_samples = vec![0.0f32; samples_len];
    let chunks = samples_len / 8;
    for chunk in 0..chunks {
        let base = chunk * 8;
        windowed_samples[base] = samples[base];
        windowed_samples[base + 1] = samples[base + 1];
        windowed_samples[base + 2] = samples[base + 2];
        windowed_samples[base + 3] = samples[base + 3];
        windowed_samples[base + 4] = samples[base + 4];
        windowed_samples[base + 5] = samples[base + 5];
        windowed_samples[base + 6] = samples[base + 6];
        windowed_samples[base + 7] = samples[base + 7];
    }
    for i in 0..samples_len {
        fft_input[i].re = windowed_samples[i];
        fft_input[i].im = 0.0;
    }
    for i in samples_len..FFT_SIZE {
        fft_input[i].re = 0.0;
        fft_input[i].im = 0.0;
    }
    fft.process(fft_input);
    let spectrum = &fft_input[..FFT_SIZE / 2];
    let mut bands = vec![0.0f32; BANDS];
    {
        for i in 0..BANDS {
            let (start_idx, end_idx) = band_index[i];
            if start_idx >= end_idx {
                continue;
            }
            let sum_squares = compute_magnitudes(spectrum, start_idx, end_idx);
            let count = (end_idx - start_idx) as f32;
            if count > 0.0 {
                bands[i] = (sum_squares / count).sqrt() * band_gains[i];
            }
        }
    }
    apply_band_gain_compensation(&mut bands);
    improved_normalize_spectrum(&mut bands);
    spectrum_pipe.write(&bands)
}

fn apply_band_gain_compensation(bands: &mut [f32]) {
    let bands_len = bands.len();
    for (i, band) in bands.iter_mut().enumerate() {
        let freq_ratio = i as f32 / bands_len as f32; // 归一化频率位置 [0,1]

        if freq_ratio < 0.3 {
            // 低频衰减：防止低频过强，衰减系数随频率增加而减小
            let attenuation = 1.0 - (0.3 - freq_ratio) * 2.0;
            *band *= attenuation; // 最小衰减到50%
        } else if freq_ratio > 0.8 {
            // 高频增强：提升高频可见度，增强系数随频率增加而增大
            let boost = 1.0 + (freq_ratio - 0.8) * 1.5;
            *band *= boost; // 最大增强到150%
        }
        // 中频段(30%-80%)保持原值不变
    }
}

fn improved_normalize_spectrum(bands: &mut [f32]) {
    // 步骤1: 创建副本并排序以找到稳健的参考值
    let mut sorted_bands = bands.to_vec();
    sorted_bands.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // 步骤2: 使用95百分位数作为参考值（排除极值影响）
    let percentile_95_idx = (sorted_bands.len() as f32 * 0.95) as usize;
    let reference_value = sorted_bands[percentile_95_idx.max(1)].max(1e-6); // 防止除零错误

    // 步骤3: 对每个频段进行归一化和增强处理
    for band in bands.iter_mut() {
        let normalized = (*band / reference_value).clamp(0.0, 1.0); // 归一化到[0,1]范围
        *band = s_curve_enhancement(normalized); // 应用S型曲线增强
    }
}

fn s_curve_enhancement(x: f32) -> f32 {
    if x < 0.1 {
        // 低值区域：轻微压缩
        x * 0.1
    } else if x > 0.9 {
        // 高值区域：显著增强
        0.9 + (x - 0.9) * 5.0
    } else {
        // 中值区域：二次曲线增强
        let t = (x - 0.1) / 0.8;
        0.01 + 0.98 * t.powf(2.0)
    }
}

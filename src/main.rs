//! 代码注释由 Alibaba Lingma 服务生成，仅供参考，请以代码实际意图为准。
//! 因项目仍处于开发阶段，部分警告尚未消除。
mod audio;
mod dsp;
mod viz;

use crate::audio::capture::capture;
use crate::dsp::spectrum::SharedPipe;
use crate::viz::viz::run;
use anyhow::Error;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::slice::from_raw_parts;
use std::sync::Arc;
use std::time::Duration;
use std::{ptr, thread};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, WAVEFORMATEX, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};

// FFT配置常量
const FFT_SIZE: usize = 1024; // 更小的FFT窗口提供更好的时间分辨率
const BANDS: usize = 64; // 频段数量，影响可视化精度

fn main() {
    // 创建共享数据管道
    let spectrum = SharedPipe::new();
    let audio_spectrum = spectrum.clone();

    // 启动真实的音频捕获和处理线程
    std::thread::spawn(move || {
        match crate::audio::capture::capture() {
            Ok(capture_client) => {
                println!("capture ready");

                // 初始化FFT相关组件
                let mut planner = FftPlanner::new();
                let fft = planner.plan_fft_forward(FFT_SIZE);
                let mut samples = vec![0.0f32; FFT_SIZE];
                let mut fft_input = vec![Complex::new(0.0, 0.0); FFT_SIZE];

                loop {
                    // 从音频捕获客户端获取数据包大小
                    match unsafe { capture_client.GetNextPacketSize() } {
                        Ok(packet_length) => {
                            if packet_length > 0 {
                                // 获取音频缓冲区
                                let mut data_ptr: *mut u8 = std::ptr::null_mut();
                                let mut num_frames: u32 = 0;
                                let mut flags: u32 = 0;

                                match unsafe {
                                    capture_client.GetBuffer(
                                        &mut data_ptr,
                                        &mut num_frames,
                                        &mut flags,
                                        None,
                                        None,
                                    )
                                } {
                                    Ok(_) => {
                                        // 检查是否为静音数据
                                        if (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) == 0 {
                                            // 将原始音频数据转换为f32样本
                                            let raw_samples: &[f32] = unsafe {
                                                std::slice::from_raw_parts(
                                                    data_ptr as *const f32,
                                                    (num_frames * 2) as usize, // 假设立体声
                                                )
                                            };

                                            // 将新样本添加到缓冲区（简单的循环缓冲区）
                                            for (i, &sample) in
                                                raw_samples.iter().enumerate().take(FFT_SIZE)
                                            {
                                                samples[i] = sample;
                                            }

                                            // 执行FFT分析
                                            run_fft(
                                                &mut samples,
                                                &mut fft_input,
                                                &*fft,
                                                &audio_spectrum,
                                            );
                                        }

                                        // 释放缓冲区
                                        let _ = unsafe { capture_client.ReleaseBuffer(num_frames) };
                                    }
                                    Err(e) => {
                                        eprintln!("获取音频缓冲区失败: {:?}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("获取数据包大小失败: {:?}", e);
                        }
                    }

                    // 毫秒级采样以实现即时响应
                    std::thread::sleep(std::time::Duration::from_micros(500)); // 0.5ms间隔
                }
            }
            Err(e) => {
                eprintln!("Capture failed: {:?}", e);
                // 如果音频捕获失败，回退到测试数据
            }
        }
    });

    // 启动可视化线程
    run(spectrum);
}

fn run_fft(
    samples: &mut [f32],
    fft_input: &mut [Complex<f32>],
    fft: &dyn Fft<f32>,
    spectrum_pipe: &SharedPipe,
) {
    // 将实数样本转换为复数格式（虚部为0）
    for i in 0..FFT_SIZE {
        fft_input[i].re = samples[i];
        fft_input[i].im = 0.0;
    }

    // 应用汉宁窗减少频谱泄漏
    apply_hanning_window(fft_input);

    // 执行FFT变换
    fft.process(fft_input);

    // 取FFT结果的前半部分（正频率部分）
    let spectrum = &fft_input[..FFT_SIZE / 2];
    let sample_rate = 48000.0; // 假设采样率为48kHz
    let freq_resolution = sample_rate / FFT_SIZE as f32;

    // 使用对数刻度分配频段（更好地匹配人耳感知）
    let mut bands = vec![0.0f32; BANDS];
    let min_freq: f32 = 20.0; // 最低可听频率
    let max_freq: f32 = 20000.0; // 最高可听频率

    for i in 0..BANDS {
        // 计算当前频段的频率范围（对数刻度）
        let log_min = min_freq.log10();
        let log_max = max_freq.log10();
        let log_pos = log_min + (log_max - log_min) * (i as f32 / BANDS as f32);
        let freq_start = 10_f32.powf(log_pos);

        let log_pos_end = log_min + (log_max - log_min) * ((i + 1) as f32 / BANDS as f32);
        let freq_end = 10_f32.powf(log_pos_end);

        // 找到对应的FFT索引范围
        let start_idx = (freq_start / freq_resolution) as usize;
        let end_idx = (freq_end / freq_resolution) as usize;

        // 确保索引在有效范围内
        let start_idx = start_idx.max(1).min(spectrum.len() - 1);
        let end_idx = end_idx.max(start_idx + 1).min(spectrum.len());

        // 计算该频段的能量（RMS）
        let mut sum_squares = 0.0;
        let mut count = 0;
        for idx in start_idx..end_idx {
            let magnitude =
                (spectrum[idx].re * spectrum[idx].re + spectrum[idx].im * spectrum[idx].im).sqrt();
            sum_squares += magnitude * magnitude;
            count += 1;
        }

        if count > 0 {
            let rms = (sum_squares / count as f32).sqrt();
            bands[i] = rms;
        }
    }

    // 应用频段增益补偿（修正频率响应不平衡）
    apply_band_gain_compensation(&mut bands);

    // 对频谱数据进行改进的归一化处理
    improved_normalize_spectrum(&mut bands);

    // 写入共享管道供可视化使用
    spectrum_pipe.write(&bands);
}

fn apply_hanning_window(fft_input: &mut [Complex<f32>]) {
    let len = fft_input.len();
    for (i, sample) in fft_input.iter_mut().enumerate() {
        let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / len as f32).cos());
        sample.re *= window;
        // 虚部通常为0，但为了完整性也处理
        sample.im *= window;
    }
}

fn apply_band_gain_compensation(bands: &mut [f32]) {
    let bands_len = bands.len();
    // 针对不同频段应用不同的增益补偿
    // 低频衰减，高频适当提升
    for (i, band) in bands.iter_mut().enumerate() {
        let freq_ratio = i as f32 / bands_len as f32;

        // 低频衰减曲线：在前30%频段逐渐衰减
        if freq_ratio < 0.3 {
            let attenuation = 1.0 - (0.3 - freq_ratio) * 2.0; // 最大衰减50%
            *band *= attenuation.max(0.5);
        }
        // 高频轻微提升：在后20%频段轻微提升
        else if freq_ratio > 0.8 {
            let boost = 1.0 + (freq_ratio - 0.8) * 1.5; // 最大提升50%
            *band *= boost.min(1.5);
        }
    }
}

fn improved_normalize_spectrum(bands: &mut [f32]) {
    // 使用分位数归一化而不是全局最大值
    let mut sorted_bands = bands.to_vec();
    sorted_bands.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // 取95%分位数作为参考值（排除极端峰值）
    let percentile_95_idx = (sorted_bands.len() as f32 * 0.95) as usize;
    let reference_value = sorted_bands[percentile_95_idx.max(1)].max(1e-6);

    // 归一化处理
    for band in bands.iter_mut() {
        let normalized = (*band / reference_value).clamp(0.0, 1.0);
        // 应用S形曲线增强对比度
        *band = s_curve_enhancement(normalized);
    }
}

fn s_curve_enhancement(x: f32) -> f32 {
    // 使用硬限幅实现果断的视觉效果
    if x < 0.1 {
        x * 0.1 // 极度压缩背景噪声
    } else if x > 0.9 {
        0.9 + (x - 0.9) * 5.0 // 极度放大峰值
    } else {
        // 中间区域使用更陡峭的幂函数
        let t = (x - 0.1) / 0.8;
        0.01 + 0.98 * t.powf(2.0) // 二次幂增强对比度
    }
}

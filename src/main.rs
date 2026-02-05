//! 绝区零音乐可视化器主程序
//!
//! 基于Windows Core Audio API实现音频捕获，并使用FFT算法进行频谱分析，
//! 最终以字符画形式显示音频频谱
//! 代码注释由 Alibaba Lingma 服务生成，仅供参考，请以代码实际意图为准。
//! 因项目仍处于开发阶段，部分警告尚未消除。
mod audio;

use crate::audio::capture::capture;
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
const FFT_SIZE: usize = 1024; // FFT大小，影响频率分辨率
const BANDS: usize = 32; // 频段数量，影响可视化精度

/// 主函数
///
/// # 程序流程
/// 1. 初始化音频捕获系统
/// 2. 设置FFT分析器
/// 3. 进入主循环持续捕获和分析音频
/// 4. 实时显示频谱可视化
fn main() -> Result<(), Error> {
    // 初始化音频捕获客户端
    let capture_client = &capture();
    println!("Successfully leave function capture");

    // 原型验证 - 波形分形 Demo
    unsafe {
        // 初始化FFT规划器
        let mut planner: FftPlanner<f32> = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE); // 创建正向FFT计划
        let mut fft_input: Vec<Complex<f32>> = vec![Complex::ZERO; FFT_SIZE]; // FFT输入缓冲区
        let mut sample_buffer: Vec<f32> = Vec::with_capacity(FFT_SIZE); // 音频样本缓冲区

        // 重新获取音频设备信息（这部分代码可能存在冗余）
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device: IMMDevice = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let audio_client: IAudioClient = device.Activate::<IAudioClient>(CLSCTX_ALL, None)?;
        let mut pwfx: *mut WAVEFORMATEX = audio_client.GetMixFormat()?;
        let format = *pwfx; // 获取音频格式信息

        println!("STAGE 3: Demo Visualizer");

        // 主循环：持续捕获和处理音频数据
        loop {
            // 获取下一个音频数据包的大小
            let mut packet_length = capture_client.as_ref().unwrap().GetNextPacketSize()?;

            // 处理所有可用的音频数据包
            while packet_length > 0 {
                // 声明变量用于接收音频数据
                let mut data_ptr: *mut u8 = ptr::null_mut(); // 音频数据指针
                let mut num_frames = 0; // 音频帧数
                let mut flags = 0; // 状态标志

                // 获取音频缓冲区数据
                capture_client.as_ref().unwrap().GetBuffer(
                    &mut data_ptr,
                    &mut num_frames,
                    &mut flags,
                    None, // 时间戳参数，不使用
                    None, // QPC位置参数，不使用
                )?;

                // 检查数据有效性：非静音且指针非空
                if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) == 0 && !data_ptr.is_null() {
                    // 将原始字节数据转换为f32样本
                    let samples = from_raw_parts(
                        data_ptr as *const f32,
                        (num_frames * format.nChannels as u32) as usize,
                    );

                    // 处理每个音频帧，提取左声道数据
                    for frame in samples.chunks(format.nChannels as usize) {
                        sample_buffer.push(frame[0]); // 只取第一个声道

                        // 当缓冲区满时进行FFT分析
                        if sample_buffer.len() >= FFT_SIZE {
                            run_fft(&mut sample_buffer, &mut fft_input, fft.as_ref());
                            sample_buffer.clear(); // 清空缓冲区准备下次分析
                        }
                    }
                }

                // 释放音频缓冲区
                capture_client.as_ref().unwrap().ReleaseBuffer(num_frames)?;

                // 检查是否还有更多数据包
                packet_length = capture_client.as_ref().unwrap().GetNextPacketSize()?;
            }

            // 短暂休眠避免过度占用CPU
            thread::sleep(Duration::from_millis(5));
        }
    }
}

/// 执行FFT频谱分析
///
/// # 参数说明
/// - `samples`: 输入的音频样本数据
/// - `fft_input`: FFT计算的复数输入缓冲区
/// - `fft`: FFT变换器实例
///
/// # 处理流程
/// 1. 将实数样本填充到复数输入缓冲区
/// 2. 执行FFT变换
/// 3. 计算各频段的能量平均值
/// 4. 调用打印函数显示频谱
fn run_fft(samples: &mut [f32], fft_input: &mut [Complex<f32>], fft: &dyn rustfft::Fft<f32>) {
    // 将实数样本转换为复数格式（虚部为0）
    for i in 0..FFT_SIZE {
        fft_input[i].re = samples[i];
        fft_input[i].im = 0.0;
    }

    // 执行FFT变换
    fft.process(fft_input);

    // 取FFT结果的前半部分（正频率部分）
    let spectrum = &fft_input[..FFT_SIZE / 2];
    let band_size = spectrum.len() / BANDS; // 每个频段包含的频率点数

    // 计算各频段的能量
    let mut bands = vec![0.0f32; BANDS];
    for i in 0..BANDS {
        let start = i * band_size;
        let end = start + band_size;

        // 计算该频段内所有频率点的幅度平均值
        let mut sum = 0.0;
        for c in &spectrum[start..end] {
            sum += (c.re * c.re + c.im * c.im).sqrt(); // 复数幅度计算
        }
        bands[i] = sum / band_size as f32;
    }

    // 显示频谱可视化
    print_spectrum(&bands);
}

/// 打印频谱可视化
///
/// 使用字符画方式显示音频频谱，高度表示能量强度
///
/// # 可视化字符
/// ▁▂▃▄▅▆▇█ 从低到高的能量等级
fn print_spectrum(bands: &[f32]) {
    const LEVELS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    // 找到最大值用于归一化（避免除零）
    let max = bands.iter().cloned().fold(0.0, f32::max).max(1e-6);

    // 为每个频段选择合适的可视化字符
    for &v in bands {
        let norm = (v / max).clamp(0.0, 1.0); // 归一化到[0,1]范围
        let idx = (norm * (LEVELS.len() - 1) as f32) as usize; // 映射到字符索引
        print!("{}", LEVELS[idx]);
    }
    println!(); // 换行
}

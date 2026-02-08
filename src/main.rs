//! 绝区零音乐可视化器主程序
//!
//! 该程序实现了类似游戏《绝区零》中音乐播放器的音频可视化效果
//! 主要功能包括：音频捕获、频谱分析、实时渲染
//!
//! 程序架构：
//! - 音频模块：负责Windows系统音频捕获
//! - DSP模块：处理音频信号的频谱分析
//! - 可视化模块：使用WGPU进行实时图形渲染
//!
//! 现阶段所有文档注释均由 Alibaba Lingma 服务生成，将在 0.1.1 Release 版本发布时对代码进行规范，请以代码实际意图为准。。

// 声明模块
mod audio; // 音频捕获模块
mod dsp; // 数字信号处理模块
mod viz; // 可视化渲染模块
// 导入必要的模块和类型
use crate::dsp::spectrum::SharedPipe; // 频谱数据共享管道
use crate::viz::viz::run; // 可视化渲染入口函数
use rustfft::{Fft, FftPlanner, num_complex::Complex}; // FFT计算相关
use windows::Win32::Media::Audio::AUDCLNT_BUFFERFLAGS_SILENT; // 音频静音标志

// 全局常量定义
const FFT_SIZE: usize = 2048; // FFT计算的采样点数，影响频率分辨率
const BANDS: usize = 64; // 频谱分析的频段数量
/// 程序主入口函数
///
/// 程序采用双线程架构：
/// 1. 音频处理线程：负责音频捕获和频谱分析
/// 2. 渲染主线程：负责图形界面和可视化渲染
fn main() {
    // 创建频谱数据共享管道，用于线程间通信
    let spectrum = SharedPipe::new();
    let audio_spectrum = spectrum.clone(); // 克隆句柄供音频线程使用

    // 启动音频处理线程
    std::thread::spawn(move || {
        // 尝试初始化音频捕获
        match audio::capture::capture() {
            Ok(capture_client) => {
                println!("音频捕获初始化成功");

                // 初始化FFT规划器和相关缓冲区
                let mut planner = FftPlanner::new(); // FFT规划器
                let fft = planner.plan_fft_forward(FFT_SIZE); // 前向FFT计划
                let mut samples = vec![0.0f32; FFT_SIZE]; // 音频采样缓冲区
                let mut fft_input = vec![Complex::new(0.0, 0.0); FFT_SIZE]; // FFT输入缓冲区
                // 音频处理主循环
                loop {
                    // 检查是否有新的音频数据包
                    match unsafe { capture_client.GetNextPacketSize() } {
                        Ok(packet_length) => {
                            // 如果有数据需要处理
                            if packet_length > 0 {
                                // 准备接收音频数据的变量
                                let mut data_ptr: *mut u8 = std::ptr::null_mut(); // 数据指针
                                let mut num_frames: u32 = 0; // 帧数
                                let mut flags: u32 = 0; // 状态标志
                                // 获取音频缓冲区数据
                                match unsafe {
                                    capture_client.GetBuffer(
                                        &mut data_ptr,   // 输出数据指针
                                        &mut num_frames, // 输出帧数
                                        &mut flags,      // 输出状态标志
                                        None,            // 不需要时间戳
                                        None,            // 不需要设备位置
                                    )
                                } {
                                    Ok(_) => {
                                        // 检查是否为有效音频数据（非静音）
                                        if (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) == 0 {
                                            // 将原始字节数据转换为浮点数采样数据
                                            let raw_samples: &[f32] = unsafe {
                                                std::slice::from_raw_parts(
                                                    data_ptr as *const f32,    // 强制转换为f32指针
                                                    (num_frames * 2) as usize, // 立体声数据，每帧2个样本
                                                )
                                            };
                                            // 将原始采样数据复制到处理缓冲区
                                            for (i, &sample) in
                                                raw_samples.iter().enumerate().take(FFT_SIZE)
                                            {
                                                samples[i] = sample;
                                            }
                                            // 执行频谱分析
                                            run_fft(
                                                &mut samples,    // 输入采样数据
                                                &mut fft_input,  // FFT输入缓冲区
                                                &*fft,           // FFT计算计划
                                                &audio_spectrum, // 输出频谱管道
                                            );
                                        }
                                        // 释放音频缓冲区
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
                    // 短暂休眠以控制采样频率
                    std::thread::sleep(std::time::Duration::from_micros(200));
                }
            }
            Err(e) => {
                eprintln!("capture fn failed: {:?}", e);
            }
        }
    });
    run(spectrum);
}
/// 执行FFT频谱分析的核心函数
///
/// 该函数将时域音频信号转换为频域表示，并进行频段划分和后处理
/// 实现了从原始音频采样到可视化频谱数据的完整处理链路
///
/// # 参数
/// * `samples` - 输入的时域音频采样数据，长度为FFT_SIZE
/// * `fft_input` - FFT计算的复数输入缓冲区
/// * `fft` - 预计算的FFT变换计划
/// * `spectrum_pipe` - 输出频谱数据的共享管道
fn run_fft(
    samples: &mut [f32],
    fft_input: &mut [Complex<f32>],
    fft: &dyn Fft<f32>,
    spectrum_pipe: &SharedPipe,
) {
    // 步骤1: 准备FFT输入数据
    // 将实数采样数据转换为复数形式（虚部为0）
    for i in 0..FFT_SIZE {
        fft_input[i].re = samples[i]; // 实部设置为采样值
        fft_input[i].im = 0.0; // 虚部设为0
    }

    // 步骤3: 执行FFT变换，将时域信号转换为频域表示
    fft.process(fft_input);
    // 步骤4: 提取正频率部分的频谱数据
    let spectrum = &fft_input[..FFT_SIZE / 2]; // 只取前半部分（正频率）

    // 步骤5: 计算频率分析相关参数
    let sample_rate = 48000.0; // 采样率48kHz
    let freq_resolution = sample_rate / FFT_SIZE as f32; // 频率分辨率 = 采样率/FFT点数
    let mut bands = vec![0.0f32; BANDS]; // 初始化频段能量存储

    // 定义人耳可听频率范围（20Hz-20kHz）
    let min_freq: f32 = 20.0; // 最低可听频率
    let max_freq: f32 = 20000.0; // 最高可听频率
    // 步骤6: 按对数间隔划分频段（模拟人耳感知特性）
    for i in 0..BANDS {
        // 计算当前频段在对数尺度上的位置
        let log_min = min_freq.log10(); // 最小频率的对数值
        let log_max = max_freq.log10(); // 最大频率的对数值
        let log_pos = log_min + (log_max - log_min) * (i as f32 / BANDS as f32);
        let freq_start = 10_f32.powf(log_pos); // 起始频率（对数反变换）

        // 计算下一个频段的结束频率
        let log_pos_end = log_min + (log_max - log_min) * ((i + 1) as f32 / BANDS as f32);
        let freq_end = 10_f32.powf(log_pos_end); // 结束频率
        // 步骤7: 将频率范围转换为FFT数组索引
        let start_idx = (freq_start / freq_resolution) as usize; // 起始索引
        let end_idx = (freq_end / freq_resolution) as usize; // 结束索引

        // 确保索引在有效范围内，避免边界访问错误
        let start_idx = start_idx.max(1).min(spectrum.len() - 1);
        let end_idx = end_idx.max(start_idx + 1).min(spectrum.len());
        // 步骤8: 计算该频段内所有频率点的能量均方根(RMS)
        let mut sum_squares = 0.0; // 平方和累加器
        let mut count = 0; // 有效点数计数器

        // 遍历当前频段内的所有频率点
        for idx in start_idx..end_idx {
            // 计算复数的幅值（magnitude = √(real² + imaginary²)）
            let magnitude =
                (spectrum[idx].re * spectrum[idx].re + spectrum[idx].im * spectrum[idx].im).sqrt();
            sum_squares += magnitude * magnitude; // 累加平方值
            count += 1;
        }

        // 计算RMS值并存储到对应频段
        if count > 0 {
            let rms = (sum_squares / count as f32).sqrt(); // RMS = √(mean of squares)
            bands[i] = rms;
        }
    }
    // 步骤9: 应用频段增益补偿（校正人耳感知差异）
    apply_band_gain_compensation(&mut bands);

    // 步骤10: 改进的频谱归一化处理
    improved_normalize_spectrum(&mut bands);

    // 步骤11: 将处理后的频谱数据写入共享管道
    spectrum_pipe.write(&bands);
}

/// 应用频段增益补偿
///
/// 根据人耳对不同频率的敏感度差异，对频谱进行增益调整
/// 低频部分适当衰减，高频部分适度增强，使视觉效果更加均衡
/// 这种处理模拟了人耳的等响度曲线特性
///
/// # 参数
/// * `bands` - 各频段的能量值数组
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
/// 改进的频谱归一化处理
///
/// 使用95百分位数作为参考值进行归一化，相比最大值更能抵抗极值干扰
/// 提高了动态范围表现，并应用S型曲线增强对比度
///
/// # 参数
/// * `bands` - 待归一化的频段能量数组
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
/// S型曲线增强函数
///
/// 对归一化后的频谱值应用非线性变换，增强视觉对比度
/// 低值压缩，中值适度放大，高值显著增强
///
/// # 参数
/// * `x` - 输入的归一化值 [0.0, 1.0]
///
/// # 返回值
/// * 增强后的值 [0.0, 1.0]
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

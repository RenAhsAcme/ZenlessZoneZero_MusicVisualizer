//! 音频捕获模块
//! 提供基于Windows Core Audio API的音频数据捕获功能

use anyhow::{Result, anyhow};
use std::ptr;
use std::slice::from_raw_parts;
use std::thread;
use std::time::Duration;
use windows::{
    Win32::{
        Media::Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
            IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
            WAVEFORMATEX, eConsole, eRender,
        },
        System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
    },
    core::HRESULT,
};

/// 音频捕获函数
///
/// # 功能说明
/// 初始化并启动音频捕获客户端，用于捕获系统音频输出（loopback模式）
///
/// # 返回值
/// 成功时返回IAudioCaptureClient实例，可用于获取音频数据
/// 失败时返回错误信息
///
/// # 实现步骤
/// 1. 初始化COM库
/// 2. 获取默认音频输出设备
/// 3. 激活音频客户端
/// 4. 获取音频格式信息
/// 5. 初始化音频客户端为loopback模式
/// 6. 获取捕获客户端并启动捕获
pub fn capture() -> Result<IAudioCaptureClient> {
    unsafe {
        // 初始化COM库，使用多线程模式
        // 这是使用Windows COM API的必要步骤
        let initialize_com: HRESULT = CoInitializeEx(None, COINIT_MULTITHREADED);
        if initialize_com.is_err() {
            return Err(anyhow!("CoInitializeEx failed: {:?}", initialize_com));
        };

        // 创建设备枚举器，用于查找音频设备
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

        // 获取默认的音频渲染设备（通常是扬声器或耳机）
        let device: IMMDevice = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

        // 激活音频客户端接口，用于控制音频流
        let audio_client: IAudioClient = device.Activate::<IAudioClient>(CLSCTX_ALL, None)?;

        // 获取设备支持的最佳音频格式
        let audio_info: *mut WAVEFORMATEX = audio_client.GetMixFormat()?;
        let channels: u16 = (*audio_info).nChannels; // 声道数
        let sample_rate: u32 = (*audio_info).nSamplesPerSec; // 采样率

        // 打印音频设备信息
        println!(
            "STAGE 1: Get device successfully, format is: {} ch, {} Hz",
            channels, sample_rate
        );

        // 初始化音频客户端
        // AUDCLNT_SHAREMODE_SHARED: 共享模式，允许多个应用同时使用音频设备
        // AUDCLNT_STREAMFLAGS_LOOPBACK: 启用loopback模式，捕获输出音频而非输入音频
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            0,          // 延迟时间，0表示使用默认值
            0,          // 周期时间，0表示使用默认值
            audio_info, // 音频格式
            None,       // 会话GUID，None表示使用默认值
        )?;

        // 获取音频捕获客户端，用于实际读取音频数据
        let capture_client: IAudioCaptureClient = audio_client.GetService()?;

        // 启动音频捕获
        audio_client.Start()?;
        println!("STAGE 2: Capture Started.");

        // 返回捕获客户端，供主程序使用
        Ok(capture_client)
        // 原型验证 - 音频捕获接口验证，现已弃用。
        // loop {
        //     let mut packet_length: u32 = capture_client.GetNextPacketSize()?;
        //     while packet_length > 0 {
        //         let mut data_ptr: *mut u8 = ptr::null_mut();
        //         let mut num_frames: u32 = 0;
        //         let mut flags: u32 = 0;
        //         capture_client.GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)?;
        //         if (flags * (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) == 0 {
        //             let samples: &[f32] = from_raw_parts(
        //                 data_ptr as *const f32,
        //                 (num_frames * channels as u32) as usize,
        //             );
        //             let sum: f32 = samples.iter().map(|x: &f32| x * x).sum();
        //             let rms: f32 = (sum / samples.len() as f32).sqrt();
        //             println!("RMS: {}", rms);
        //         }
        //         capture_client.ReleaseBuffer(num_frames)?;
        //         packet_length = capture_client.GetNextPacketSize()?;
        //     }
        //     thread::sleep(Duration::from_millis(10));
        // }
    }
}

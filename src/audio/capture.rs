//! Windows音频捕获模块
//!
//! 该模块实现了基于Windows Core Audio API的系统音频捕获功能
//! 支持捕获系统混音输出（Loopback模式），用于音频可视化

use anyhow::{Result, anyhow};
use windows::{
    Win32::{
        Media::Audio::{
            AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, IAudioCaptureClient,
            IAudioClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
            eConsole, eRender,
        },
        System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
    },
    core::HRESULT,
};
/// 初始化并返回音频捕获客户端
///
/// 完成完整的音频捕获初始化流程
pub fn capture() -> Result<IAudioCaptureClient> {
    unsafe {
        // 初始化COM库，使用多线程模式
        // 这是使用Windows COM API的必要步骤
        let initialize_com: HRESULT = CoInitializeEx(None, COINIT_MULTITHREADED);
        if initialize_com.is_err() {
            return Err(anyhow!("CoInitializeEx failed: {:?}", initialize_com));
        };
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device: IMMDevice = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let audio_client: IAudioClient = device.Activate::<IAudioClient>(CLSCTX_ALL, None)?;
        let audio_info: *mut WAVEFORMATEX = audio_client.GetMixFormat()?;
        let channels: u16 = (*audio_info).nChannels;
        let sample_rate: u32 = (*audio_info).nSamplesPerSec;
        println!(
            "STAGE 1: Get device successfully, format is: {} ch, {} Hz",
            channels, sample_rate
        );
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            0,
            0,
            audio_info,
            None,
        )?;
        let capture_client: IAudioCaptureClient = audio_client.GetService()?;
        audio_client.Start()?;
        println!("STAGE 2: Capture Started.");
        Ok(capture_client)
    }
}

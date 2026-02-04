use std::ptr;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use windows::{
    Win32::{
        Media::Audio::*,
        System::Com::*,
    },
};

fn main() -> Result<()> {
    unsafe {
        // 1️⃣ 初始化 COM（注意：返回 HRESULT）
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if hr.is_err() {
            return Err(anyhow!("CoInitializeEx failed: {:?}", hr));
        }

        // 2️⃣ 获取默认渲染设备
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

        let device = enumerator.GetDefaultAudioEndpoint(
            eRender,
            eConsole,
        )?;

        // 3️⃣ 激活 AudioClient
        let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

        // 4️⃣ 获取混音格式
        let pwfx = audio_client.GetMixFormat()?;
        
        // 安全地访问packed结构体字段
        let channels = (*pwfx).nChannels;
        let sample_rate = (*pwfx).nSamplesPerSec;
        
        println!("🎧 Mix format: {} ch, {} Hz", channels, sample_rate);

        // 5️⃣ 初始化 Loopback
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            0,
            0,
            pwfx,
            None,
        )?;

        // 6️⃣ 获取 CaptureClient
        let capture_client: IAudioCaptureClient =
            audio_client.GetService()?;

        // 7️⃣ 开始捕获
        audio_client.Start()?;
        println!("▶ Loopback capture started");

        loop {
            let mut packet_length = capture_client.GetNextPacketSize()?;

            while packet_length > 0 {
                let mut data_ptr: *mut u8 = ptr::null_mut();
                let mut num_frames: u32 = 0;
                let mut flags: u32 = 0;

                capture_client.GetBuffer(
                    &mut data_ptr,
                    &mut num_frames,
                    &mut flags,
                    None,
                    None,
                )?;

                // flags 是 u32
                if (flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 {
                    // 静音帧，跳过
                } else {
                    let samples = std::slice::from_raw_parts(
                        data_ptr as *const f32,
                        (num_frames * channels as u32) as usize,
                    );

                    let rms = compute_rms(samples);
                    println!("RMS: {:.4}", rms);
                }

                capture_client.ReleaseBuffer(num_frames)?;

                packet_length = capture_client.GetNextPacketSize()?;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn compute_rms(samples: &[f32]) -> f32 {
    let sum: f32 = samples.iter().map(|x| x * x).sum();
    (sum / samples.len() as f32).sqrt()
}

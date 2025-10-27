use crate::{AudioFrame, AudioMuxer, Muxer, TaskPool, VideoMuxer, screen_capture};
use anyhow::{Context, anyhow};
use cap_enc_ffmpeg::AACEncoder;
use cap_media_info::{AudioInfo, VideoInfo};
use futures::channel::oneshot;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::AtomicBool,
        mpsc::{SyncSender, sync_channel},
    },
    time::Duration,
};
use tracing::*;
use windows::{
    Foundation::TimeSpan,
    Graphics::SizeInt32,
    Win32::Graphics::{Direct3D11::ID3D11Device, Dxgi::Common::DXGI_FORMAT},
};

/// Muxes to MP4 using a combination of FFmpeg and Media Foundation
pub struct WindowsMuxer {
    video_tx: SyncSender<Option<(scap_direct3d::Frame, Duration)>>,
    output: Arc<Mutex<ffmpeg::format::context::Output>>,
    audio_encoder: Option<AACEncoder>,
}

pub struct WindowsMuxerConfig {
    pub pixel_format: DXGI_FORMAT,
    pub d3d_device: ID3D11Device,
    pub frame_rate: u32,
    pub bitrate_multiplier: f32,
    pub output_size: Option<SizeInt32>,
    pub encoder_type: Option<String>,
    pub preset: Option<String>,
}

impl Muxer for WindowsMuxer {
    type Config = WindowsMuxerConfig;

    async fn setup(
        config: Self::Config,
        output_path: PathBuf,
        video_config: Option<VideoInfo>,
        audio_config: Option<AudioInfo>,
        _: Arc<AtomicBool>,
        tasks: &mut TaskPool,
    ) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let video_config =
            video_config.ok_or_else(|| anyhow!("invariant: video config expected"))?;
        let input_size = SizeInt32 {
            Width: video_config.width as i32,
            Height: video_config.height as i32,
        };
        let output_size = config.output_size.unwrap_or(input_size);
        let (video_tx, video_rx) = sync_channel::<Option<(scap_direct3d::Frame, Duration)>>(8);

        let mut output = ffmpeg::format::output(&output_path)?;
        let audio_encoder = audio_config
            .map(|config| AACEncoder::init(config, &mut output))
            .transpose()?;

        let output = Arc::new(Mutex::new(output));
        let (ready_tx, ready_rx) = oneshot::channel::<anyhow::Result<()>>();

        {
            let output = output.clone();

            tasks.spawn_thread("windows-encoder", move || {
                cap_mediafoundation_utils::thread_init();

                let encoder_type = config.encoder_type.as_deref().unwrap_or("auto");
                let encoder_type_enum = cap_enc_ffmpeg::H264EncoderType::from_str(encoder_type);
                info!("Encoder selection mode: {} ({:?})", encoder_type, encoder_type_enum);

                let preset = config.preset.as_deref().unwrap_or("medium");
                let preset_enum = cap_enc_ffmpeg::H264Preset::from_str(preset);
                info!("Encoder preset: {} ({:?})", preset, preset_enum);

                let bitrate_multiplier = config.bitrate_multiplier;

                let encoder = (|| {
                    let mut output = output.lock().unwrap();

                    let fallback_width = if output_size.Width > 0 {
                        output_size.Width as u32
                    } else {
                        video_config.width
                    };
                    let fallback_height = if output_size.Height > 0 {
                        output_size.Height as u32
                    } else {
                        video_config.height
                    };

                    // Use FFmpeg encoder with auto hardware detection or user choice
                    cap_enc_ffmpeg::H264Encoder::builder(video_config)
                        .with_encoder_type(encoder_type_enum)
                        .with_preset(preset_enum)
                        .with_bpp(bitrate_multiplier)
                        .with_output_size(fallback_width, fallback_height)
                        .and_then(|builder| builder.build(&mut output))
                        .map_err(|e| anyhow!("H264Encoder/{e}"))
                })();

                let mut encoder = match encoder {
                    Ok(encoder) => {
                        if ready_tx.send(Ok(())).is_err() {
                            error!("Failed to send ready signal - receiver dropped");
                            return Ok(());
                        }
                        encoder
                    }
                    Err(e) => {
                        error!("Encoder setup failed: {:#}", e);
                        let _ = ready_tx.send(Err(anyhow!("{e}")));
                        return Err(anyhow!("{e}"));
                    }
                };

                // Process frames with FFmpeg encoder
                while let Ok(Some((frame, time))) = video_rx.recv() {
                    let Ok(mut output) = output.lock() else {
                        continue;
                    };

                    use scap_ffmpeg::AsFFmpeg;

                    frame
                        .as_ffmpeg()
                        .context("frame as_ffmpeg")
                        .and_then(|frame| {
                            encoder
                                .queue_frame(frame, time, &mut output)
                                .context("queue_frame")
                        })?;
                }

                Ok(())
            });
        }

        ready_rx
            .await
            .map_err(|_| anyhow!("Encoder thread ended unexpectedly"))??;

        output.lock().unwrap().write_header()?;

        Ok(Self {
            video_tx,
            output,
            audio_encoder,
        })
    }

    fn stop(&mut self) {
        let _ = self.video_tx.send(None);
    }

    fn finish(&mut self, _: Duration) -> anyhow::Result<()> {
        let mut output = self.output.lock().unwrap();
        if let Some(audio_encoder) = self.audio_encoder.as_mut() {
            let _ = audio_encoder.finish(&mut output);
        }
        Ok(output.write_trailer()?)
    }
}

impl VideoMuxer for WindowsMuxer {
    type VideoFrame = screen_capture::VideoFrame;

    fn send_video_frame(
        &mut self,
        frame: Self::VideoFrame,
        timestamp: Duration,
    ) -> anyhow::Result<()> {
        Ok(self.video_tx.send(Some((frame.frame, timestamp)))?)
    }
}

impl AudioMuxer for WindowsMuxer {
    fn send_audio_frame(&mut self, frame: AudioFrame, timestamp: Duration) -> anyhow::Result<()> {
        if let Some(encoder) = self.audio_encoder.as_mut()
            && let Ok(mut output) = self.output.lock()
        {
            encoder.send_frame(frame.inner, timestamp, &mut output)?;
        }

        Ok(())
    }
}

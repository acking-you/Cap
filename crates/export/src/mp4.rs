use crate::ExporterBase;
use cap_editor::{AudioRenderer, get_audio_segments};
use cap_enc_ffmpeg::{AACEncoder, AudioEncoder, H264Encoder, H264Preset, MP4File, MP4Input};
use cap_media_info::{RawVideoFormat, VideoInfo};
use cap_project::XY;
use cap_rendering::{ProjectUniforms, RenderSegment, RenderedFrame};
use futures::FutureExt;
use image::ImageBuffer;
use serde::Deserialize;
use specta::Type;
use std::{path::PathBuf, time::Duration};
use tracing::{info, trace, warn};

#[derive(Deserialize, Type, Clone, Copy, Debug)]
pub enum ExportCompression {
    NearLossless,
    HighQuality,
    Minimal,
    Social,
    Web,
    Potato,
    H264Lossless,
    H265Lossless,
}

impl ExportCompression {
    pub fn bits_per_pixel(&self) -> Option<f32> {
        match self {
            Self::NearLossless => Some(2.0),
            Self::HighQuality => Some(0.8),
            Self::Minimal => Some(0.5),
            Self::Social => Some(0.15),
            Self::Web => Some(0.08),
            Self::Potato => Some(0.04),
            Self::H264Lossless | Self::H265Lossless => None,
        }
    }

    pub fn preset(&self) -> H264Preset {
        match self {
            Self::H264Lossless | Self::H265Lossless => H264Preset::Lossless,
            Self::NearLossless | Self::HighQuality => H264Preset::Slow,
            Self::Minimal => H264Preset::Medium,
            _ => H264Preset::Ultrafast,
        }
    }

    pub fn encoder_type(&self) -> Option<cap_enc_ffmpeg::H264EncoderType> {
        match self {
            Self::H264Lossless => Some(cap_enc_ffmpeg::H264EncoderType::Software),
            Self::H265Lossless => Some(cap_enc_ffmpeg::H264EncoderType::Nvenc),
            _ => Some(cap_enc_ffmpeg::H264EncoderType::Auto),
        }
    }
}

#[derive(Deserialize, Type, Clone, Copy, Debug)]
pub struct Mp4ExportSettings {
    pub fps: u32,
    pub resolution_base: XY<u32>,
    pub compression: ExportCompression,
    #[serde(default)]
    pub gpu_async_depth: Option<u32>,
    #[serde(default)]
    pub gpu_delay: Option<i32>,
    #[serde(default)]
    pub gpu_rc_lookahead: Option<u32>,
}

impl Mp4ExportSettings {
    pub async fn export(
        self,
        base: ExporterBase,
        mut on_progress: impl FnMut(u32) + Send + 'static,
    ) -> Result<PathBuf, String> {
        let output_path = base.output_path.clone();
        let meta = &base.studio_meta;

        info!("Exporting mp4 with settings: {:?}", &self);
        info!("Expected to render {} frames", base.total_frames(self.fps));

        if self.gpu_async_depth.is_some() || self.gpu_delay.is_some() || self.gpu_rc_lookahead.is_some() {
            info!("User GPU settings - async_depth: {:?}, delay: {:?}, rc_lookahead: {:?}",
                self.gpu_async_depth, self.gpu_delay, self.gpu_rc_lookahead);
        }

        let (tx_image_data, mut video_rx) = tokio::sync::mpsc::channel::<(RenderedFrame, u32)>(64);
        let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<MP4Input>(64);

        let fps = self.fps;

        let output_size = ProjectUniforms::get_output_size(
            &base.render_constants.options,
            &base.project_config,
            self.resolution_base,
        );

        let mut video_info =
            VideoInfo::from_raw(RawVideoFormat::Rgba, output_size.0, output_size.1, fps);
        video_info.time_base = ffmpeg::Rational::new(1, fps as i32);

        let audio_segments = get_audio_segments(&base.segments);

        let mut audio_renderer = audio_segments
            .first()
            .filter(|_| !base.project_config.audio.mute)
            .map(|_| AudioRenderer::new(audio_segments.clone()));
        let has_audio = audio_renderer.is_some();

        let encoder_thread = tokio::task::spawn_blocking(move || {
            trace!("Creating MP4File encoder");

            let mut encoder = MP4File::init(
                "output",
                base.output_path.clone(),
                |o| {
                    let mut builder = H264Encoder::builder(video_info);

                    if let Some(bpp) = self.compression.bits_per_pixel() {
                        builder = builder.with_bpp(bpp);
                    }

                    builder = builder.with_preset(self.compression.preset());

                    if let Some(encoder_type) = self.compression.encoder_type() {
                        builder = builder.with_encoder_type(encoder_type);

                        if matches!(encoder_type, cap_enc_ffmpeg::H264EncoderType::Nvenc) {
                            let async_depth = self.gpu_async_depth.unwrap_or(32);
                            let delay = self.gpu_delay.unwrap_or(4);
                            let rc_lookahead = self.gpu_rc_lookahead.unwrap_or(16);

                            builder = builder
                                .with_async_depth(async_depth)
                                .with_delay(delay)
                                .with_rc_lookahead(rc_lookahead);
                        }
                    }

                    let result = builder.build(o);

                    if result.is_err() && matches!(self.compression, ExportCompression::H265Lossless) {
                        warn!("NVENC not available, falling back to H.264 software lossless");
                        let fallback_builder = H264Encoder::builder(video_info)
                            .with_preset(H264Preset::Lossless)
                            .with_encoder_type(cap_enc_ffmpeg::H264EncoderType::Software);
                        return fallback_builder.build(o);
                    }

                    result
                },
                |o| {
                    has_audio.then(|| {
                        AACEncoder::init(AudioRenderer::info(), o)
                            .map(|v| v.boxed())
                            .map_err(Into::into)
                    })
                },
            )
            .map_err(|v| v.to_string())?;

            info!("Created MP4File encoder");

            let mut encoded_frames = 0;
            while let Ok(frame) = frame_rx.recv() {
                let _ = encoder.queue_video_frame(
                    frame.video,
                    Duration::from_secs_f32(encoded_frames as f32 / fps as f32),
                );
                encoded_frames += 1;
                if let Some(audio) = frame.audio {
                    let _ = encoder.queue_audio_frame(audio);
                }
            }

            info!("Encoded {encoded_frames} video frames");

            encoder.finish();

            Ok::<_, String>(base.output_path)
        })
        .then(|r| async { r.map_err(|e| e.to_string()).and_then(|v| v) });

        let render_task = tokio::spawn({
            let project = base.project_config.clone();
            let project_path = base.project_path.clone();
            async move {
                let mut frame_count = 0;
                let mut first_frame = None;

                let audio_samples_per_frame =
                    (f64::from(AudioRenderer::SAMPLE_RATE) / f64::from(fps)).ceil() as usize;

                loop {
                    let (frame, frame_number) =
                        match tokio::time::timeout(Duration::from_secs(6), video_rx.recv()).await {
                            Err(_) => {
                                warn!("render_task frame receive timed out");
                                break;
                            }
                            Ok(Some(v)) => v,
                            _ => {
                                break;
                            }
                        };

                    (on_progress)(frame_count);

                    if frame_count == 0 {
                        first_frame = Some(frame.clone());
                        if let Some(audio) = &mut audio_renderer {
                            audio.set_playhead(0.0, &project);
                        }
                    }

                    let audio_frame = audio_renderer
                        .as_mut()
                        .and_then(|audio| audio.render_frame(audio_samples_per_frame, &project))
                        .map(|mut frame| {
                            let pts = ((frame_number * frame.rate()) as f64 / fps as f64) as i64;
                            frame.set_pts(Some(pts));
                            frame
                        });

                    if frame_tx
                        .send(MP4Input {
                            audio: audio_frame,
                            video: video_info.wrap_frame(
                                &frame.data,
                                frame_number as i64,
                                frame.padded_bytes_per_row as usize,
                            ),
                        })
                        .is_err()
                    {
                        warn!("Renderer task sender dropped. Exiting");
                        return Ok(());
                    }

                    frame_count += 1;
                }

                if let Some(frame) = first_frame {
                    let rgb_img = ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(
                        frame.width,
                        frame.height,
                        frame
                            .data
                            .chunks(frame.padded_bytes_per_row as usize)
                            .flat_map(|row| {
                                row[0..(frame.width * 4) as usize]
                                    .chunks(4)
                                    .flat_map(|chunk| [chunk[0], chunk[1], chunk[2]])
                            })
                            .collect::<Vec<_>>(),
                    )
                    .expect("Failed to create image from frame data");

                    let screenshots_dir = project_path.join("screenshots");
                    std::fs::create_dir_all(&screenshots_dir).unwrap_or_else(|e| {
                        eprintln!("Failed to create screenshots directory: {e:?}");
                    });

                    // Save full-size screenshot
                    let screenshot_path = screenshots_dir.join("display.jpg");
                    rgb_img.save(&screenshot_path).unwrap_or_else(|e| {
                        eprintln!("Failed to save screenshot: {e:?}");
                    });
                } else {
                    warn!("No frames were processed, cannot save screenshot or thumbnail");
                }

                Ok::<_, String>(())
            }
        })
        .then(|r| async {
            r.map_err(|e| e.to_string())
                .and_then(|v| v.map_err(|e| e.to_string()))
        });

        let render_video_task = cap_rendering::render_video_to_channel(
            &base.render_constants,
            &base.project_config,
            tx_image_data,
            &base.recording_meta,
            meta,
            base.segments
                .iter()
                .map(|s| RenderSegment {
                    cursor: s.cursor.clone(),
                    decoders: s.decoders.clone(),
                })
                .collect(),
            fps,
            self.resolution_base,
            &base.recordings,
        )
        .then(|v| async { v.map_err(|e| e.to_string()) });

        tokio::try_join!(encoder_thread, render_video_task, render_task)?;

        Ok(output_path)
    }
}

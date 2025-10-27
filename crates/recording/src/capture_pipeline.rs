use crate::{
    feeds::microphone::MicrophoneFeedLock,
    output_pipeline::*,
    sources,
    sources::screen_capture::{self, CropBounds, ScreenCaptureFormat, ScreenCaptureTarget},
};
use anyhow::anyhow;
use cap_timestamp::Timestamps;
use std::{path::PathBuf, sync::Arc};

pub trait MakeCapturePipeline: ScreenCaptureFormat + std::fmt::Debug + 'static {
    async fn make_studio_mode_pipeline(
        screen_capture: screen_capture::VideoSourceConfig,
        output_path: PathBuf,
        start_time: Timestamps,
        recording_bpp: Option<f32>,
        recording_preset: Option<String>,
        encoder_type: Option<String>,
        recording_fps: u32,
    ) -> anyhow::Result<OutputPipeline>
    where
        Self: Sized;
}

pub struct Stop;

#[cfg(target_os = "macos")]
impl MakeCapturePipeline for screen_capture::CMSampleBufferCapture {
    async fn make_studio_mode_pipeline(
        screen_capture: screen_capture::VideoSourceConfig,
        output_path: PathBuf,
        start_time: Timestamps,
        _recording_bpp: Option<f32>,
        _recording_preset: Option<String>,
        _encoder_type: Option<String>,
        _recording_fps: u32,
    ) -> anyhow::Result<OutputPipeline> {
        OutputPipeline::builder(output_path.clone())
            .with_video::<screen_capture::VideoSource>(screen_capture)
            .with_timestamps(start_time)
            .build::<AVFoundationMp4Muxer>(Default::default())
            .await
    }
}

#[cfg(windows)]
impl MakeCapturePipeline for screen_capture::Direct3DCapture {
    async fn make_studio_mode_pipeline(
        screen_capture: screen_capture::VideoSourceConfig,
        output_path: PathBuf,
        start_time: Timestamps,
        recording_bpp: Option<f32>,
        recording_preset: Option<String>,
        encoder_type: Option<String>,
        recording_fps: u32,
    ) -> anyhow::Result<OutputPipeline> {
        let d3d_device = screen_capture.d3d_device.clone();
        let bitrate_multiplier = recording_bpp.unwrap_or(1.2);

        OutputPipeline::builder(output_path.clone())
            .with_video::<screen_capture::VideoSource>(screen_capture)
            .with_timestamps(start_time)
            .build::<WindowsMuxer>(WindowsMuxerConfig {
                pixel_format: screen_capture::Direct3DCapture::PIXEL_FORMAT.as_dxgi(),
                d3d_device,
                bitrate_multiplier,
                frame_rate: recording_fps,
                output_size: None,
                encoder_type,
                preset: recording_preset,
            })
            .await
    }
}

#[cfg(target_os = "macos")]
pub type ScreenCaptureMethod = screen_capture::CMSampleBufferCapture;

#[cfg(windows)]
pub type ScreenCaptureMethod = screen_capture::Direct3DCapture;

pub fn target_to_display_and_crop(
    target: &ScreenCaptureTarget,
) -> anyhow::Result<(scap_targets::Display, Option<CropBounds>)> {
    use scap_targets::{bounds::*, *};

    let display = target
        .display()
        .ok_or_else(|| anyhow!("Display not found"))?;

    let crop_bounds = match target {
        ScreenCaptureTarget::Display { .. } => None,
        ScreenCaptureTarget::Window { id } => {
            let window = Window::from_id(id).ok_or_else(|| anyhow!("Window not found"))?;

            #[cfg(target_os = "macos")]
            {
                let raw_display_bounds = display
                    .raw_handle()
                    .logical_bounds()
                    .ok_or_else(|| anyhow!("No display bounds"))?;
                let raw_window_bounds = window
                    .raw_handle()
                    .logical_bounds()
                    .ok_or_else(|| anyhow!("No window bounds"))?;

                Some(LogicalBounds::new(
                    LogicalPosition::new(
                        raw_window_bounds.position().x() - raw_display_bounds.position().x(),
                        raw_window_bounds.position().y() - raw_display_bounds.position().y(),
                    ),
                    raw_window_bounds.size(),
                ))
            }

            #[cfg(windows)]
            {
                let raw_display_position = display
                    .raw_handle()
                    .physical_position()
                    .ok_or_else(|| anyhow!("No display bounds"))?;
                let raw_window_bounds = window
                    .raw_handle()
                    .physical_bounds()
                    .ok_or_else(|| anyhow!("No window bounds"))?;

                Some(PhysicalBounds::new(
                    PhysicalPosition::new(
                        raw_window_bounds.position().x() - raw_display_position.x(),
                        raw_window_bounds.position().y() - raw_display_position.y(),
                    ),
                    raw_window_bounds.size(),
                ))
            }
        }
        ScreenCaptureTarget::Area {
            bounds: relative_bounds,
            ..
        } => {
            #[cfg(target_os = "macos")]
            {
                Some(*relative_bounds)
            }

            #[cfg(windows)]
            {
                let raw_display_size = display
                    .physical_size()
                    .ok_or_else(|| anyhow!("No display bounds"))?;
                let logical_display_size = display
                    .logical_size()
                    .ok_or_else(|| anyhow!("No display logical size"))?;
                Some(PhysicalBounds::new(
                    PhysicalPosition::new(
                        (relative_bounds.position().x() / logical_display_size.width())
                            * raw_display_size.width(),
                        (relative_bounds.position().y() / logical_display_size.height())
                            * raw_display_size.height(),
                    ),
                    PhysicalSize::new(
                        (relative_bounds.size().width() / logical_display_size.width())
                            * raw_display_size.width(),
                        (relative_bounds.size().height() / logical_display_size.height())
                            * raw_display_size.height(),
                    ),
                ))
            }
        }
    };

    Ok((display, crop_bounds))
}

#[cfg(windows)]
pub fn create_d3d_device()
-> windows::core::Result<windows::Win32::Graphics::Direct3D11::ID3D11Device> {
    use windows::Win32::Graphics::{
        Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE},
        Direct3D11::{D3D11_CREATE_DEVICE_FLAG, ID3D11Device},
    };

    let mut device = None;
    let flags = {
        use windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_BGRA_SUPPORT;

        let mut flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
        if cfg!(feature = "d3ddebug") {
            use windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_DEBUG;

            flags |= D3D11_CREATE_DEVICE_DEBUG;
        }
        flags
    };
    let mut result = create_d3d_device_with_type(D3D_DRIVER_TYPE_HARDWARE, flags, &mut device);
    if let Err(error) = &result {
        use windows::Win32::Graphics::Dxgi::DXGI_ERROR_UNSUPPORTED;

        if error.code() == DXGI_ERROR_UNSUPPORTED {
            use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;

            result = create_d3d_device_with_type(D3D_DRIVER_TYPE_WARP, flags, &mut device);
        }
    }
    result?;

    fn create_d3d_device_with_type(
        driver_type: D3D_DRIVER_TYPE,
        flags: D3D11_CREATE_DEVICE_FLAG,
        device: *mut Option<ID3D11Device>,
    ) -> windows::core::Result<()> {
        unsafe {
            use windows::Win32::{
                Foundation::HMODULE,
                Graphics::Direct3D11::{D3D11_SDK_VERSION, D3D11CreateDevice},
            };

            D3D11CreateDevice(
                None,
                driver_type,
                HMODULE(std::ptr::null_mut()),
                flags,
                None,
                D3D11_SDK_VERSION,
                Some(device),
                None,
                None,
            )
        }
    }

    Ok(device.unwrap())
}

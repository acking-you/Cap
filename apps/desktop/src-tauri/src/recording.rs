use cap_fail::fail;
use cap_project::CursorMoveEvent;
use cap_project::cursor::SHORT_CURSOR_SHAPE_DEBOUNCE_MS;
use cap_project::{
    CursorClickEvent, MultipleSegments, Platform, ProjectConfiguration,
    RecordingMeta, RecordingMetaInner, SharingMeta, StudioRecordingMeta, StudioRecordingStatus,
    TimelineConfiguration, TimelineSegment, UploadMeta, ZoomMode, ZoomSegment,
    cursor::CursorEvents,
};
use cap_recording::PipelineDoneError;
use cap_recording::feeds::camera::CameraFeedLock;
use cap_recording::feeds::microphone::MicrophoneFeedLock;
use cap_recording::{
    RecordingError, RecordingMode,
    feeds::{camera, microphone},
    sources::{
        screen_capture,
        screen_capture::{CaptureDisplay, CaptureWindow, ScreenCaptureTarget},
    },
    studio_recording,
};
use cap_rendering::ProjectRecordingsMeta;
use cap_utils::{ensure_dir, spawn_actor};
use futures::stream;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogBuilder};
use tauri_specta::Event;
use tracing::*;

use crate::web_api::AuthedApiError;
use crate::{
    App, CurrentRecordingChanged, MutableState, NewStudioRecordingAdded, RecordingState,
    RecordingStopped, VideoUploadInfo,
    api::PresignedS3PutRequestMethod,
    audio::AppSounds,
    auth::AuthStore,
    create_screenshot,
    general_settings::{
        self, GeneralSettingsStore, PostDeletionBehaviour, PostStudioRecordingBehaviour,
    },
    open_external_link,
    presets::PresetsStore,
    thumbnails::*,
    upload::{
        build_video_meta, compress_image, create_or_get_video, upload_video,
    },
    web_api::ManagerExt,
    windows::{CapWindowId, ShowCapWindow},
};

pub struct InProgressRecordingCommon {
    pub target_name: String,
    pub inputs: StartRecordingInputs,
    pub recording_dir: PathBuf,
}

pub struct InProgressRecording {
    pub handle: studio_recording::ActorHandle,
    pub common: InProgressRecordingCommon,
}

impl InProgressRecording {
    pub fn capture_target(&self) -> &ScreenCaptureTarget {
        &self.handle.capture_target
    }

    pub fn inputs(&self) -> &StartRecordingInputs {
        &self.common.inputs
    }

    pub async fn pause(&self) -> anyhow::Result<()> {
        self.handle.pause().await
    }

    pub async fn resume(&self) -> anyhow::Result<()> {
        self.handle.resume().await
    }

    pub fn recording_dir(&self) -> &PathBuf {
        &self.common.recording_dir
    }

    pub async fn stop(self) -> anyhow::Result<CompletedRecording> {
        Ok(CompletedRecording {
            recording: self.handle.stop().await?,
            target_name: self.common.target_name,
        })
    }

    pub fn done_fut(&self) -> cap_recording::DoneFut {
        self.handle.done_fut()
    }

    pub async fn cancel(self) -> anyhow::Result<()> {
        self.handle.cancel().await
    }

    pub fn mode(&self) -> RecordingMode {
        RecordingMode::Studio
    }
}

pub struct CompletedRecording {
    pub recording: studio_recording::CompletedRecording,
    pub target_name: String,
}

impl CompletedRecording {
    pub fn project_path(&self) -> &PathBuf {
        &self.recording.project_path
    }

    pub fn target_name(&self) -> &String {
        &self.target_name
    }
}

#[tauri::command(async)]
#[specta::specta]
pub async fn list_capture_displays() -> Vec<CaptureDisplay> {
    screen_capture::list_displays()
        .into_iter()
        .map(|(v, _)| v)
        .collect()
}

#[tauri::command(async)]
#[specta::specta]
pub async fn list_capture_windows() -> Vec<CaptureWindow> {
    screen_capture::list_windows()
        .into_iter()
        .map(|(v, _)| v)
        .collect()
}

#[tauri::command(async)]
#[specta::specta]
pub fn list_cameras() -> Vec<cap_camera::CameraInfo> {
    cap_camera::list_cameras().collect()
}

#[tauri::command]
#[specta::specta]
#[instrument]
pub async fn list_displays_with_thumbnails() -> Result<Vec<CaptureDisplayWithThumbnail>, String> {
    tokio::task::spawn_blocking(|| {
        tauri::async_runtime::block_on(collect_displays_with_thumbnails())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
#[instrument]
pub async fn list_windows_with_thumbnails() -> Result<Vec<CaptureWindowWithThumbnail>, String> {
    tokio::task::spawn_blocking(
        || tauri::async_runtime::block_on(collect_windows_with_thumbnails()),
    )
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Deserialize, Type, Clone, Debug)]
pub struct StartRecordingInputs {
    pub capture_target: ScreenCaptureTarget,
    #[serde(default)]
    pub capture_system_audio: bool,
    pub mode: RecordingMode,
    #[serde(default = "default_recording_bpp")]
    pub recording_bpp: Option<f32>,
    #[serde(default)]
    pub recording_preset: Option<String>,
}

fn default_recording_bpp() -> Option<f32> {
    Some(1.2)
}

#[derive(tauri_specta::Event, specta::Type, Clone, Debug, serde::Serialize)]
#[serde(tag = "variant")]
pub enum RecordingEvent {
    Countdown { value: u32 },
    Started,
    Stopped,
    Failed { error: String },
}

#[derive(Serialize, Type)]
pub enum RecordingAction {
    Started,
    InvalidAuthentication,
    UpgradeRequired,
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(name = "recording", skip_all)]
pub async fn start_recording(
    app: AppHandle,
    state_mtx: MutableState<'_, App>,
    inputs: StartRecordingInputs,
) -> Result<RecordingAction, String> {
    if !matches!(state_mtx.read().await.recording_state, RecordingState::None) {
        return Err("Recording already in progress".to_string());
    }

    let id = uuid::Uuid::new_v4().to_string();
    let general_settings = GeneralSettingsStore::get(&app).ok().flatten();
    let general_settings = general_settings.as_ref();

    let recording_dir = app
        .path()
        .app_data_dir()
        .unwrap()
        .join("recordings")
        .join(format!("{id}.cap"));

    ensure_dir(&recording_dir).map_err(|e| format!("Failed to create recording directory: {e}"))?;
    state_mtx
        .write()
        .await
        .add_recording_logging_handle(&recording_dir.join("recording-logs.log"))
        .await?;

    let target_name = {
        let title = inputs.capture_target.title();

        match inputs.capture_target.clone() {
            ScreenCaptureTarget::Area { .. } => title.unwrap_or_else(|| "Area".to_string()),
            ScreenCaptureTarget::Window { .. } => title.unwrap_or_else(|| "Window".to_string()),
            ScreenCaptureTarget::Display { .. } => title.unwrap_or_else(|| "Screen".to_string()),
        }
    };

    if let Some(window) = CapWindowId::Camera.get(&app) {
        let _ = window.set_content_protected(true);
    }

    let date_time = if cfg!(windows) {
        // Windows doesn't support colon in file paths
        chrono::Local::now().format("%Y-%m-%d %H.%M.%S")
    } else {
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    };

    let meta = RecordingMeta {
        platform: Some(Platform::default()),
        project_path: recording_dir.clone(),
        pretty_name: format!("{target_name} {date_time}"),
        inner: RecordingMetaInner::Studio(StudioRecordingMeta::MultipleSegments {
            inner: MultipleSegments {
                segments: Default::default(),
                cursors: Default::default(),
                status: Some(StudioRecordingStatus::InProgress),
            },
        }),
        sharing: None,
        upload: None,
    };

    meta.save_for_project()
        .map_err(|e| format!("Failed to save recording meta: {e}"))?;

    match &inputs.capture_target {
        ScreenCaptureTarget::Window { id: _id } => {
            if let Some(show) = inputs
                .capture_target
                .display()
                .map(|d| ShowCapWindow::WindowCaptureOccluder { screen_id: d.id() })
            {
                let _ = show.show(&app).await;
            }
        }
        ScreenCaptureTarget::Area { screen, .. } => {
            let _ = ShowCapWindow::WindowCaptureOccluder {
                screen_id: screen.clone(),
            }
            .show(&app)
            .await;
        }
        _ => {}
    }

    // Set pending state BEFORE closing main window and starting countdown
    state_mtx
        .write()
        .await
        .set_pending_recording(inputs.mode, inputs.capture_target.clone());

    let countdown = general_settings.and_then(|v| v.recording_countdown);
    for (id, win) in app
        .webview_windows()
        .iter()
        .filter_map(|(label, win)| CapWindowId::from_str(label).ok().map(|id| (id, win)))
    {
        if matches!(id, CapWindowId::TargetSelectOverlay { .. }) {
            win.close().ok();
        }
    }
    let _ = ShowCapWindow::InProgressRecording { countdown }
        .show(&app)
        .await;

    if let Some(window) = CapWindowId::Main.get(&app) {
        let _ = general_settings
            .map(|v| v.main_window_recording_start_behaviour)
            .unwrap_or_default()
            .perform(&window);
    }

    if let Some(countdown) = countdown {
        for t in 0..countdown {
            let _ = RecordingEvent::Countdown {
                value: countdown - t,
            }
            .emit(&app);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    debug!("spawning start_recording actor");

    // done in spawn to catch panics just in case
    let app_handle = app.clone();
    let spawn_actor_res = async {
        spawn_actor({
            let state_mtx = Arc::clone(&state_mtx);
            let general_settings = general_settings.cloned();
            let recording_dir = recording_dir.clone();
            async move {
                fail!("recording::spawn_actor");
                let mut state = state_mtx.write().await;

                use kameo::error::SendError;
                let mic_feed = match state.mic_feed.ask(microphone::Lock).await {
                    Ok(lock) => Some(Arc::new(lock)),
                    Err(SendError::HandlerError(microphone::LockFeedError::NoInput)) => None,
                    Err(e) => return Err(e.to_string()),
                };

                let camera_feed = match state.camera_feed.ask(camera::Lock).await {
                    Ok(lock) => Some(Arc::new(lock)),
                    Err(SendError::HandlerError(camera::LockFeedError::NoInput)) => None,
                    Err(e) => return Err(e.to_string()),
                };

                #[cfg(target_os = "macos")]
                let shareable_content = crate::platform::get_shareable_content()
                    .await
                    .map_err(|e| format!("GetShareableContent: {e}"))?
                    .ok_or_else(|| format!("GetShareableContent/NotAvailable"))?;

                let common = InProgressRecordingCommon {
                    target_name,
                    inputs: inputs.clone(),
                    recording_dir: recording_dir.clone(),
                };

                #[cfg(target_os = "macos")]
                let excluded_windows = {
                    let window_exclusions = general_settings
                        .as_ref()
                        .map_or_else(general_settings::default_excluded_windows, |settings| {
                            settings.excluded_windows.clone()
                        });

                    crate::window_exclusion::resolve_window_ids(&window_exclusions)
                };

                let mut builder = studio_recording::Actor::builder(
                    recording_dir.clone(),
                    inputs.capture_target.clone(),
                )
                .with_system_audio(inputs.capture_system_audio)
                .with_custom_cursor(
                    general_settings
                        .map(|s| s.custom_cursor_capture)
                        .unwrap_or_default(),
                )
                .with_recording_quality(inputs.recording_bpp, inputs.recording_preset.clone());

                #[cfg(target_os = "macos")]
                {
                    builder = builder.with_excluded_windows(excluded_windows.clone());
                }

                if let Some(camera_feed) = camera_feed {
                    builder = builder.with_camera_feed(camera_feed);
                }

                if let Some(mic_feed) = mic_feed {
                    builder = builder.with_mic_feed(mic_feed);
                }

                let handle = builder
                    .build(
                        #[cfg(target_os = "macos")]
                        shareable_content,
                    )
                    .await
                    .map_err(|e| {
                        error!("Failed to spawn studio recording actor: {e:#}");
                        format!("{e:#}")
                    })?;

                let actor = InProgressRecording { handle, common };

                let done_fut = actor.done_fut();

                state.set_current_recording(actor);

                Ok::<_, String>(done_fut)
            }
        })
        .await
        .map_err(|e| format!("Failed to spawn recording actor: {e}"))?
    }
    .await;

    let actor_done_fut = match spawn_actor_res {
        Ok(rx) => rx,
        Err(err) => {
            let _ = RecordingEvent::Failed { error: err.clone() }.emit(&app);

            let mut dialog = MessageDialogBuilder::new(
                app.dialog().clone(),
                "An error occurred".to_string(),
                err.clone(),
            )
            .kind(tauri_plugin_dialog::MessageDialogKind::Error);

            if let Some(window) = CapWindowId::RecordingControls.get(&app) {
                dialog = dialog.parent(&window);
            }

            dialog.blocking_show();

            let mut state = state_mtx.write().await;
            let _ = handle_recording_end(app, Err(err.clone()), &mut state, recording_dir).await;

            return Err(err);
        }
    };

    let _ = RecordingEvent::Started.emit(&app);

    spawn_actor({
        let app = app.clone();
        let state_mtx = Arc::clone(&state_mtx);
        async move {
            fail!("recording::wait_actor_done");
            let res = actor_done_fut.await;
            info!("recording wait actor done: {:?}", &res);
            match res {
                Ok(()) => {
                    let _ = RecordingEvent::Stopped.emit(&app);
                }
                Err(e) => {
                    let mut state = state_mtx.write().await;

                    let _ = RecordingEvent::Failed {
                        error: e.to_string(),
                    }
                    .emit(&app);

                    let mut dialog = MessageDialogBuilder::new(
                        app.dialog().clone(),
                        "An error occurred".to_string(),
                        e.to_string(),
                    )
                    .kind(tauri_plugin_dialog::MessageDialogKind::Error);

                    if let Some(window) = CapWindowId::RecordingControls.get(&app) {
                        dialog = dialog.parent(&window);
                    }

                    dialog.blocking_show();

                    // this clears the current recording for us
                    handle_recording_end(app, Err(e.to_string()), &mut state, recording_dir)
                        .await
                        .ok();
                }
            }
        }
    });

    AppSounds::StartRecording.play();

    Ok(RecordingAction::Started)
}

#[tauri::command]
#[specta::specta]
#[instrument(skip(state))]
pub async fn pause_recording(state: MutableState<'_, App>) -> Result<(), String> {
    let mut state = state.write().await;

    if let Some(recording) = state.current_recording_mut() {
        recording.pause().await.map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
#[instrument(skip(state))]
pub async fn resume_recording(state: MutableState<'_, App>) -> Result<(), String> {
    let mut state = state.write().await;

    if let Some(recording) = state.current_recording_mut() {
        recording.resume().await.map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
#[instrument(skip(app, state))]
pub async fn stop_recording(app: AppHandle, state: MutableState<'_, App>) -> Result<(), String> {
    let mut state = state.write().await;
    let Some(current_recording) = state.clear_current_recording() else {
        return Err("Recording not in progress".to_string())?;
    };

    let completed_recording = current_recording.stop().await.map_err(|e| e.to_string())?;
    let recording_dir = completed_recording.project_path().clone();

    handle_recording_end(app, Ok(completed_recording), &mut state, recording_dir).await?;

    Ok(())
}

#[tauri::command]
#[specta::specta]
#[instrument(skip(app, state))]
pub async fn restart_recording(
    app: AppHandle,
    state: MutableState<'_, App>,
) -> Result<RecordingAction, String> {
    let Some(recording) = state.write().await.clear_current_recording() else {
        return Err("No recording in progress".to_string());
    };

    let _ = CurrentRecordingChanged.emit(&app);

    let inputs = recording.inputs().clone();

    let _ = recording.cancel().await;

    tokio::time::sleep(Duration::from_millis(1000)).await;

    start_recording(app.clone(), state, inputs).await
}

#[tauri::command]
#[specta::specta]
#[instrument(skip(app, state))]
pub async fn delete_recording(app: AppHandle, state: MutableState<'_, App>) -> Result<(), String> {
    let recording_data = {
        let mut app_state = state.write().await;
        app_state.clear_current_recording()
    };

    if let Some(recording) = recording_data {
        let recording_dir = recording.recording_dir().clone();
        CurrentRecordingChanged.emit(&app).ok();
        RecordingStopped {}.emit(&app).ok();

        let _ = recording.cancel().await;

        std::fs::remove_dir_all(&recording_dir).ok();

        // Check user's post-deletion behavior setting
        let settings = GeneralSettingsStore::get(&app)
            .ok()
            .flatten()
            .unwrap_or_default();

        if let Some(window) = CapWindowId::RecordingControls.get(&app) {
            let _ = window.close();
        }

        match settings.post_deletion_behaviour {
            PostDeletionBehaviour::DoNothing => {}
            PostDeletionBehaviour::ReopenRecordingWindow => {
                let _ = ShowCapWindow::Main {
                    init_target_mode: None,
                }
                .show(&app)
                .await;
            }
        }
    }

    Ok(())
}

// runs when a recording ends, whether from success or failure
async fn handle_recording_end(
    handle: AppHandle,
    recording: Result<CompletedRecording, String>,
    app: &mut App,
    recording_dir: PathBuf,
) -> Result<(), String> {
    // Clear current recording, just in case :)
    app.clear_current_recording();

    let res = match recording {
        // we delay reporting errors here so that everything else happens first
        Ok(recording) => Some(handle_recording_finish(&handle, recording).await),
        Err(error) => {
            if let Ok(mut project_meta) =
                RecordingMeta::load_for_project(&recording_dir).map_err(|err| {
                    error!("Error loading recording meta while finishing recording: {err}")
                })
            {
                if let RecordingMetaInner::Studio(meta) = &mut project_meta.inner {
                    if let StudioRecordingMeta::MultipleSegments { inner } = meta {
                        inner.status = Some(StudioRecordingStatus::Failed { error });
                    }
                }
                project_meta
                    .save_for_project()
                    .map_err(|err| {
                        error!("Error saving recording meta while finishing recording: {err}")
                    })
                    .ok();
            }

            None
        }
    };

    let _ = RecordingStopped.emit(&handle);

    let _ = app.recording_logging_handle.reload(None);

    if let Some(window) = CapWindowId::RecordingControls.get(&handle) {
        let _ = window.close();
    }

    if let Some(window) = CapWindowId::Main.get(&handle) {
        window.unminimize().ok();
    } else {
        if let Some(v) = CapWindowId::Camera.get(&handle) {
            let _ = v.close();
        }
        let _ = app.mic_feed.ask(microphone::RemoveInput).await;
        let _ = app.camera_feed.ask(camera::RemoveInput).await;
        if let Some(win) = CapWindowId::Camera.get(&handle) {
            win.close().ok();
        }
    }

    CurrentRecordingChanged.emit(&handle).ok();

    if let Some(res) = res {
        res?;
    }

    Ok(())
}

// runs when a recording successfully finishes
async fn handle_recording_finish(
    app: &AppHandle,
    completed_recording: CompletedRecording,
) -> Result<(), String> {
    let recording_dir = completed_recording.project_path().clone();

    let screenshots_dir = recording_dir.join("screenshots");
    std::fs::create_dir_all(&screenshots_dir).ok();

    let display_output_path = match &completed_recording.recording.meta {
        StudioRecordingMeta::SingleSegment { segment } => {
            segment.display.path.to_path(&recording_dir)
        }
        StudioRecordingMeta::MultipleSegments { inner, .. } => {
            inner.segments[0].display.path.to_path(&recording_dir)
        }
    };

    let display_screenshot = screenshots_dir.join("display.jpg");
    let _screenshot_task = tokio::spawn(create_screenshot(
        display_output_path,
        display_screenshot.clone(),
        None,
    ));

    let recordings = ProjectRecordingsMeta::new(&recording_dir, &completed_recording.recording.meta)?;

    let config = project_config_from_recording(
        app,
        &completed_recording.recording,
        &recordings,
        PresetsStore::get_default_preset(app)?.map(|p| p.config),
    );

    config.write(&recording_dir).map_err(|e| e.to_string())?;

    let meta_inner = RecordingMetaInner::Studio(completed_recording.recording.meta.clone());

    if let Ok(mut meta) = RecordingMeta::load_for_project(&recording_dir).map_err(|err| {
        error!("Failed to load recording meta while saving finished recording: {err}")
    }) {
        meta.inner = meta_inner.clone();
        meta.sharing = None;
        meta.save_for_project()
            .map_err(|e| format!("Failed to save recording meta: {e}"))?;
    }

    match GeneralSettingsStore::get(app)
        .ok()
        .flatten()
        .map(|v| v.post_studio_recording_behaviour)
        .unwrap_or(PostStudioRecordingBehaviour::OpenEditor)
    {
        PostStudioRecordingBehaviour::OpenEditor => {
            let _ = ShowCapWindow::Editor {
                project_path: recording_dir,
            }
            .show(app)
            .await;
        }
        PostStudioRecordingBehaviour::ShowOverlay => {
            let _ = ShowCapWindow::RecordingsOverlay.show(app).await;

            let app = AppHandle::clone(app);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1000)).await;

                let _ = NewStudioRecordingAdded {
                    path: recording_dir.clone(),
                }
                .emit(&app);
            });
        }
    };

    // Play sound to indicate recording has stopped
    AppSounds::StopRecording.play();

    Ok(())
}

/// Core logic for generating zoom segments based on mouse click events.
/// This is an experimental feature that automatically creates zoom effects
/// around user interactions to highlight important moments.
fn generate_zoom_segments_from_clicks_impl(
    mut clicks: Vec<CursorClickEvent>,
    mut moves: Vec<CursorMoveEvent>,
    max_duration: f64,
) -> Vec<ZoomSegment> {
    const STOP_PADDING_SECONDS: f64 = 0.8;
    const CLICK_PRE_PADDING: f64 = 0.6;
    const CLICK_POST_PADDING: f64 = 1.6;
    const MOVEMENT_PRE_PADDING: f64 = 0.4;
    const MOVEMENT_POST_PADDING: f64 = 1.2;
    const MERGE_GAP_THRESHOLD: f64 = 0.6;
    const MIN_SEGMENT_DURATION: f64 = 1.3;
    const MOVEMENT_WINDOW_SECONDS: f64 = 1.2;
    const MOVEMENT_EVENT_DISTANCE_THRESHOLD: f64 = 0.025;
    const MOVEMENT_WINDOW_DISTANCE_THRESHOLD: f64 = 0.1;

    if max_duration <= 0.0 {
        return Vec::new();
    }

    // We trim the tail of the recording to avoid using the final
    // "stop recording" click as a zoom target.
    let activity_end_limit = if max_duration > STOP_PADDING_SECONDS {
        max_duration - STOP_PADDING_SECONDS
    } else {
        max_duration
    };

    if activity_end_limit <= f64::EPSILON {
        return Vec::new();
    }

    clicks.sort_by(|a, b| {
        a.time_ms
            .partial_cmp(&b.time_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    moves.sort_by(|a, b| {
        a.time_ms
            .partial_cmp(&b.time_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Remove trailing click-down events that are too close to the end.
    while let Some(index) = clicks.iter().rposition(|c| c.down) {
        let time_secs = clicks[index].time_ms / 1000.0;
        if time_secs > activity_end_limit {
            clicks.remove(index);
        } else {
            break;
        }
    }

    let mut intervals: Vec<(f64, f64)> = Vec::new();

    for click in clicks.into_iter().filter(|c| c.down) {
        let time = click.time_ms / 1000.0;
        if time >= activity_end_limit {
            continue;
        }

        let start = (time - CLICK_PRE_PADDING).max(0.0);
        let end = (time + CLICK_POST_PADDING).min(activity_end_limit);

        if end > start {
            intervals.push((start, end));
        }
    }

    let mut last_move_by_cursor: HashMap<String, (f64, f64, f64)> = HashMap::new();
    let mut distance_window: VecDeque<(f64, f64)> = VecDeque::new();
    let mut window_distance = 0.0_f64;

    for mv in moves.iter() {
        let time = mv.time_ms / 1000.0;
        if time >= activity_end_limit {
            break;
        }

        let distance = if let Some((_, last_x, last_y)) = last_move_by_cursor.get(&mv.cursor_id) {
            let dx = mv.x - last_x;
            let dy = mv.y - last_y;
            (dx * dx + dy * dy).sqrt()
        } else {
            0.0
        };

        last_move_by_cursor.insert(mv.cursor_id.clone(), (time, mv.x, mv.y));

        if distance <= f64::EPSILON {
            continue;
        }

        distance_window.push_back((time, distance));
        window_distance += distance;

        while let Some(&(old_time, old_distance)) = distance_window.front() {
            if time - old_time > MOVEMENT_WINDOW_SECONDS {
                distance_window.pop_front();
                window_distance -= old_distance;
            } else {
                break;
            }
        }

        if window_distance < 0.0 {
            window_distance = 0.0;
        }

        let significant_movement = distance >= MOVEMENT_EVENT_DISTANCE_THRESHOLD
            || window_distance >= MOVEMENT_WINDOW_DISTANCE_THRESHOLD;

        if !significant_movement {
            continue;
        }

        let start = (time - MOVEMENT_PRE_PADDING).max(0.0);
        let end = (time + MOVEMENT_POST_PADDING).min(activity_end_limit);

        if end > start {
            intervals.push((start, end));
        }
    }

    if intervals.is_empty() {
        return Vec::new();
    }

    intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut merged: Vec<(f64, f64)> = Vec::new();
    for interval in intervals {
        if let Some(last) = merged.last_mut()
            && interval.0 <= last.1 + MERGE_GAP_THRESHOLD
        {
            last.1 = last.1.max(interval.1);
            continue;
        }
        merged.push(interval);
    }

    merged
        .into_iter()
        .filter_map(|(start, end)| {
            let duration = end - start;
            if duration < MIN_SEGMENT_DURATION {
                return None;
            }

            Some(ZoomSegment {
                start,
                end,
                amount: 2.0,
                mode: ZoomMode::Auto,
            })
        })
        .collect()
}

/// Generates zoom segments based on mouse click events during recording.
/// Used during the recording completion process.
pub fn generate_zoom_segments_from_clicks(
    recording: &studio_recording::CompletedRecording,
    recordings: &ProjectRecordingsMeta,
) -> Vec<ZoomSegment> {
    // Build a temporary RecordingMeta so we can use the common implementation
    let recording_meta = RecordingMeta {
        platform: None,
        project_path: recording.project_path.clone(),
        pretty_name: String::new(),
        sharing: None,
        inner: RecordingMetaInner::Studio(recording.meta.clone()),
        upload: None,
    };

    generate_zoom_segments_for_project(&recording_meta, recordings)
}

/// Generates zoom segments from clicks for an existing project.
/// Used in the editor context where we have RecordingMeta.
pub fn generate_zoom_segments_for_project(
    recording_meta: &RecordingMeta,
    recordings: &ProjectRecordingsMeta,
) -> Vec<ZoomSegment> {
    let RecordingMetaInner::Studio(studio_meta) = &recording_meta.inner else {
        return Vec::new();
    };

    let mut all_clicks = Vec::new();
    let mut all_moves = Vec::new();

    match studio_meta {
        StudioRecordingMeta::SingleSegment { segment } => {
            if let Some(cursor_path) = &segment.cursor {
                let mut events = CursorEvents::load_from_file(&recording_meta.path(cursor_path))
                    .unwrap_or_default();
                let pointer_ids = studio_meta.pointer_cursor_ids();
                let pointer_ids_ref = (!pointer_ids.is_empty()).then_some(&pointer_ids);
                events.stabilize_short_lived_cursor_shapes(
                    pointer_ids_ref,
                    SHORT_CURSOR_SHAPE_DEBOUNCE_MS,
                );
                all_clicks = events.clicks;
                all_moves = events.moves;
            }
        }
        StudioRecordingMeta::MultipleSegments { inner, .. } => {
            for segment in inner.segments.iter() {
                let events = segment.cursor_events(recording_meta);
                all_clicks.extend(events.clicks);
                all_moves.extend(events.moves);
            }
        }
    }

    generate_zoom_segments_from_clicks_impl(all_clicks, all_moves, recordings.duration())
}

fn project_config_from_recording(
    app: &AppHandle,
    completed_recording: &studio_recording::CompletedRecording,
    recordings: &ProjectRecordingsMeta,
    default_config: Option<ProjectConfiguration>,
) -> ProjectConfiguration {
    let settings = GeneralSettingsStore::get(app)
        .unwrap_or(None)
        .unwrap_or_default();

    let mut config = default_config.unwrap_or_default();

    let timeline_segments = recordings
        .segments
        .iter()
        .enumerate()
        .map(|(i, segment)| TimelineSegment {
            recording_segment: i as u32,
            start: 0.0,
            end: segment.duration(),
            timescale: 1.0,
        })
        .collect::<Vec<_>>();

    let zoom_segments = if settings.auto_zoom_on_clicks {
        generate_zoom_segments_from_clicks(completed_recording, recordings)
    } else {
        Vec::new()
    };

    if !zoom_segments.is_empty() {
        config.cursor.size = 200;
    }

    config.timeline = Some(TimelineConfiguration {
        segments: timeline_segments,
        zoom_segments,
        scene_segments: Vec::new(),
    });

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_event(time_ms: f64) -> CursorClickEvent {
        CursorClickEvent {
            active_modifiers: vec![],
            cursor_num: 0,
            cursor_id: "default".to_string(),
            time_ms,
            down: true,
        }
    }

    fn move_event(time_ms: f64, x: f64, y: f64) -> CursorMoveEvent {
        CursorMoveEvent {
            active_modifiers: vec![],
            cursor_id: "default".to_string(),
            time_ms,
            x,
            y,
        }
    }

    #[test]
    fn skips_trailing_stop_click() {
        let segments =
            generate_zoom_segments_from_clicks_impl(vec![click_event(11_900.0)], vec![], 12.0);

        assert!(
            segments.is_empty(),
            "expected trailing stop click to be ignored"
        );
    }

    #[test]
    fn generates_segment_for_sustained_activity() {
        let clicks = vec![click_event(1_200.0), click_event(4_200.0)];
        let moves = vec![
            move_event(1_500.0, 0.10, 0.12),
            move_event(1_720.0, 0.42, 0.45),
            move_event(1_940.0, 0.74, 0.78),
        ];

        let segments = generate_zoom_segments_from_clicks_impl(clicks, moves, 20.0);

        assert!(
            !segments.is_empty(),
            "expected activity to produce zoom segments"
        );
        let first = &segments[0];
        assert!(first.start < first.end);
        assert!(first.end - first.start >= 1.3);
        assert!(first.end <= 19.5);
    }

    #[test]
    fn ignores_cursor_jitter() {
        let jitter_moves = (0..30)
            .map(|i| {
                let t = 1_000.0 + (i as f64) * 30.0;
                let delta = (i as f64) * 0.0004;
                move_event(t, 0.5 + delta, 0.5)
            })
            .collect::<Vec<_>>();

        let segments = generate_zoom_segments_from_clicks_impl(Vec::new(), jitter_moves, 15.0);

        assert!(
            segments.is_empty(),
            "small jitter should not generate segments"
        );
    }
}

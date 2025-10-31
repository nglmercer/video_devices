mod nokhwa_camera;
mod buffer_pool;
mod frame_processor;
mod async_frame_handler;
mod slint_renderer;

#[cfg(target_os = "windows")]
mod windows_permissions;

use nokhwa_camera::{NokhwaCameraManager, VideoStream};
use slint::{Image, SharedPixelBuffer};
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::Result;
use parking_lot::Mutex as ParkingMutex;
use async_frame_handler::{AsyncFrameHandler, AsyncFrameHandlerConfig};
use slint_renderer::{SlintRenderer, SlintRendererConfig};

slint::include_modules!();

#[derive(Clone)]
struct CameraState {
    manager: Arc<ParkingMutex<NokhwaCameraManager>>,
    stream: Arc<ParkingMutex<Option<VideoStream>>>,
    current_camera: Arc<ParkingMutex<Option<usize>>>,
    is_streaming: Arc<ParkingMutex<bool>>,
    frame_count: Arc<ParkingMutex<u32>>,
    last_fps_update: Arc<ParkingMutex<Instant>>,
    async_handler: Arc<ParkingMutex<Option<AsyncFrameHandler>>>,
    slint_renderer: Arc<ParkingMutex<Option<SlintRenderer>>>,
    render_performance_stats: Arc<ParkingMutex<RenderPerformanceStats>>,
}

#[derive(Debug, Default)]
struct RenderPerformanceStats {
    frames_rendered: u64,
    frames_skipped: u64,
    total_render_time: Duration,
    average_render_time: Duration,
    current_fps: f64,
}

impl CameraState {
    fn new() -> Self {
        Self {
            manager: Arc::new(ParkingMutex::new(NokhwaCameraManager::new())),
            stream: Arc::new(ParkingMutex::new(None)),
            current_camera: Arc::new(ParkingMutex::new(None)),
            is_streaming: Arc::new(ParkingMutex::new(false)),
            frame_count: Arc::new(ParkingMutex::new(0)),
            last_fps_update: Arc::new(ParkingMutex::new(Instant::now())),
            async_handler: Arc::new(ParkingMutex::new(None)),
            slint_renderer: Arc::new(ParkingMutex::new(None)),
            render_performance_stats: Arc::new(ParkingMutex::new(RenderPerformanceStats::default())),
        }
    }
}

impl RenderPerformanceStats {
    fn update(&mut self, render_time: Duration) {
        self.frames_rendered += 1;
        self.total_render_time += render_time;
        self.average_render_time = self.total_render_time / self.frames_rendered as u32;

        if self.frames_rendered % 30 == 0 {
            self.current_fps = 30.0 / render_time.as_secs_f64();
        }
    }

    fn should_skip_frame(&self, target_fps: f64) -> bool {
        let target_frame_time = Duration::from_secs_f64(1.0 / target_fps);
        self.average_render_time > target_frame_time * 9 / 10
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let ui = CameraView::new()?;
    let camera_state = CameraState::new();

    let ui_weak_refresh = ui.as_weak();
    let ui_weak_start = ui.as_weak();
    let ui_weak_stop = ui.as_weak();
    let ui_weak_update = ui.as_weak();

    let camera_state_refresh = camera_state.clone();
    let camera_state_start = camera_state.clone();
    let camera_state_stop = camera_state.clone();
    let camera_state_update = camera_state.clone();
    let camera_state_initial = camera_state.clone();

    ui.on_refresh_cameras(move || {
        let ui = ui_weak_refresh.upgrade().unwrap();
        let state = camera_state_refresh.clone();

        match refresh_camera_list(&state) {
            Ok(camera_names) => {
                let count = camera_names.len();
                let model: Vec<slint::SharedString> = camera_names.into_iter()
                    .map(|name| name.into())
                    .collect();
                ui.set_cameras(slint::ModelRc::from(model.as_slice()));
                ui.set_status_text(format!("Found {} cameras", count).into());
            }
            Err(e) => {
                eprintln!("Error refreshing cameras: {}", e);
                ui.set_status_text(format!("Error: {}", e).into());
            }
        }
    });

    ui.on_start_camera(move || {
        let ui = ui_weak_start.upgrade().unwrap();
        let state = camera_state_start.clone();

        let selected_index = ui.get_selected_camera_index() as usize;

        {
            let manager = state.manager.lock();
            let cameras = manager.list_cameras();

            if let Some(camera) = cameras.get(selected_index) {
                let camera_name = camera.name.clone();
                drop(manager);
                if let Err(e) = select_camera_by_name(&state, &camera_name) {
                    eprintln!("Error selecting camera: {}", e);
                    ui.set_status_text(format!("Error: {}", e).into());
                    return;
                }
            } else {
                ui.set_status_text("Camera not found".into());
                return;
            }
        }

        match start_camera_stream(&state) {
            Ok(_) => {
                ui.set_camera_active(true);
                ui.set_status_text("Camera started successfully".into());
                ui.set_is_streaming(true);

                *state.frame_count.lock() = 0;
                *state.last_fps_update.lock() = Instant::now();

                if let Err(e) = start_async_frame_handler(&state) {
                    eprintln!("Error starting async handler: {}", e);
                    ui.set_status_text(format!("Async handler error: {}", e).into());
                }

                if let Err(e) = initialize_slint_renderer(&state) {
                    eprintln!("Error initializing Slint renderer: {}", e);
                    ui.set_status_text(format!("Renderer error: {}", e).into());
                }
            }
            Err(e) => {
                eprintln!("Error starting camera: {}", e);
                ui.set_status_text(format!("Error: {}", e).into());
            }
        }
    });

    ui.on_stop_camera(move || {
        let ui = ui_weak_stop.upgrade().unwrap();
        let state = camera_state_stop.clone();

        if let Err(e) = stop_async_frame_handler(&state) {
            eprintln!("Error stopping async handler: {}", e);
        }

        if let Err(e) = cleanup_slint_renderer(&state) {
            eprintln!("Error cleaning up renderer: {}", e);
        }

        if let Err(e) = stop_camera_stream(&state) {
            eprintln!("Error stopping camera: {}", e);
        }

        ui.set_camera_active(false);
        ui.set_status_text("Camera stopped".into());
        ui.set_is_streaming(false);
        ui.set_fps(0);
        ui.set_camera_frame(Image::from_rgba8(SharedPixelBuffer::new(1, 1)));
    });

    ui.on_update_frame(move || {
        let state = camera_state_update.clone();

        if !*state.is_streaming.lock() {
            return;
        }

        if let Some(ui) = ui_weak_update.upgrade() {
            match update_frame_with_slint_native(&state) {
                Ok(Some(image)) => {
                    ui.set_camera_frame(image);
                    update_fps_counter_optimized(&state, &ui);
                }
                Ok(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(e) => {
                    eprintln!("Optimized frame error: {}", e);
                    ui.set_status_text(format!("Frame error: {}", e).into());
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        }
    });

    match refresh_camera_list(&camera_state_initial) {
        Ok(camera_names) => {
            let count = camera_names.len();
            let model: Vec<slint::SharedString> = camera_names.into_iter()
                .map(|name| name.into())
                .collect();
            ui.set_cameras(slint::ModelRc::from(model.as_slice()));
            ui.set_status_text(format!("Ready - {} cameras found", count).into());
        }
        Err(e) => {
            eprintln!("Error initial camera scan: {}", e);
            ui.set_status_text(format!("Error: {}", e).into());
        }
    }

    ui.run()?;
    Ok(())
}

fn refresh_camera_list(state: &CameraState) -> Result<Vec<String>> {
    let mut manager = state.manager.lock();
    manager.scan_cameras()?;

    let cameras = manager.list_cameras();
    let camera_names: Vec<String> = cameras.iter()
        .map(|c| c.name.clone())
        .collect();

    println!("Found {} cameras", camera_names.len());
    Ok(camera_names)
}

fn select_camera_by_name(state: &CameraState, camera_name: &str) -> Result<()> {
    let mut manager = state.manager.lock();
    let cameras = manager.list_cameras();

    for (index, camera) in cameras.iter().enumerate() {
        if camera.name == camera_name {
            manager.select_camera(index)?;
            let mut current = state.current_camera.lock();
            *current = Some(index);
            println!("Selected camera: {}", camera_name);
            return Ok(());
        }
    }

    Err(anyhow::anyhow!("Camera '{}' not found", camera_name))
}

fn start_camera_stream(state: &CameraState) -> Result<()> {
    println!("🎥 Starting camera...");

    let manager = state.manager.lock();
    let camera_index = manager.get_selected_camera()
        .ok_or_else(|| anyhow::anyhow!("No camera selected"))?
        .index;

    drop(manager);

    match VideoStream::new(camera_index) {
        Ok(mut video_stream) => {
            video_stream.start()?;
            println!("✅ Camera started successfully");

            {
                let mut stream_guard = state.stream.lock();
                *stream_guard = Some(video_stream);
            }

            {
                let mut streaming = state.is_streaming.lock();
                *streaming = true;
            }

            Ok(())
        }
        Err(e) => {
            eprintln!("❌ Error creating camera stream: {}", e);
            Err(e)
        }
    }
}

fn stop_camera_stream(state: &CameraState) -> Result<()> {
    {
        let mut streaming = state.is_streaming.lock();
        *streaming = false;
    }

    let mut stream = state.stream.lock();
    if let Some(ref mut video_stream) = *stream {
        video_stream.stop()?;
    }
    *stream = None;

    let mut current = state.current_camera.lock();
    *current = None;

    println!("Camera stream stopped");
    Ok(())
}

fn start_async_frame_handler(state: &CameraState) -> Result<()> {
    let config = AsyncFrameHandlerConfig {
        target_fps: 30.0,
        max_buffer_size: 2,
        frame_skip_threshold: Duration::from_millis(33),
        enable_adaptive_skipping: true,
    };

    let mut handler = AsyncFrameHandler::new(config);
    handler.start()?;

    *state.async_handler.lock() = Some(handler);
    println!("🚀 Async frame handler iniciado");
    Ok(())
}

fn stop_async_frame_handler(state: &CameraState) -> Result<()> {
    let mut handler_guard = state.async_handler.lock();
    if let Some(ref mut handler) = *handler_guard {
        handler.stop()?;
        *handler_guard = None;
        println!("⏹️ Async frame handler detenido");
    }
    Ok(())
}

fn update_frame_with_slint_native(state: &CameraState) -> Result<Option<Image>> {
    let start_time = Instant::now();

    {
        let stats = state.render_performance_stats.lock();
        if stats.should_skip_frame(25.0) {
            drop(stats);
            let mut stats = state.render_performance_stats.lock();
            stats.frames_skipped += 1;
            return Ok(None);
        }
    }

    let buffer = {
        let mut stream = state.stream.lock();
        if let Some(ref mut video_stream) = *stream {
            match video_stream.capture_frame() {
                Ok(buffer) => buffer,
                Err(e) => {
                    eprintln!("Error capturing frame: {}", e);
                    return Err(e);
                }
            }
        } else {
            return Err(anyhow::anyhow!("No video stream available"));
        }
    };

    {
        let handler_guard = state.async_handler.lock();
        if let Some(ref handler) = *handler_guard {
            if let Err(e) = handler.submit_captured_frame(buffer) {
                eprintln!("Error submitting frame: {}", e);
            }
        }
    }

    let handler_guard = state.async_handler.lock();
    if let Some(ref handler) = *handler_guard {
        if let Some(processed_frame) = handler.get_next_frame() {
            let render_time = start_time.elapsed();
            let mut stats = state.render_performance_stats.lock();
            stats.update(render_time);

            return Ok(Some(processed_frame.image));
        }
    }

    Ok(None)
}

fn initialize_slint_renderer(state: &CameraState) -> Result<()> {
    let config = SlintRendererConfig {
        enable_frame_caching: true,
        adaptive_quality: true,
        max_cache_size: 3,
        target_fps: 30.0,
    };

    let mut renderer = SlintRenderer::new(config);

    if let Some(ref mut stream) = *state.stream.lock() {
        if let Ok(buffer) = stream.capture_frame() {
            let resolution = buffer.resolution();
            renderer.initialize(resolution.width() as u32, resolution.height() as u32);
        }
    }

    *state.slint_renderer.lock() = Some(renderer);
    println!("🚀 Slint renderer optimizado inicializado");
    Ok(())
}

fn cleanup_slint_renderer(state: &CameraState) -> Result<()> {
    let mut renderer_guard = state.slint_renderer.lock();
    if let Some(ref mut renderer) = *renderer_guard {
        let metrics = renderer.get_performance_metrics();
        println!("📊 Render stats - Frames: {}, Avg time: {:?}, Cache hit: {:.2}%",
                metrics.total_frames_processed,
                metrics.average_processing_time,
                metrics.cache_hit_rate * 100.0);
        *renderer_guard = None;
    }
    Ok(())
}

fn update_fps_counter_optimized(state: &CameraState, ui: &CameraView) {
    {
        let handler_guard = state.async_handler.lock();
        if let Some(ref handler) = *handler_guard {
            let metrics = handler.get_metrics();
            ui.set_fps(metrics.current_fps as i32);
        }
    }

    {
        let renderer_guard = state.slint_renderer.lock();
        if let Some(ref renderer) = *renderer_guard {
            let metrics = renderer.get_performance_metrics();
            if metrics.total_frames_processed > 0 {
                let fps = 1.0 / metrics.average_processing_time.as_secs_f64();
                ui.set_fps(fps as i32);
            }
        }
    }

    {
        let stats = state.render_performance_stats.lock();
        if stats.current_fps > 0.0 {
            ui.set_fps(stats.current_fps as i32);
        }
    }
}

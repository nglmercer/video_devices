mod nokhwa_camera;
mod buffer_pool;
mod frame_processor;
mod async_frame_handler;
mod slint_renderer;

#[cfg(target_os = "windows")]
mod windows_permissions;

use nokhwa_camera::{NokhwaCameraManager, VideoStream};
// use nokhwa::utils::FrameFormat; // No utilizado
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
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
    performance_mode: Arc<ParkingMutex<bool>>, // true = optimized, false = legacy
    slint_renderer: Arc<ParkingMutex<Option<SlintRenderer>>>, // Nuevo renderizador optimizado
    // last_frame_render_time: Arc<ParkingMutex<Instant>>, // No utilizado
    render_performance_stats: Arc<ParkingMutex<RenderPerformanceStats>>,
}

/// Estadísticas de rendimiento del renderizado
#[derive(Debug, Default)]
struct RenderPerformanceStats {
    frames_rendered: u64,
    frames_skipped: u64,
    total_render_time: Duration,
    average_render_time: Duration,
    current_fps: f64,
    // target_fps: f64, // No utilizado
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
            performance_mode: Arc::new(ParkingMutex::new(true)), // Optimizado por defecto
            slint_renderer: Arc::new(ParkingMutex::new(None)),
            // last_frame_render_time: Arc::new(ParkingMutex::new(Instant::now())), // No utilizado
            render_performance_stats: Arc::new(ParkingMutex::new(RenderPerformanceStats::default())),
        }
    }
}

impl RenderPerformanceStats {
    fn update(&mut self, render_time: Duration) {
        self.frames_rendered += 1;
        self.total_render_time += render_time;
        self.average_render_time = self.total_render_time / self.frames_rendered as u32;
        
        // Calcular FPS actual
        let _now = Instant::now();
        if self.frames_rendered % 30 == 0 { // Actualizar cada 30 frames
            self.current_fps = 30.0 / render_time.as_secs_f64();
        }
    }
    
    fn should_skip_frame(&self, target_fps: f64) -> bool {
        let target_frame_time = Duration::from_secs_f64(1.0 / target_fps);
        // Solo saltar frames si el tiempo de renderizado es significativamente mayor al objetivo
        self.average_render_time > target_frame_time * 12 / 10 // 120% del tiempo objetivo (menos agresivo)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let ui = CameraView::new()?;
    let camera_state = CameraState::new();
    
    // Clone references for callbacks
    let ui_weak_refresh = ui.as_weak();
    let ui_weak_start = ui.as_weak();
    let ui_weak_stop = ui.as_weak();
    let ui_weak_update = ui.as_weak();
    
    let camera_state_refresh = camera_state.clone();
    let camera_state_start = camera_state.clone();
    let camera_state_stop = camera_state.clone();
    let camera_state_update = camera_state.clone();
    let camera_state_initial = camera_state.clone();
    
    // Refresh cameras callback
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
    
    // Start camera callback
    ui.on_start_camera(move || {
        let ui = ui_weak_start.upgrade().unwrap();
        let state = camera_state_start.clone();
        
        // Get selected camera index from ComboBox
        let selected_index = ui.get_selected_camera_index() as usize;
        
        // Select the camera
        {
            let manager = state.manager.lock();
            let cameras = manager.list_cameras();
            
            let accessible_cameras: Vec<_> = cameras.iter()
                .filter(|c| c.accessible)
                .collect();
                
            if let Some(camera) = accessible_cameras.get(selected_index) {
                let camera_name = camera.name.clone();
                drop(manager);
                if let Err(e) = select_camera_by_name(&state, &camera_name) {
                    eprintln!("Error selecting camera: {}", e);
                    ui.set_status_text(format!("Error: {}", e).into());
                    return;
                }
            } else {
                ui.set_status_text("No accessible camera found".into());
                return;
            }
        }
        
        // Start camera stream
        match start_camera_stream(&state) {
            Ok(_) => {
                ui.set_camera_active(true);
                ui.set_status_text("Camera started successfully".into());
                ui.set_is_streaming(true);
                
                // Reset frame counter
                *state.frame_count.lock() = 0;
                *state.last_fps_update.lock() = Instant::now();
                
                // Start async frame handler if in performance mode
                if *state.performance_mode.lock() {
                    if let Err(e) = start_async_frame_handler(&state) {
                        eprintln!("Error starting async handler: {}", e);
                        ui.set_status_text(format!("Async handler error: {}", e).into());
                    }
                    
                    // Inicializar renderizador Slint optimizado
                    if let Err(e) = initialize_slint_renderer(&state) {
                        eprintln!("Error initializing Slint renderer: {}", e);
                        ui.set_status_text(format!("Renderer error: {}", e).into());
                    }
                }
            }
            Err(e) => {
                eprintln!("Error starting camera: {}", e);
                ui.set_status_text(format!("Error: {}", e).into());
            }
        }
    });
    
    // Stop camera callback
    ui.on_stop_camera(move || {
        let ui = ui_weak_stop.upgrade().unwrap();
        let state = camera_state_stop.clone();
        
        // Stop async frame handler if running
        if *state.performance_mode.lock() {
            if let Err(e) = stop_async_frame_handler(&state) {
                eprintln!("Error stopping async handler: {}", e);
            }
            
            // Limpiar renderizador Slint
            if let Err(e) = cleanup_slint_renderer(&state) {
                eprintln!("Error cleaning up renderer: {}", e);
            }
        }
        
        if let Err(e) = stop_camera_stream(&state) {
            eprintln!("Error stopping camera: {}", e);
        }
        
        ui.set_camera_active(false);
        ui.set_status_text("Camera stopped".into());
        ui.set_is_streaming(false);
        ui.set_fps(0);
        
        // Clear the camera frame
        ui.set_camera_frame(Image::from_rgba8(SharedPixelBuffer::new(1, 1)));
    });
    
    // Update frame callback - Called by Slint Timer
    ui.on_update_frame(move || {
        let state = camera_state_update.clone();
        
        // Only update if streaming is active
        if !*state.is_streaming.lock() {
            return;
        }
        
        if let Some(ui) = ui_weak_update.upgrade() {
            // Check performance mode
            let performance_mode = *state.performance_mode.lock();
            
            if performance_mode {
                // Optimized async mode con nuevo renderizador
                match update_frame_optimized(&state) {
                    Ok(Some(image)) => {
                        ui.set_camera_frame(image);
                        update_fps_counter_optimized(&state, &ui);
                    }
                    Ok(None) => {
                        // No frame available (frame skipping inteligente)
                    }
                    Err(e) => {
                        eprintln!("Optimized frame error: {}", e);
                        ui.set_status_text(format!("Frame error: {}", e).into());
                    }
                }
            } else {
                // Legacy sync mode con renderizador mejorado
                match capture_and_convert_frame_enhanced(&state) {
                    Ok(image) => {
                        ui.set_camera_frame(image);
                        update_fps_counter_legacy(&state, &ui);
                    }
                    Err(e) => {
                        eprintln!("Frame capture error: {}", e);
                        ui.set_status_text(format!("Frame error: {}", e).into());
                    }
                }
            }
        }
    });
    
    // Initial camera scan
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
        .filter(|c| c.accessible)
        .map(|c| c.name.clone())
        .collect();
    
    println!("Found {} accessible cameras", camera_names.len());
    Ok(camera_names)
}

fn select_camera_by_name(state: &CameraState, camera_name: &str) -> Result<()> {
    let mut manager = state.manager.lock();
    let cameras = manager.list_cameras();
    
    for (index, camera) in cameras.iter().enumerate() {
        if camera.name == camera_name && camera.accessible {
            manager.select_camera(index)?;
            let mut current = state.current_camera.lock();
            *current = Some(index);
            println!("Selected camera: {}", camera_name);
            return Ok(());
        }
    }
    
    Err(anyhow::anyhow!("Camera '{}' not found or not accessible", camera_name))
}

fn start_camera_stream(state: &CameraState) -> Result<()> {
    println!("🎥 Starting camera...");
    
    let manager = state.manager.lock();
    let camera_index = manager.get_selected_camera()
        .ok_or_else(|| anyhow::anyhow!("No camera selected"))?
        .index;
    
    drop(manager);
    
    // Create and start camera stream
    match VideoStream::new(camera_index) {
        Ok(mut video_stream) => {
            video_stream.start()?;
            println!("✅ Camera started successfully");
            
            // Store the stream
            {
                let mut stream_guard = state.stream.lock();
                *stream_guard = Some(video_stream);
            }
            
            // Set streaming flag
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
    // Set streaming flag to false
    {
        let mut streaming = state.is_streaming.lock();
        *streaming = false;
    }
    
    // Stop and clear the stream
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

/// Inicia el manejador asíncrono de frames optimizado
fn start_async_frame_handler(state: &CameraState) -> Result<()> {
    let config = AsyncFrameHandlerConfig {
        target_fps: 30.0,
        max_buffer_size: 2, // Buffer pequeño para minimizar latencia
        frame_skip_threshold: Duration::from_millis(33), // ~30 FPS
        enable_adaptive_skipping: true,
    };
    
    let mut handler = AsyncFrameHandler::new(config);
    handler.start()?;
    
    *state.async_handler.lock() = Some(handler);
    println!("🚀 Async frame handler iniciado");
    Ok(())
}

/// Detiene el manejador asíncrono de frames
fn stop_async_frame_handler(state: &CameraState) -> Result<()> {
    let mut handler_guard = state.async_handler.lock();
    if let Some(ref mut handler) = *handler_guard {
        handler.stop()?;
        *handler_guard = None;
        println!("⏹️ Async frame handler detenido");
    }
    Ok(())
}

/// Actualiza contador FPS en modo legacy
fn update_fps_counter_legacy(state: &CameraState, ui: &CameraView) {
    let mut frame_count = state.frame_count.lock();
    *frame_count += 1;
    
    let mut last_update = state.last_fps_update.lock();
    let elapsed = last_update.elapsed();
    
    if elapsed >= Duration::from_secs(1) {
        let fps = (*frame_count as f64 / elapsed.as_secs_f64()) as i32;
        ui.set_fps(fps);
        *frame_count = 0;
        *last_update = Instant::now();
    }
}

/// Función legacy de conversión (mantenida para compatibilidad)
fn convert_nokhwa_buffer_to_slint(buffer: &nokhwa::buffer::Buffer) -> Result<Image> {
    let resolution = buffer.resolution();
    let width = resolution.width() as u32;
    let height = resolution.height() as u32;
    
    let image_data = buffer.buffer();
    
    // Calculate expected sizes
    let expected_rgb = (width * height * 3) as usize;
    let expected_rgba = (width * height * 4) as usize;
    
    // Convert to RGBA format
    let rgba_data = if image_data.len() == expected_rgb {
        // RGB format - convert to RGBA with BGR swap (common in Windows)
        image_data
            .chunks_exact(3)
            .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], 255])
            .collect()
    } else if image_data.len() == expected_rgba {
        // Already RGBA
        image_data.to_vec()
    } else {
        // Compressed format (likely MJPEG) - decode it
        decode_jpeg_to_rgba(image_data, width, height)?
    };
    
    let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba_data, width, height);
    Ok(Image::from_rgba8(pixel_buffer))
}

/// Función legacy de decodificación JPEG (mantenida para compatibilidad)
fn decode_jpeg_to_rgba(jpeg_data: &[u8], target_width: u32, target_height: u32) -> Result<Vec<u8>> {
    match image::load_from_memory(jpeg_data) {
        Ok(img) => {
            let rgba_img = img.to_rgba8();
            let resized_img = image::imageops::resize(
                &rgba_img,
                target_width,
                target_height,
                image::imageops::FilterType::Lanczos3
            );
            Ok(resized_img.into_raw())
        }
        Err(e) => {
            Err(anyhow::anyhow!("Failed to decode JPEG: {}", e))
        }
    }
}

/// Inicializa el renderizador Slint optimizado
fn initialize_slint_renderer(state: &CameraState) -> Result<()> {
    let config = SlintRendererConfig {
        enable_simd: true,
        enable_frame_caching: true,
        adaptive_quality: true,
        max_cache_size: 3,
        target_fps: 30.0,
    };
    
    let mut renderer = SlintRenderer::new(config);
    
    // Obtener resolución inicial de la cámara si está disponible
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

/// Limpia el renderizador Slint
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

/// Actualización de frame optimizada con nuevo renderizador
fn update_frame_optimized(state: &CameraState) -> Result<Option<Image>> {
    let start_time = Instant::now();
    
    // Verificar si debemos saltar este frame para mantener rendimiento
    {
        let stats = state.render_performance_stats.lock();
        if stats.should_skip_frame(30.0) {
            drop(stats);
            let mut stats = state.render_performance_stats.lock();
            stats.frames_skipped += 1;
            return Ok(None);
        }
    }
    
    // Capturar frame
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
    
    // Enviar al handler asíncrono para procesamiento en background
    {
        let handler_guard = state.async_handler.lock();
        if let Some(ref handler) = *handler_guard {
            if let Err(e) = handler.submit_captured_frame(buffer) {
                eprintln!("Error submitting frame: {}", e);
            }
        }
    }
    
    // Obtener frame procesado del handler
    let handler_guard = state.async_handler.lock();
    if let Some(ref handler) = *handler_guard {
        if let Some(processed_frame) = handler.get_next_frame() {
            // Para simplificar, usar directamente la imagen procesada
            // El renderizador optimizado ya está integrado en el async handler
            let render_time = start_time.elapsed();
            let mut stats = state.render_performance_stats.lock();
            stats.update(render_time);
            
            return Ok(Some(processed_frame.image));
        }
    }
    
    Ok(None) // No frame disponible
}

/// Función legacy mejorada con renderizador optimizado
fn capture_and_convert_frame_enhanced(state: &CameraState) -> Result<Image> {
    let start_time = Instant::now();
    
    // Capturar frame
    let buffer = {
        let mut stream = state.stream.lock();
        if let Some(ref mut video_stream) = *stream {
            video_stream.capture_frame()?
        } else {
            return Err(anyhow::anyhow!("No video stream available"));
        }
    };
    
    // Intentar usar renderizador optimizado si está disponible
    {
        let mut renderer_guard = state.slint_renderer.lock();
        if let Some(ref mut renderer) = *renderer_guard {
            match renderer.render_frame(&buffer) {
                Ok(image) => {
                    // Actualizar estadísticas
                    let render_time = start_time.elapsed();
                    let mut stats = state.render_performance_stats.lock();
                    stats.update(render_time);
                    
                    return Ok(image);
                }
                Err(e) => {
                    eprintln!("Error in enhanced rendering, falling back: {}", e);
                }
            }
        }
    }
    
    // Fallback a conversión legacy
    convert_nokhwa_buffer_to_slint(&buffer)
}

/// Actualiza contador FPS en modo optimizado
fn update_fps_counter_optimized(state: &CameraState, ui: &CameraView) {
    // Actualizar FPS desde handler asíncrono
    {
        let handler_guard = state.async_handler.lock();
        if let Some(ref handler) = *handler_guard {
            let metrics = handler.get_metrics();
            ui.set_fps(metrics.current_fps as i32);
        }
    }
    
    // Actualizar FPS desde renderizador Slint
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
    
    // Actualizar FPS desde estadísticas locales
    {
        let stats = state.render_performance_stats.lock();
        if stats.current_fps > 0.0 {
            ui.set_fps(stats.current_fps as i32);
        }
    }
}

// Función eliminada - no necesaria para el renderizado optimizado
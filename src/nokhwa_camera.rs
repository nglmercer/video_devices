use anyhow::{anyhow, Result};
use core::fmt;
use slint::Rgba8Pixel;
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use nokhwa::{
    pixel_format::{RgbAFormat, RgbFormat, YuyvFormat},
    utils::{RequestedFormat, RequestedFormatType},
};

pub use nokhwa::utils::CameraIndex as NokhwaIndex;

use crate::slint_renderer;

// Thread-local buffer para evitar contención de locks con pre-allocation
thread_local! {
    static LOCAL_RGBA_BUFFER: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(Vec::with_capacity(1920 * 1080 * 4)); // Pre-allocar para 1080p
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraIndex(pub String);

impl fmt::Display for CameraIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<NokhwaIndex> for CameraIndex {
    fn from(value: NokhwaIndex) -> Self {
        CameraIndex(value.as_string())
    }
}

impl From<CameraIndex> for NokhwaIndex {
    fn from(value: CameraIndex) -> Self {
        NokhwaIndex::String(value.0)
    }
}

impl PartialEq<NokhwaIndex> for CameraIndex {
    fn eq(&self, other: &NokhwaIndex) -> bool {
        match other {
            NokhwaIndex::Index(i) => i.to_string() == self.0,
            NokhwaIndex::String(s) => &self.0 == s,
        }
    }
}

pub fn create_camera(camera_index: NokhwaIndex) -> Result<nokhwa::Camera> {
    println!("🎥 Creando VideoStream para cámara {camera_index}...");

    // Prioridad: RGBA primero (solo reordena canales), luego RGB (expande), finalmente YUYV
    let requested_formats = vec![
        RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestResolution),
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution),
        RequestedFormat::new::<YuyvFormat>(RequestedFormatType::AbsoluteHighestResolution),
    ];

    let mut camera = requested_formats
        .into_iter()
        .enumerate()
        .find_map(|(i, format)| {
            match panic::catch_unwind(|| nokhwa::Camera::new(camera_index.clone(), format)) {
                Ok(Ok(cam)) => {
                    println!("✅ Cámara creada con formato {i}: {format:?}");
                    Some(cam)
                }
                Ok(Err(e)) => {
                    println!("⚠️  Formato {i} falló: {e}");
                    None
                }
                Err(_) => {
                    println!("❌ Panic durante inicialización del formato {i}");
                    None
                }
            }
        })
        .ok_or_else(|| anyhow!("No se pudo crear la cámara con ningún formato soportado"))?;

    camera.open_stream()?;

    Ok(camera)
}

// Buffer circular para frame times - más eficiente que Vec
struct FrameTimeBuffer {
    times: [f64; 60],
    index: usize,
    count: usize,
}

impl FrameTimeBuffer {
    fn new() -> Self {
        Self {
            times: [0.0; 60],
            index: 0,
            count: 0,
        }
    }

    fn push(&mut self, time: f64) {
        self.times[self.index] = time;
        self.index = (self.index + 1) % 60;
        if self.count < 60 {
            self.count += 1;
        }
    }

    fn clear(&mut self) {
        self.index = 0;
        self.count = 0;
    }

    fn average(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let sum: f64 = self.times.iter().take(self.count).sum();
        self.count as f64 / sum
    }
}

pub fn create_camera_stream<C>(mut camera: nokhwa::Camera, mut callback: C) -> CameraAbort
where
    C: FnMut(Result<(slint::SharedPixelBuffer<Rgba8Pixel>, f64)>) + Send + 'static,
{
    let abort = Arc::new(AtomicBool::new(false));
    let camera_abort = CameraAbort(abort.clone());

    std::thread::spawn(move || {
        let mut frame_times = FrameTimeBuffer::new();
        let mut last_fps_update = Instant::now();
        let mut cached_fps = 0.0;
        let target_frame_time = std::time::Duration::from_millis(16); // ~60 FPS
        
        loop {
            let frame_start = Instant::now();
            let result = camera_frame(&mut camera);
            
            // Calcular tiempo de procesamiento inmediatamente después para precisión
            let processing_time = frame_start.elapsed();
            
            match &result {
                Ok((_, fps)) => {
                    frame_times.push(1.0 / fps);
                }
                Err(_) => frame_times.clear(),
            }
            
            // Calcular FPS promedio solo cada segundo para reducir overhead
            let now = Instant::now();
            if now.duration_since(last_fps_update).as_secs() >= 1 {
                cached_fps = frame_times.average();
                last_fps_update = now;
            }
            
            callback(result.map(|(frame, _)| (frame, cached_fps)));

            if abort.load(Ordering::Relaxed) {
                break;
            }
            
            // Throttling adaptativo mejorado: dormir solo si es necesario
            // Usar processing_time calculado antes para mayor precisión
            if processing_time < target_frame_time {
                let sleep_time = target_frame_time - processing_time;
                // Reducir umbral a 500μs para mejor responsividad
                if sleep_time > std::time::Duration::from_micros(500) {
                    std::thread::sleep(sleep_time);
                }
            }
        }
    });

    camera_abort
}

fn camera_frame(
    camera: &mut nokhwa::Camera,
) -> Result<(slint::SharedPixelBuffer<Rgba8Pixel>, f64)> {
    let start_time = Instant::now();

    let buffer = camera
        .frame()
        .map_err(|e| anyhow!("Capturing frame: {e}"))?;

    // Usar thread-local buffer con pre-allocation inteligente
    let image = LOCAL_RGBA_BUFFER.with(|local_buffer| {
        let mut temp_buffer = local_buffer.borrow_mut();
        
        slint_renderer::render_frame_with_buffer(&mut temp_buffer, &buffer)
    })?;

    let render_time = start_time.elapsed();
    let fps = 1.0 / render_time.as_secs_f64();

    Ok((image, fps))
}

// Optimización: usar AtomicBool con Ordering más eficiente
pub struct CameraAbort(Arc<AtomicBool>);

impl CameraAbort {
    pub fn abort(self) {
        // Usar Release ordering para asegurar que todos los writes anteriores sean visibles
        self.0.store(true, Ordering::Release);
    }
}

impl Drop for CameraAbort {
    fn drop(&mut self) {
        // Relaxed es suficiente aquí ya que es el último acceso
        self.0.store(true, Ordering::Relaxed);
    }
}

// Asegurar que el hilo se limpie adecuadamente
#[allow(dead_code)]
impl CameraAbort {
    pub fn wait_for_completion(&self, timeout: std::time::Duration) -> bool {
        let start = Instant::now();
        while !self.0.load(Ordering::Acquire) {
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        true
    }
}

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

pub fn create_camera_stream<C>(mut camera: nokhwa::Camera, mut callback: C) -> CameraAbort
where
    C: FnMut(Result<(slint::SharedPixelBuffer<Rgba8Pixel>, f64)>) + Send + 'static,
{
    let abort = Arc::new(AtomicBool::new(false));
    let camera_abort = CameraAbort(abort.clone());

    std::thread::spawn(move || {
        let mut frame_times: Vec<f64> = Vec::with_capacity(60);
        let mut last_fps_update = Instant::now();
        let mut cached_fps = 0.0;
        
        loop {
            let frame_start = Instant::now();
            let result = camera_frame(&mut camera);
            
            match &result {
                Ok((_, fps)) => {
                    frame_times.push(1.0 / fps);
                    if frame_times.len() > 60 {
                        frame_times.remove(0);
                    }
                }
                Err(_) => frame_times.clear(),
            }
            
            // Calcular FPS promedio solo cada segundo para reducir overhead
            let now = Instant::now();
            if now.duration_since(last_fps_update).as_secs() >= 1 {
                cached_fps = if frame_times.is_empty() {
                    0.0
                } else {
                    frame_times.len() as f64 / frame_times.iter().sum::<f64>()
                };
                last_fps_update = now;
            }
            
            callback(result.map(|(frame, _)| (frame, cached_fps)));

            if abort.load(Ordering::Relaxed) {
                break;
            }
            
            // Throttling: dormir brevemente para reducir uso de CPU
            // Esto permite que el hilo no consuma 100% CPU cuando la cámara
            // no puede mantener altos framerates
            let elapsed = frame_start.elapsed();
            if elapsed.as_millis() < 16 { // Aprox 60 FPS max
                std::thread::sleep(std::time::Duration::from_millis(1));
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

    let image = slint_renderer::render_frame(&buffer)?;

    let render_time = start_time.elapsed();
    let fps = 1.0 / render_time.as_secs_f64();

    Ok((image, fps))
}

pub struct CameraAbort(Arc<AtomicBool>);

impl CameraAbort {
    pub fn abort(self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl Drop for CameraAbort {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

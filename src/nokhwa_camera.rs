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

    let requested_formats = vec![
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution),
        RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestResolution),
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

    std::thread::spawn(move || loop {
        callback(camera_frame(&mut camera));

        if abort.load(Ordering::SeqCst) {
            break;
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

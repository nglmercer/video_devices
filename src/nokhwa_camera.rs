use anyhow::{anyhow, Result};
use core::fmt;
use std::panic;

use nokhwa::{
    pixel_format::{RgbAFormat, RgbFormat, YuyvFormat},
    utils::{RequestedFormat, RequestedFormatType},
};

pub use nokhwa::utils::CameraIndex as NokhwaIndex;

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

pub fn create_camera_stream(camera_index: NokhwaIndex) -> Result<nokhwa::Camera> {
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

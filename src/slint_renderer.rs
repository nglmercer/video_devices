//! Renderizador Slint optimizado para modo Direct
//!
//! Este módulo proporciona conversión ultrarrápida de buffers de nokhwa
//! a imágenes Slint utilizando métodos nativos para máxima velocidad.

use anyhow::Result;
use nokhwa::buffer::Buffer;
use slint::{Image, SharedPixelBuffer, Rgb8Pixel, Rgba8Pixel};
use std::sync::Arc;
use std::time::{Duration, Instant};
use crate::buffer_pool::{get_buffer_pool_manager, RgbaBufferPool};

/// Configuración del renderizador Slint
#[derive(Debug, Clone)]
pub struct SlintRendererConfig {
    pub target_fps: f64,
}

impl Default for SlintRendererConfig {
    fn default() -> Self {
        Self {
            target_fps: 60.0, // Mayor FPS para menor delay
        }
    }
}

/// Renderizador optimizado para modo Direct
pub struct SlintRenderer {
    config: SlintRendererConfig,
    rgba_pool: Option<Arc<RgbaBufferPool>>,
    total_frames_processed: u64,
    total_processing_time: Duration,
}

impl SlintRenderer {
    /// Crea un nuevo renderizador
    pub fn new(config: SlintRendererConfig) -> Self {
        Self {
            config,
            rgba_pool: None,
            total_frames_processed: 0,
            total_processing_time: Duration::ZERO,
        }
    }

    /// Inicializa el renderizador con resolución específica
    pub fn initialize(&mut self, width: u32, height: u32) {
        let pool_manager = get_buffer_pool_manager();
        let pool = pool_manager.get_rgba_pool(width, height);
        self.rgba_pool = Some(pool);
    }

    /// Renderiza un frame usando métodos nativos de Slint
    pub fn render_frame(&mut self, buffer: &Buffer) -> Result<Image> {
        let start_time = Instant::now();

        // Actualizar pool si es necesario
        let resolution = buffer.resolution();
        let width = resolution.width() as u32;
        let height = resolution.height() as u32;

        if self.rgba_pool.is_none() ||
           self.rgba_pool.as_ref().unwrap().buffer_size() != (width * height * 4) as usize {
            self.initialize(width, height);
        }

        let image_data = buffer.buffer();

        // Calcular tamaños esperados
        let expected_rgb = (width * height * 3) as usize;
        let expected_rgba = (width * height * 4) as usize;

        let image = if image_data.len() == expected_rgb {
            // RGB → usar from_rgb8 nativo con conversión BGR→RGB
            let rgb_data = self.convert_bgr_to_rgb_native_optimized(image_data)?;
            let pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(&rgb_data, width, height);
            Image::from_rgb8(pixel_buffer)

        } else if image_data.len() == expected_rgba {
            // RGBA → usar from_rgba8 nativo con conversión BGRA→RGBA
            let rgba_data = self.convert_bgra_to_rgba_native_optimized(image_data)?;
            let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba_data, width, height);
            Image::from_rgba8(pixel_buffer)

        } else {
            // MJPEG → decodificar y usar métodos nativos
            self.decode_to_slint_native_optimized(image_data, width, height)?
        };

        let render_time = start_time.elapsed();
        self.update_metrics(render_time);

        Ok(image)
    }

    /// Conversión BGR -> RGB optimizada usando buffer pool
    fn convert_bgr_to_rgb_native_optimized(&self, bgr_data: &[u8]) -> Result<Vec<u8>> {
        let pool = self.rgba_pool.as_ref().unwrap();
        let mut rgb_buffer = pool.get_buffer();

        let pixel_count = bgr_data.len() / 3;
        rgb_buffer.resize(pixel_count * 3, 0);

        // Para resoluciones bajas, usar procesamiento secuencial
        if pixel_count <= 640 * 480 {
            for i in 0..pixel_count {
                let bgr_idx = i * 3;
                let rgb_idx = i * 3;

                rgb_buffer[rgb_idx] = bgr_data[bgr_idx + 2];     // R
                rgb_buffer[rgb_idx + 1] = bgr_data[bgr_idx + 1]; // G
                rgb_buffer[rgb_idx + 2] = bgr_data[bgr_idx];     // B
            }
        } else {
            // Para resoluciones altas, procesar en chunks
            for (i, chunk) in bgr_data.chunks_exact(3).enumerate() {
                let rgb_idx = i * 3;

                rgb_buffer[rgb_idx] = chunk[2];     // R
                rgb_buffer[rgb_idx + 1] = chunk[1]; // G
                rgb_buffer[rgb_idx + 2] = chunk[0]; // B
            }
        }

        Ok(rgb_buffer)
    }

    /// Conversión BGRA -> RGBA optimizada usando buffer pool
    fn convert_bgra_to_rgba_native_optimized(&self, bgra_data: &[u8]) -> Result<Vec<u8>> {
        let pool = self.rgba_pool.as_ref().unwrap();
        let mut rgba_buffer = pool.get_buffer();

        let pixel_count = bgra_data.len() / 4;
        rgba_buffer.resize(pixel_count * 4, 0);

        // Procesamiento optimizado por chunks
        for (i, chunk) in bgra_data.chunks_exact(4).enumerate() {
            let rgba_idx = i * 4;

            rgba_buffer[rgba_idx] = chunk[2];     // R
            rgba_buffer[rgba_idx + 1] = chunk[1]; // G
            rgba_buffer[rgba_idx + 2] = chunk[0]; // B
            rgba_buffer[rgba_idx + 3] = chunk[3]; // A
        }

        Ok(rgba_buffer)
    }

    /// Decodificación MJPEG optimizada para Slint
    fn decode_to_slint_native_optimized(&mut self, image_data: &[u8], width: u32, height: u32) -> Result<Image> {
        // Intentar decodificar como JPEG
        match image::load_from_memory(image_data) {
            Ok(decoded_image) => {
                // Redimensionar si es necesario
                let final_img = if decoded_image.width() != width || decoded_image.height() != height {
                    decoded_image.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
                } else {
                    decoded_image
                };

                // Convertir a RGBA8 para Slint
                let rgba_image = final_img.to_rgba8();
                let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    rgba_image.as_raw(),
                    width,
                    height
                );
                Ok(Image::from_rgba8(pixel_buffer))
            }
            Err(_) => {
                // Si no es JPEG, crear placeholder
                let pixel_count = (width * height) as usize;
                let rgba_data = vec![128u8; pixel_count * 4]; // Gris

                Ok(Image::from_rgba8(SharedPixelBuffer::clone_from_slice(
                    &rgba_data,
                    width,
                    height
                )))
            }
        }
    }

    /// Actualiza métricas de rendimiento
    fn update_metrics(&mut self, render_time: Duration) {
        self.total_frames_processed += 1;
        self.total_processing_time += render_time;
    }

    /// Obtiene métricas de rendimiento
    pub fn get_performance_metrics(&self) -> SlintRendererMetrics {
        SlintRendererMetrics {
            total_frames_processed: self.total_frames_processed,
            average_processing_time: if self.total_frames_processed > 0 {
                self.total_processing_time / self.total_frames_processed as u32
            } else {
                Duration::ZERO
            },
            cache_hit_rate: 0.0, // No hay cache en modo Direct
            adaptive_quality_level: 1.0, // Calidad máxima en modo Direct
        }
    }

    /// Reinicia las métricas
    pub fn reset_metrics(&mut self) {
        self.total_frames_processed = 0;
        self.total_processing_time = Duration::ZERO;
    }
}

/// Métricas de rendimiento del renderizador
#[derive(Debug)]
pub struct SlintRendererMetrics {
    pub total_frames_processed: u64,
    pub average_processing_time: Duration,
    pub cache_hit_rate: f64, // Mantenido para compatibilidad
    pub adaptive_quality_level: f32, // Mantenido para compatibilidad
}

/// Función de conveniencia para renderizado rápido
pub fn render_frame_optimized(buffer: &nokhwa::buffer::Buffer) -> Result<Image> {
    let mut renderer = SlintRenderer::new(SlintRendererConfig::default());
    renderer.render_frame(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slint_renderer_creation() {
        let config = SlintRendererConfig::default();
        let renderer = SlintRenderer::new(config);

        assert_eq!(renderer.config.target_fps, 60.0);
        assert_eq!(renderer.total_frames_processed, 0);
    }

    #[test]
    fn test_renderer_metrics() {
        let config = SlintRendererConfig::default();
        let mut renderer = SlintRenderer::new(config);

        let metrics = renderer.get_performance_metrics();
        assert_eq!(metrics.total_frames_processed, 0);
        assert_eq!(metrics.average_processing_time, Duration::ZERO);

        renderer.reset_metrics();
        assert_eq!(renderer.total_frames_processed, 0);
    }

    #[test]
    fn test_render_frame_function() {
        // Test de la función de conveniencia
        assert!(render_frame_optimized.is_ok());
    }
}

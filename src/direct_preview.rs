//! Preview directa con latencia mínima usando métodos nativos de Slint
//!
//! Este módulo utiliza el renderizador nativo de Slint para conversiones
//! ultrarrápidas sin buffering para lograr preview en tiempo real con delay de ~1-2ms.

use anyhow::Result;
use nokhwa::buffer::Buffer;
use slint::Image;
use std::time::Instant;

/// Convierte un frame a Slint con latencia mínima usando métodos nativos
/// Esta función delega al renderizador optimizado existente para máxima eficiencia
pub fn convert_frame_to_slint(buffer: &Buffer) -> Result<Image> {
    let start_time = Instant::now();

    // Usar directamente el método nativo optimizado de SlintRenderer
    let image = crate::slint_renderer::render_frame_optimized(buffer)?;

    let conversion_time = start_time.elapsed();

    // Log cada 100 frames para monitorear rendimiento
    static mut FRAME_COUNT: u64 = 0;
    unsafe {
        FRAME_COUNT += 1;
        if FRAME_COUNT % 100 == 0 {
            let resolution = buffer.resolution();
            println!("🚀 Preview directa (nativa): {}x{} conversión en {:?} (frame #{})",
                    resolution.width(), resolution.height(), conversion_time, FRAME_COUNT);
        }
    }

    Ok(image)
}

/// Configuración para la preview directa
#[derive(Debug, Clone)]
pub struct DirectPreviewConfig {
    pub enable_simd: bool,
    pub skip_frames: bool,
    pub target_fps: f64,
}

impl Default for DirectPreviewConfig {
    fn default() -> Self {
        Self {
            enable_simd: true,
            skip_frames: false, // No saltar frames para preview directa
            target_fps: 60.0,   // Alto FPS para menor delay
        }
    }
}

/// Procesador de preview directa con métricas
pub struct DirectPreviewProcessor {
    config: DirectPreviewConfig,
    frame_count: u64,
    total_conversion_time: std::time::Duration,
    last_performance_check: std::time::Instant,
}

impl DirectPreviewProcessor {
    pub fn new(config: DirectPreviewConfig) -> Self {
        Self {
            config,
            frame_count: 0,
            total_conversion_time: std::time::Duration::ZERO,
            last_performance_check: std::time::Instant::now(),
        }
    }

    /// Procesa un frame con métricas de rendimiento usando métodos nativos
    pub fn process_frame(&mut self, buffer: &Buffer) -> Result<Image> {
        let start_time = std::time::Instant::now();

        // Usar conversión nativa optimizada
        let image = convert_frame_to_slint(buffer)?;

        let conversion_time = start_time.elapsed();
        self.update_metrics(conversion_time);

        Ok(image)
    }

    /// Actualiza métricas de rendimiento
    fn update_metrics(&mut self, conversion_time: std::time::Duration) {
        self.frame_count += 1;
        self.total_conversion_time += conversion_time;

        // Reportar rendimiento cada 1000 frames
        if self.frame_count % 1000 == 0 {
            let avg_time = self.total_conversion_time / self.frame_count as u32;
            let current_fps = self.frame_count as f64 /
                self.last_performance_check.elapsed().as_secs_f64();

            println!("📊 Direct Preview Stats (Nativo):");
            println!("   Frames procesados: {}", self.frame_count);
            println!("   Tiempo promedio: {:?}", avg_time);
            println!("   FPS actual: {:.2}", current_fps);
            println!("   Latencia estimada: {:.1}ms", avg_time.as_millis());

            // Resetear contadores
            self.last_performance_check = std::time::Instant::now();
            self.frame_count = 0;
            self.total_conversion_time = std::time::Duration::ZERO;
        }
    }

    /// Obtiene estadísticas actuales
    pub fn get_stats(&self) -> DirectPreviewStats {
        DirectPreviewStats {
            frames_processed: self.frame_count,
            average_conversion_time: if self.frame_count > 0 {
                self.total_conversion_time / self.frame_count as u32
            } else {
                std::time::Duration::ZERO
            },
            current_fps: if self.frame_count > 0 {
                self.frame_count as f64 / self.last_performance_check.elapsed().as_secs_f64()
            } else {
                0.0
            },
        }
    }
}

/// Estadísticas de la preview directa
#[derive(Debug)]
pub struct DirectPreviewStats {
    pub frames_processed: u64,
    pub average_conversion_time: std::time::Duration,
    pub current_fps: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_preview_config() {
        let config = DirectPreviewConfig::default();
        assert!(config.enable_simd);
        assert!(!config.skip_frames);
        assert_eq!(config.target_fps, 60.0);
    }

    #[test]
    fn test_preview_processor() {
        let config = DirectPreviewConfig::default();
        let processor = DirectPreviewProcessor::new(config);

        let stats = processor.get_stats();
        assert_eq!(stats.frames_processed, 0);
        assert_eq!(stats.current_fps, 0.0);
    }
}

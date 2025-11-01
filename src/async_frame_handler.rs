//! Módulo simplificado - AsyncFrameHandler no utilizado en modo Direct
//!
//! Este módulo mantiene la compatibilidad pero las funciones son no-op
//! para el modo de preview directa con latencia mínima.

use anyhow::Result;

// Estructuras simplificadas para compatibilidad (no utilizadas en modo Direct)
#[allow(dead_code)]
pub struct ProcessedFrame {
    pub image: slint::Image,
    pub timestamp: std::time::Instant,
    pub frame_number: u64,
    pub processing_time: std::time::Duration,
}

// Estructuras simplificadas para compatibilidad (no utilizadas en modo Direct)
#[allow(dead_code)]
pub struct AsyncFrameHandler {
    // No-op implementation para modo Direct
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct PerformanceMetrics {
    pub current_fps: f64,
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct AsyncFrameHandlerConfig {
    pub target_fps: f64,
}

impl AsyncFrameHandler {
    pub fn new(_config: AsyncFrameHandlerConfig) -> Self {
        Self {}
    }

    pub fn start(&mut self) -> Result<()> {
        // No-op para modo Direct
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        // No-op para modo Direct
        Ok(())
    }

    pub fn submit_captured_frame(&self, _buffer: nokhwa::buffer::Buffer) -> Result<()> {
        // No-op para modo Direct
        Ok(())
    }

    pub fn get_next_frame(&self) -> Option<ProcessedFrame> {
        // No-op para modo Direct
        None
    }

    pub fn get_metrics(&self) -> PerformanceMetrics {
        PerformanceMetrics::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_async_frame_handler_creation() {
        let config = AsyncFrameHandlerConfig::default();
        let handler = AsyncFrameHandler::new(config);

        assert_eq!(handler.config.target_fps, 30.0);
        assert_eq!(handler.config.max_buffer_size, 3);
    }

    #[test]
    fn test_performance_metrics() {
        let mut metrics = PerformanceMetrics::new(30.0);

        metrics.update_with_frame(Duration::from_millis(10));
        assert_eq!(metrics.frames_processed, 1);
        assert_eq!(metrics.average_processing_time, Duration::from_millis(10));

        metrics.update_fps(25);
        assert_eq!(metrics.current_fps, 25.0);
    }

    #[tokio::test]
    async fn test_frame_handler_lifecycle() {
        let mut handler = AsyncFrameHandler::new(AsyncFrameHandlerConfig::default());

        // Iniciar
        assert!(handler.start().is_ok());
        assert!(*handler.is_running.lock());

        // Detener
        assert!(handler.stop().is_ok());
        assert!(!*handler.is_running.lock());
    }
}

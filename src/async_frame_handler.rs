//! Manejador asíncrono de frames con channels y frame skipping inteligente
//! 
//! Este módulo implementa:
//! - Separación de captura y visualización con tokio
//! - Frame skipping adaptativo para mantener 30 FPS
//! - Canales de comunicación sin bloqueo
//! - Métricas de rendimiento en tiempo real

use anyhow::Result;
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use slint::Image;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// Frame procesado listo para visualización
#[derive(Debug, Clone)]
pub struct ProcessedFrame {
    pub image: Image,
    pub timestamp: Instant,
    #[allow(dead_code)] // Mantenido para tracking futuro
    pub frame_number: u64,
    pub processing_time: Duration,
}

// Implementar Send para ProcessedFrame
unsafe impl Send for ProcessedFrame {}

/// Métricas de rendimiento del procesamiento de frames
#[derive(Debug, Default)]
pub struct PerformanceMetrics {
    pub frames_captured: u64,
    pub frames_processed: u64,
    pub frames_displayed: u64,
    pub frames_dropped: u64,
    pub average_processing_time: Duration,
    pub average_capture_time: Duration,
    pub current_fps: f64,
    pub target_fps: f64,
}

impl PerformanceMetrics {
    pub fn new(target_fps: f64) -> Self {
        Self {
            target_fps,
            ..Default::default()
        }
    }
    
    /// Actualiza las métricas con un nuevo frame procesado
    pub fn update_with_frame(&mut self, processing_time: Duration) {
        self.frames_processed += 1;
        
        // Media móvil del tiempo de procesamiento
        let total_time = self.average_processing_time * (self.frames_processed - 1) as u32 + processing_time;
        self.average_processing_time = total_time / self.frames_processed as u32;
    }
    
    /// Calcula el FPS actual basado en los frames mostrados
    #[allow(dead_code)] // Mantenido para métricas futuras
    pub fn update_fps(&mut self, frames_in_last_second: u32) {
        self.current_fps = frames_in_last_second as f64;
    }
}

/// Configuración del manejador asíncrono
pub struct AsyncFrameHandlerConfig {
    pub target_fps: f64,
    pub max_buffer_size: usize,
    #[allow(dead_code)] // Mantenido para configuración avanzada
    pub frame_skip_threshold: Duration,
    pub enable_adaptive_skipping: bool,
}

impl Default for AsyncFrameHandlerConfig {
    fn default() -> Self {
        Self {
            target_fps: 30.0,
            max_buffer_size: 2, // Buffer más pequeño para minimizar latencia
            frame_skip_threshold: Duration::from_millis(16), // ~60 FPS base rate
            enable_adaptive_skipping: true,
        }
    }
}

/// Manejador asíncrono de frames con optimizaciones
pub struct AsyncFrameHandler {
    /// Configuración del manejador
    config: AsyncFrameHandlerConfig,
    
    /// Canal para frames capturados (captura → procesamiento)
    capture_sender: Sender<nokhwa::buffer::Buffer>,
    capture_receiver: Receiver<nokhwa::buffer::Buffer>,
    
    /// Canal asíncrono para frames procesados (procesamiento → visualización)
    processed_sender: crossbeam_channel::Sender<ProcessedFrame>,
    processed_receiver: Arc<Mutex<Option<crossbeam_channel::Receiver<ProcessedFrame>>>>,
    
    /// Tarea de procesamiento en background
    processing_task: Option<JoinHandle<()>>,
    
    /// Métricas de rendimiento
    metrics: Arc<Mutex<PerformanceMetrics>>,
    
    /// Control del frame skipping
    last_display_time: Arc<Mutex<Instant>>,
    frame_skip_counter: Arc<Mutex<u32>>,
    
    /// Estado del manejador
    is_running: Arc<Mutex<bool>>,
}

impl AsyncFrameHandler {
    /// Crea un nuevo manejador asíncrono
    pub fn new(config: AsyncFrameHandlerConfig) -> Self {
        let (capture_sender, capture_receiver) = bounded(config.max_buffer_size);
        let (processed_sender, processed_receiver) = crossbeam_channel::unbounded();
        
        let metrics = Arc::new(Mutex::new(PerformanceMetrics::new(config.target_fps)));
        let last_display_time = Arc::new(Mutex::new(Instant::now()));
        let frame_skip_counter = Arc::new(Mutex::new(0));
        let is_running = Arc::new(Mutex::new(false));
        
        Self {
            config,
            capture_sender,
            capture_receiver,
            processed_sender,
            processed_receiver: Arc::new(Mutex::new(Some(processed_receiver))),
            processing_task: None,
            metrics,
            last_display_time,
            frame_skip_counter,
            is_running,
        }
    }
    
    /// Inicia el procesamiento asíncrono de frames
    pub fn start(&mut self) -> Result<()> {
        if *self.is_running.lock() {
            return Ok(());
        }
        
        *self.is_running.lock() = true;
        
        // Iniciar tarea de procesamiento en background
        let capture_receiver = self.capture_receiver.clone();
        let processed_sender = self.processed_sender.clone();
        let metrics = self.metrics.clone();
        let is_running = self.is_running.clone();
        
        self.processing_task = Some(tokio::task::spawn_blocking(move || {
            let mut frame_number = 0u64;
            let mut last_process_time = Instant::now();
            
            while *is_running.lock() {
                match capture_receiver.recv_timeout(Duration::from_millis(5)) {
                    Ok(buffer) => {
                        let start_time = Instant::now();
                        
                        // Verificar si debemos procesar este frame (adaptive skipping mucho menos agresivo)
                        let time_since_last = start_time.duration_since(last_process_time);
                        let target_frame_time = Duration::from_secs_f64(1.0 / 30.0); // 30 FPS objetivo
                        
                        // NUNCA saltar frames - siempre procesar para máxima fluidez
                        // Solo limitar si es extremadamente rápido (más de 60 FPS)
                        if time_since_last >= target_frame_time * 5 / 10 { // 50% del tiempo objetivo (casi siempre)
                            // Procesar frame con renderizador optimizado
                            let image = match crate::slint_renderer::render_frame_optimized(&buffer) {
                                Ok(img) => img,
                                Err(e) => {
                                    eprintln!("Error en renderizado optimizado: {}", e);
                                    // Fallback a procesador estándar
                                    match crate::frame_processor::process_frame_optimized(&buffer) {
                                        Ok(img) => img,
                                        Err(e2) => {
                                            eprintln!("Error en procesamiento fallback: {}", e2);
                                            continue;
                                        }
                                    }
                                }
                            };
                            
                            let processing_time = start_time.elapsed();
                            
                            let processed_frame = ProcessedFrame {
                                image,
                                timestamp: Instant::now(),
                                frame_number,
                                processing_time,
                            };
                            
                            // Enviar frame procesado sin bloquear
                            match processed_sender.try_send(processed_frame) {
                                Ok(()) => {
                                    last_process_time = Instant::now();
                                    metrics.lock().update_with_frame(processing_time);
                                    frame_number += 1;
                                }
                                Err(crossbeam_channel::TrySendError::Full(_)) => {
                                    // Buffer lleno, descartar frame
                                    metrics.lock().frames_dropped += 1;
                                }
                                Err(_) => break, // Canal cerrado
                            }
                        } else {
                            // Frame saltado para mantener rendimiento
                            metrics.lock().frames_dropped += 1;
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        // Timeout normal, continuar
                        continue;
                    }
                    Err(_) => {
                        // Canal cerrado
                        break;
                    }
                }
            }
        }));
        
        println!("🚀 AsyncFrameHandler iniciado con target FPS: {}", self.config.target_fps);
        Ok(())
    }
    
    /// Detiene el procesamiento asíncrono
    pub fn stop(&mut self) -> Result<()> {
        *self.is_running.lock() = false;
        
        // Detener tarea de procesamiento
        if let Some(task) = self.processing_task.take() {
            task.abort();
        }
        
        println!("⏹️ AsyncFrameHandler detenido");
        Ok(())
    }
    
    /// Envía un frame capturado para su procesamiento
    pub fn submit_captured_frame(&self, buffer: nokhwa::buffer::Buffer) -> Result<()> {
        self.metrics.lock().frames_captured += 1;
        
        match self.capture_sender.try_send(buffer) {
            Ok(()) => Ok(()),
            Err(crossbeam_channel::TrySendError::Full(_)) => {
                // Buffer lleno, descartar frame sin error para evitar spam
                self.metrics.lock().frames_dropped += 1;
                Ok(()) // Continuar sin error para no saturar logs
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                Err(anyhow::anyhow!("Canal de captura desconectado"))
            }
        }
    }
    
    /// Obtiene el siguiente frame procesado para visualización
    pub fn get_next_frame(&self) -> Option<ProcessedFrame> {
        let mut receiver_guard = self.processed_receiver.lock();
        if let Some(ref receiver) = *receiver_guard {
            match receiver.try_recv() {
                Ok(frame) => {
                    // Verificar frame skipping adaptativo
                    if self.should_display_frame(&frame) {
                        self.metrics.lock().frames_displayed += 1;
                        Some(frame)
                    } else {
                        self.metrics.lock().frames_dropped += 1;
                        None // Frame descartado por frame skipping
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => None,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    *receiver_guard = None;
                    None
                }
            }
        } else {
            None
        }
    }
    
    /// Determina si un frame debe ser mostrado basado en frame skipping optimizado
    fn should_display_frame(&self, frame: &ProcessedFrame) -> bool {
        if !self.config.enable_adaptive_skipping {
            return true;
        }
        
        let mut last_time = self.last_display_time.lock();
        let time_since_last_display = frame.timestamp.duration_since(*last_time);
        
        // Target ultra permisivo para máxima fluidez
        let target_frame_duration = Duration::from_secs_f64(1.0 / 30.0); // 30 FPS objetivo
        
        // Adaptive skipping ultra permisivo - casi nunca saltar frames
        let adjusted_threshold = if frame.processing_time > Duration::from_millis(100) {
            target_frame_duration * 3 // Solo si es extremadamente lento (>100ms)
        } else {
            target_frame_duration * 1 // Permitir todos los frames normalmente
        };
        
        if time_since_last_display >= adjusted_threshold {
            *last_time = frame.timestamp;
            true
        } else {
            // Frame skipping: incrementar contador
            *self.frame_skip_counter.lock() += 1;
            false
        }
    }
    
    /// Obtiene las métricas actuales de rendimiento
    pub fn get_metrics(&self) -> PerformanceMetrics {
        let metrics = self.metrics.lock();
        PerformanceMetrics {
            frames_captured: metrics.frames_captured,
            frames_processed: metrics.frames_processed,
            frames_displayed: metrics.frames_displayed,
            frames_dropped: metrics.frames_dropped,
            average_processing_time: metrics.average_processing_time,
            average_capture_time: metrics.average_capture_time,
            current_fps: metrics.current_fps,
            target_fps: metrics.target_fps,
        }
    }
    
    /// Reinicia las métricas de rendimiento
    #[allow(dead_code)] // Mantenido para API completa
    pub fn reset_metrics(&self) {
        let mut metrics = self.metrics.lock();
        *metrics = PerformanceMetrics::new(self.config.target_fps);
        *self.frame_skip_counter.lock() = 0;
        *self.last_display_time.lock() = Instant::now();
    }
    
    /// Actualiza el FPS objetivo dinámicamente
    #[allow(dead_code)] // Mantenido para API completa
    pub fn set_target_fps(&mut self, target_fps: f64) {
        self.config.target_fps = target_fps;
        self.metrics.lock().target_fps = target_fps;
    }
}

impl Drop for AsyncFrameHandler {
    fn drop(&mut self) {
        let _ = self.stop();
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
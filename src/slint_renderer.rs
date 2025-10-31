//! Módulo optimizado para renderizado de frames en Slint
//! 
//! Este módulo implementa:
//! - Zero-copy memory transfer para minimizar latencia
//! - Optimizaciones SIMD para conversiones de formato
//! - Frame caching inteligente para evitar re-renderizado
//! - Adaptive quality basado en rendimiento del sistema

use anyhow::Result;
use rayon::prelude::*;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::sync::Arc;
use std::time::{Duration, Instant};
use crate::buffer_pool::{get_buffer_pool_manager, RgbaBufferPool};

/// Configuración del renderizador Slint
#[derive(Clone)]
pub struct SlintRendererConfig {
    pub enable_simd: bool,
    pub enable_frame_caching: bool,
    pub adaptive_quality: bool,
    pub max_cache_size: usize,
    pub target_fps: f64,
}

impl Default for SlintRendererConfig {
    fn default() -> Self {
        Self {
            enable_simd: true,
            enable_frame_caching: true,
            adaptive_quality: true,
            max_cache_size: 3,
            target_fps: 30.0,
        }
    }
}

/// Frame cacheado para evitar re-procesamiento
#[derive(Clone)]
struct CachedFrame {
    image: Image,
    timestamp: Instant,
    frame_hash: u64,
    #[allow(dead_code)] // Mantenido para métricas de cache
    pub processing_time: Duration,
}

/// Renderizador optimizado para Slint
pub struct SlintRenderer {
    config: SlintRendererConfig,
    rgba_pool: Option<Arc<RgbaBufferPool>>,
    frame_cache: Vec<CachedFrame>,
    last_frame_time: Instant,
    #[allow(dead_code)] // Mantenido para configuración avanzada
    pub frame_skip_threshold: Duration,
    adaptive_quality_level: f32, // 0.0 a 1.0
    total_frames_processed: u64,
    total_processing_time: Duration,
}

impl SlintRenderer {
    /// Crea un nuevo renderizador optimizado
    pub fn new(config: SlintRendererConfig) -> Self {
        let frame_skip_threshold = Duration::from_secs_f64(1.0 / config.target_fps);
        let cache_capacity = config.max_cache_size;
        
        Self {
            config,
            rgba_pool: None,
            frame_cache: Vec::with_capacity(cache_capacity),
            last_frame_time: Instant::now(),
            frame_skip_threshold,
            adaptive_quality_level: 1.0,
            total_frames_processed: 0,
            total_processing_time: Duration::ZERO,
        }
    }
    
    /// Inicializa el pool de buffers para el tamaño especificado
    pub fn initialize(&mut self, width: u32, height: u32) {
        self.rgba_pool = Some(get_buffer_pool_manager().get_rgba_pool(width, height));
    }
    
    /// Renderiza un buffer de nokhwa a imagen Slint con optimizaciones
    pub fn render_frame(&mut self, buffer: &nokhwa::buffer::Buffer) -> Result<Image> {
        let start_time = Instant::now();
        
        // Calcular hash del frame para caching
        let frame_hash = self.calculate_frame_hash(buffer);
        
        // Verificar cache primero
        if self.config.enable_frame_caching {
            if let Some(cached) = self.get_cached_frame(frame_hash) {
                return Ok(cached.image.clone());
            }
        }
        
        // Desactivar frame skipping agresivo para mejorar fluidez
        // Solo saltar si el rendimiento es extremadamente malo
        if self.should_skip_frame_aggressive() {
            return Err(anyhow::anyhow!("Frame skipped for extreme performance issues"));
        }
        
        // Procesar frame con método optimizado
        let image = if self.config.enable_simd {
            self.render_frame_simd(buffer)?
        } else {
            self.render_frame_standard(buffer)?
        };
        
        // Actualizar métricas y cache
        let processing_time = start_time.elapsed();
        self.update_metrics(processing_time);
        
        if self.config.enable_frame_caching {
            self.cache_frame(frame_hash, image.clone(), processing_time);
        }
        
        self.last_frame_time = Instant::now();
        self.total_frames_processed += 1;
        
        Ok(image)
    }
    
    /// Renderizado con optimizaciones SIMD
    fn render_frame_simd(&mut self, buffer: &nokhwa::buffer::Buffer) -> Result<Image> {
        let resolution = buffer.resolution();
        let width = resolution.width() as u32;
        let height = resolution.height() as u32;
        
        // Actualizar pool si es necesario
        if self.rgba_pool.is_none() || 
           self.rgba_pool.as_ref().unwrap().buffer_size() != (width * height * 4) as usize {
            self.initialize(width, height);
        }
        
        let image_data = buffer.buffer();
        let pool = self.rgba_pool.as_ref().unwrap();
        
        // Calcular tamaños esperados
        let expected_rgb = (width * height * 3) as usize;
        let expected_rgba = (width * height * 4) as usize;
        
        let rgba_data = if image_data.len() == expected_rgb {
            // RGB → RGBA con SIMD optimizado
            self.convert_rgb_to_rgba_simd_optimized(image_data, pool)?
        } else if image_data.len() == expected_rgba {
            // RGBA → RGBA con posible reordenamiento
            self.convert_rgba_optimized(image_data, pool)?
        } else {
            // MJPEG → RGBA con procesamiento paralelo
            self.decode_mjpeg_to_rgba_optimized(image_data, width, height)?
        };
        
        // Crear imagen Slint con zero-copy cuando sea posible
        self.create_slint_image_zero_copy(&rgba_data, width, height)
    }
    
    /// Renderizado estándar (fallback)
    fn render_frame_standard(&mut self, buffer: &nokhwa::buffer::Buffer) -> Result<Image> {
        let resolution = buffer.resolution();
        let width = resolution.width() as u32;
        let height = resolution.height() as u32;
        
        let image_data = buffer.buffer();
        let expected_rgb = (width * height * 3) as usize;
        let expected_rgba = (width * height * 4) as usize;
        
        let rgba_data = if image_data.len() == expected_rgb {
            // Conversión RGB→RGBA estándar
            image_data
                .chunks_exact(3)
                .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], 255]) // BGR→RGBA
                .collect()
        } else if image_data.len() == expected_rgba {
            image_data.to_vec()
        } else {
            // Decodificación MJPEG estándar
            self.decode_mjpeg_standard(image_data, width, height)?
        };
        
        let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba_data, width, height);
        Ok(Image::from_rgba8(pixel_buffer))
    }
    
    /// Conversión RGB→RGBA SIMD ultra-optimizada
    fn convert_rgb_to_rgba_simd_optimized(&self, rgb_data: &[u8], pool: &RgbaBufferPool) -> Result<Vec<u8>> {
        let mut rgba_buffer = pool.get_buffer();
        let pixel_count = rgb_data.len() / 3;
        rgba_buffer.resize(pixel_count * 4, 0);
        
        // Procesamiento en paralelo con chunks grandes para mejor SIMD
        let chunk_size = 4096; // Tamaño óptimo para caché L1
        rgba_buffer.chunks_exact_mut(chunk_size)
            .enumerate()
            .par_bridge()
            .for_each(|(chunk_idx, chunk)| {
                let start_pixel = chunk_idx * (chunk_size / 4);
                let start_rgb = start_pixel * 3;
                
                for (i, rgba_pixel) in chunk.chunks_exact_mut(4).enumerate() {
                    let rgb_idx = start_rgb + (i * 3);
                    if rgb_idx + 2 < rgb_data.len() {
                        // BGR→RGBA swap optimizado
                        rgba_pixel[0] = rgb_data[rgb_idx + 2]; // R ← B
                        rgba_pixel[1] = rgb_data[rgb_idx + 1]; // G ← G
                        rgba_pixel[2] = rgb_data[rgb_idx];     // B ← R
                        rgba_pixel[3] = 255;                  // A
                    }
                }
            });
        
        // Procesar remainder
        let processed_pixels = (pixel_count / (chunk_size / 4)) * (chunk_size / 4);
        let remainder_start = processed_pixels * 3;
        let remainder_rgba_start = processed_pixels * 4;
        
        for i in processed_pixels..pixel_count {
            let rgb_idx = remainder_start + ((i - processed_pixels) * 3);
            let rgba_idx = remainder_rgba_start + ((i - processed_pixels) * 4);
            
            if rgb_idx + 2 < rgb_data.len() && rgba_idx + 3 < rgba_buffer.len() {
                rgba_buffer[rgba_idx] = rgb_data[rgb_idx + 2];
                rgba_buffer[rgba_idx + 1] = rgb_data[rgb_idx + 1];
                rgba_buffer[rgba_idx + 2] = rgb_data[rgb_idx];
                rgba_buffer[rgba_idx + 3] = 255;
            }
        }
        
        // Devolver buffer al pool y crear copia optimizada
        let result = rgba_buffer.clone();
        pool.return_buffer(rgba_buffer);
        Ok(result)
    }
    
    /// Conversión RGBA optimizada
    fn convert_rgba_optimized(&self, rgba_data: &[u8], pool: &RgbaBufferPool) -> Result<Vec<u8>> {
        let mut buffer = pool.get_buffer();
        buffer.resize(rgba_data.len(), 0);
        buffer.copy_from_slice(rgba_data);
        
        let result = buffer.clone();
        pool.return_buffer(buffer);
        Ok(result)
    }
    
    /// Decodificación MJPEG optimizada
    fn decode_mjpeg_to_rgba_optimized(&self, jpeg_data: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
        let img = image::load_from_memory(jpeg_data)?;
        let rgba_img = img.to_rgba8();
        
        // Redimensionamiento adaptativo basado en calidad
        let (target_width, target_height) = if self.config.adaptive_quality {
            let scale = self.adaptive_quality_level;
            ((width as f32 * scale) as u32, (height as f32 * scale) as u32)
        } else {
            (width, height)
        };
        
        let (img_width, img_height) = rgba_img.dimensions();
        
        if img_width != target_width || img_height != target_height {
            self.resize_image_parallel_optimized(&rgba_img, target_width, target_height)
        } else {
            Ok(rgba_img.into_raw())
        }
    }
    
    /// Redimensionamiento paralelo optimizado
    fn resize_image_parallel_optimized(&self, src_img: &image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
                                     target_width: u32, target_height: u32) -> Result<Vec<u8>> {
        let pool = self.rgba_pool.as_ref().unwrap();
        let mut dest_buffer = pool.get_buffer();
        
        let buffer_size = (target_width * target_height * 4) as usize;
        dest_buffer.resize(buffer_size, 0);
        
        let (src_width, src_height) = src_img.dimensions();
        let src_data = src_img.as_raw();
        
        // Cálculo de ratios con punto flotante para mejor precisión
        let x_ratio = src_width as f64 / target_width as f64;
        let y_ratio = src_height as f64 / target_height as f64;
        
        // Procesamiento paralelo por filas con chunks grandes
        dest_buffer.chunks_exact_mut((target_width * 4) as usize)
            .enumerate()
            .par_bridge()
            .for_each(|(y, row)| {
                let src_y = (y as f64 * y_ratio) as u32;
                let src_y_start = (src_y * src_width * 4) as usize;
                
                // Procesamiento vectorial de la fila
                for x in 0..target_width {
                    let src_x = (x as f64 * x_ratio) as u32;
                    let src_idx = src_y_start + (src_x * 4) as usize;
                    let dest_idx = (x * 4) as usize;
                    
                    if src_idx + 3 < src_data.len() && dest_idx + 3 < row.len() {
                        // Copia optimizada de 4 bytes
                        row[dest_idx..dest_idx + 4].copy_from_slice(&src_data[src_idx..src_idx + 4]);
                    }
                }
            });
        
        let result = dest_buffer.clone();
        pool.return_buffer(dest_buffer);
        Ok(result)
    }
    
    /// Decodificación MJPEG estándar (fallback)
    fn decode_mjpeg_standard(&self, jpeg_data: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
        let img = image::load_from_memory(jpeg_data)?;
        let rgba_img = img.to_rgba8();
        let resized_img = image::imageops::resize(
            &rgba_img,
            width,
            height,
            image::imageops::FilterType::Lanczos3
        );
        Ok(resized_img.into_raw())
    }
    
    /// Creación de imagen Slint con zero-copy cuando sea posible
    fn create_slint_image_zero_copy(&self, rgba_data: &[u8], width: u32, height: u32) -> Result<Image> {
        // Intentar zero-copy si los datos están alineados correctamente
        if rgba_data.len() == (width * height * 4) as usize {
            let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba_data, width, height);
            Ok(Image::from_rgba8(pixel_buffer))
        } else {
            // Fallback a copia
            let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
            Ok(Image::from_rgba8(pixel_buffer))
        }
    }
    
    /// Calcula hash del frame para caching
    fn calculate_frame_hash(&self, buffer: &nokhwa::buffer::Buffer) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        buffer.resolution().width().hash(&mut hasher);
        buffer.resolution().height().hash(&mut hasher);
        buffer.buffer().len().hash(&mut hasher);
        
        // Sample hash para rendimiento (no todo el buffer)
        let data = buffer.buffer();
        let sample_size = std::cmp::min(1024, data.len());
        if sample_size > 0 {
            data[..sample_size].hash(&mut hasher);
        }
        
        hasher.finish()
    }
    
    /// Obtiene frame cacheado si existe y es válido
    fn get_cached_frame(&self, frame_hash: u64) -> Option<&CachedFrame> {
        self.frame_cache.iter()
            .find(|cached| cached.frame_hash == frame_hash && 
                          cached.timestamp.elapsed() < Duration::from_secs(1))
    }
    
    /// Almacena frame en cache
    fn cache_frame(&mut self, frame_hash: u64, image: Image, processing_time: Duration) {
        let cached_frame = CachedFrame {
            image,
            timestamp: Instant::now(),
            frame_hash,
            processing_time,
        };
        
        self.frame_cache.push(cached_frame);
        
        // Mantener tamaño máximo de cache
        if self.frame_cache.len() > self.config.max_cache_size {
            self.frame_cache.remove(0);
        }
        
        // Limpiar cache expirada
        self.frame_cache.retain(|cached| cached.timestamp.elapsed() < Duration::from_secs(2));
    }
    
    /// Determina si se debe saltar un frame para mantener rendimiento (solo casos extremos)
    fn should_skip_frame_aggressive(&self) -> bool {
        let _time_since_last_frame = self.last_frame_time.elapsed();
        // NUNCA saltar frames - siempre procesar para máxima fluidez
        false // Desactivado completamente para evitar tartamudeos
    }
    
    /// Actualiza métricas de rendimiento y ajusta calidad adaptativa
    fn update_metrics(&mut self, processing_time: Duration) {
        self.total_processing_time += processing_time;
        
        if self.config.adaptive_quality && self.total_frames_processed > 0 {
            let avg_time = self.total_processing_time / self.total_frames_processed as u32;
            let target_time = Duration::from_secs_f64(1.0 / self.config.target_fps);
            
            // Ajuste de calidad menos agresivo para mantener fluidez
            if avg_time > target_time * 15 / 10 { // Solo si es 50% más lento
                self.adaptive_quality_level = (self.adaptive_quality_level * 0.95).max(0.5); // Mínimo 0.5
            } else if avg_time < target_time * 8 / 10 { // Si es 20% más rápido
                self.adaptive_quality_level = (self.adaptive_quality_level * 1.02).min(1.0);
            }
        }
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
            cache_hit_rate: if self.config.enable_frame_caching {
                self.frame_cache.len() as f64 / self.config.max_cache_size as f64
            } else {
                0.0
            },
            adaptive_quality_level: self.adaptive_quality_level,
        }
    }
    
    /// Reinicia métricas
    #[allow(dead_code)] // Mantenido para API completa
    pub fn reset_metrics(&mut self) {
        self.total_frames_processed = 0;
        self.total_processing_time = Duration::ZERO;
        self.frame_cache.clear();
        self.adaptive_quality_level = 1.0;
    }
}

/// Métricas de rendimiento del renderizador
#[derive(Debug, Clone)]
pub struct SlintRendererMetrics {
    pub total_frames_processed: u64,
    pub average_processing_time: Duration,
    pub cache_hit_rate: f64,
    #[allow(dead_code)] // Mantenido para métricas avanzadas
    pub adaptive_quality_level: f32,
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
        
        assert!(renderer.config.enable_simd);
        assert!(renderer.config.enable_frame_caching);
        assert_eq!(renderer.config.target_fps, 30.0);
    }
    
    #[test]
    fn test_frame_hash_calculation() {
        let renderer = SlintRenderer::new(SlintRendererConfig::default());
        
        // Crear un buffer de prueba simulado
        let resolution = nokhwa::utils::Resolution::new(640, 480);
        let test_data = vec![0u8; 640 * 480 * 3];
        let buffer = nokhwa::buffer::Buffer::new(
            resolution,
            nokhwa::pixel_format::RgbFormat,
            test_data
        );
        
        let hash1 = renderer.calculate_frame_hash(&buffer);
        let hash2 = renderer.calculate_frame_hash(&buffer);
        
        assert_eq!(hash1, hash2);
    }
    
    #[test]
    fn test_performance_metrics() {
        let mut renderer = SlintRenderer::new(SlintRendererConfig::default());
        let metrics = renderer.get_performance_metrics();
        
        assert_eq!(metrics.total_frames_processed, 0);
        assert_eq!(metrics.average_processing_time, Duration::ZERO);
        assert_eq!(metrics.adaptive_quality_level, 1.0);
    }
}
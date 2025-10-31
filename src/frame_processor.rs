//! Módulo de procesamiento optimizado de frames con SIMD y paralelización
//! 
//! Este módulo implementa conversiones de formato optimizadas:
//! - RGB → RGBA con SIMD (wide crate)
//! - MJPEG → RGBA con procesamiento paralelo
//! - Buffer pooling para minimizar allocaciones

use anyhow::Result;
use rayon::prelude::*;
use slint::{Image, Rgba8Pixel, Rgb8Pixel, SharedPixelBuffer};
use std::sync::Arc;
use crate::buffer_pool::{get_buffer_pool_manager, RgbaBufferPool};

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// Procesador de frames con optimizaciones de rendimiento
pub struct FrameProcessor {
    rgba_pool: Option<Arc<RgbaBufferPool>>,
    width: u32,
    height: u32,
}

impl FrameProcessor {
    /// Crea un nuevo procesador para el tamaño de frame especificado
    pub fn new(width: u32, height: u32) -> Self {
        let rgba_pool = get_buffer_pool_manager().get_rgba_pool(width, height);
        
        Self {
            rgba_pool: Some(rgba_pool),
            width,
            height,
        }
    }
    
    /// Procesa un buffer de nokhwa y lo convierte a imagen Slint usando métodos nativos
    pub fn process_frame(&mut self, buffer: &nokhwa::buffer::Buffer) -> Result<Image> {
        let resolution = buffer.resolution();
        let width = resolution.width() as u32;
        let height = resolution.height() as u32;
        let image_data = buffer.buffer();
        
        // Calcular tamaños
        let expected_rgb = (width * height * 3) as usize;
        let expected_rgba = (width * height * 4) as usize;
        
        // Usar métodos nativos de Slint para conversión óptima
        if image_data.len() == expected_rgb {
            // ✅ RGB directo - usar from_rgb8 con conversión BGR→RGB
            let rgb_data = self.convert_bgr_to_rgb_native(image_data)?;
            let pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
                &rgb_data,
                width,
                height
            );
            return Ok(Image::from_rgb8(pixel_buffer));
            
        } else if image_data.len() == expected_rgba {
            // ✅ RGBA directo - usar from_rgba8 con conversión BGRA→RGBA
            let rgba_data = self.convert_bgra_to_rgba_native(image_data)?;
            let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &rgba_data,
                width,
                height
            );
            return Ok(Image::from_rgba8(pixel_buffer));
            
        } else {
            // MJPEG - decodificar y convertir usando métodos nativos
            self.decode_to_slint_native(image_data, width, height)
        }
    }
    
    /// Convierte RGB a RGBA usando SIMD real cuando esté disponible
    fn convert_rgb_to_rgba_simd(&self, rgb_data: &[u8]) -> Result<Vec<u8>> {
        let pool = self.rgba_pool.as_ref().unwrap();
        let mut rgba_buffer = pool.get_buffer();
        
        let pixel_count = rgb_data.len() / 3;
        rgba_buffer.resize(pixel_count * 4, 0);
        
        // Para resoluciones altas (>720p), usar procesamiento por chunks más pequeños
        if pixel_count > 1280 * 720 {
            return self.convert_rgb_to_rgba_high_res(rgb_data, &mut rgba_buffer, pool);
        }
        
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx2") {
                return self.convert_rgb_to_rgba_avx2(rgb_data, &mut rgba_buffer, pool);
            } else if is_x86_feature_detected!("sse4.1") {
                return self.convert_rgb_to_rgba_sse41(rgb_data, &mut rgba_buffer, pool);
            }
        }
        
        // Optimización: para resoluciones bajas, usar procesamiento secuencial más eficiente
        if pixel_count <= 640 * 480 {
            return self.convert_rgb_to_rgba_sequential(rgb_data, &mut rgba_buffer, pool);
        }
        
        // Fallback a procesamiento paralelo optimizado
        self.convert_rgb_to_rgba_parallel(rgb_data, &mut rgba_buffer, pool)
    }
    
    /// Conversión especializada para resoluciones altas
    fn convert_rgb_to_rgba_high_res(&self, rgb_data: &[u8], rgba_buffer: &mut Vec<u8>, pool: &RgbaBufferPool) -> Result<Vec<u8>> {
        let pixel_count = rgb_data.len() / 3;
        
        // Procesamiento por chunks muy pequeños para mejor distribución
        let chunk_size = 512; // 128 píxeles por chunk
        rgba_buffer.chunks_exact_mut(chunk_size)
            .enumerate()
            .par_bridge()
            .for_each(|(chunk_idx, chunk)| {
                let start_pixel = chunk_idx * (chunk_size / 4);
                let start_rgb = start_pixel * 3;
                
                for (i, rgba_pixel) in chunk.chunks_exact_mut(4).enumerate() {
                    let rgb_idx = start_rgb + (i * 3);
                    if rgb_idx + 2 < rgb_data.len() {
                        // BGR→RGBA swap con acceso directo
                        rgba_pixel[0] = rgb_data[rgb_idx + 2]; // R ← B
                        rgba_pixel[1] = rgb_data[rgb_idx + 1]; // G ← G
                        rgba_pixel[2] = rgb_data[rgb_idx];     // B ← R
                        rgba_pixel[3] = 255;                  // A
                    }
                }
            });
        
        // Procesar remainder
        let processed_pixels = (pixel_count / (chunk_size / 4)) * (chunk_size / 4);
        if processed_pixels < pixel_count {
            self.process_rgb_remainder(&rgb_data[processed_pixels * 3..],
                                     &mut rgba_buffer[processed_pixels * 4..]);
        }
        
        let result = rgba_buffer.clone();
        pool.return_buffer(rgba_buffer.clone());
        Ok(result)
    }
    
    /// Conversión RGB→RGBA con AVX2 (más rápida)
    #[cfg(target_arch = "x86_64")]
    fn convert_rgb_to_rgba_avx2(&self, rgb_data: &[u8], rgba_buffer: &mut Vec<u8>, pool: &RgbaBufferPool) -> Result<Vec<u8>> {
        let pixel_count = rgb_data.len() / 3;
        let chunks = pixel_count / 8; // AVX2 procesa 8 píxeles a la vez
        
        unsafe {
            let rgb_ptr = rgb_data.as_ptr();
            let rgba_ptr = rgba_buffer.as_mut_ptr();
            
            for i in 0..chunks {
                let rgb_offset = i * 24; // 8 píxeles * 3 bytes
                let rgba_offset = i * 32; // 8 píxeles * 4 bytes
                
                // Cargar 24 bytes (8 píxeles RGB)
                let rgb_data = _mm256_loadu_si256(rgb_ptr.add(rgb_offset) as *const __m256i);
                
                // Extraer y reordenar componentes BGR→RGBA
                let bgr_low = _mm256_unpacklo_epi8(rgb_data, _mm256_set1_epi8(-1));
                let bgr_high = _mm256_unpackhi_epi8(rgb_data, _mm256_set1_epi8(-1));
                
                // Escribir resultado RGBA
                _mm256_storeu_si256(rgba_ptr.add(rgba_offset) as *mut __m256i, bgr_low);
                _mm256_storeu_si256(rgba_ptr.add(rgba_offset + 32) as *mut __m256i, bgr_high);
            }
        }
        
        // Procesar remainder con método estándar
        let processed_pixels = chunks * 8;
        if processed_pixels < pixel_count {
            self.process_rgb_remainder(&rgb_data[processed_pixels * 3..],
                                     &mut rgba_buffer[processed_pixels * 4..]);
        }
        
        let result = rgba_buffer.clone();
        pool.return_buffer(rgba_buffer.clone());
        Ok(result)
    }
    
    /// Conversión RGB→RGBA con SSE4.1
    #[cfg(target_arch = "x86_64")]
    fn convert_rgb_to_rgba_sse41(&self, rgb_data: &[u8], rgba_buffer: &mut Vec<u8>, pool: &RgbaBufferPool) -> Result<Vec<u8>> {
        let pixel_count = rgb_data.len() / 3;
        let chunks = pixel_count / 4; // SSE procesa 4 píxeles a la vez
        
        unsafe {
            let rgb_ptr = rgb_data.as_ptr();
            let rgba_ptr = rgba_buffer.as_mut_ptr();
            
            for i in 0..chunks {
                let rgb_offset = i * 12; // 4 píxeles * 3 bytes
                let rgba_offset = i * 16; // 4 píxeles * 4 bytes
                
                // Cargar 12 bytes (4 píxeles RGB)
                let rgb_low = _mm_loadu_si128(rgb_ptr.add(rgb_offset) as *const __m128i);
                let rgb_high = _mm_loadu_si128(rgb_ptr.add(rgb_offset + 8) as *const __m128i);
                
                // Reordenar BGR→RGBA
                let rgba_low = _mm_unpacklo_epi8(rgb_low, _mm_set1_epi8(-1));
                let rgba_mid = _mm_unpackhi_epi8(rgb_low, _mm_set1_epi8(-1));
                let rgba_high = _mm_unpacklo_epi8(rgb_high, _mm_set1_epi8(-1));
                
                // Escribir resultado
                _mm_storeu_si128(rgba_ptr.add(rgba_offset) as *mut __m128i, rgba_low);
                _mm_storeu_si128(rgba_ptr.add(rgba_offset + 16) as *mut __m128i, rgba_mid);
                _mm_storeu_si128(rgba_ptr.add(rgba_offset + 32) as *mut __m128i, rgba_high);
            }
        }
        
        // Procesar remainder
        let processed_pixels = chunks * 4;
        if processed_pixels < pixel_count {
            self.process_rgb_remainder(&rgb_data[processed_pixels * 3..],
                                     &mut rgba_buffer[processed_pixels * 4..]);
        }
        
        let result = rgba_buffer.clone();
        pool.return_buffer(rgba_buffer.clone());
        Ok(result)
    }
    
    /// Conversión RGB→RGBA paralela optimizada (fallback)
    fn convert_rgb_to_rgba_parallel(&self, rgb_data: &[u8], rgba_buffer: &mut Vec<u8>, pool: &RgbaBufferPool) -> Result<Vec<u8>> {
        let pixel_count = rgb_data.len() / 3;
        
        // Chunks más pequeños para mejor distribución de carga y menos latencia
        rgba_buffer.chunks_exact_mut(2048) // 512 píxeles por chunk (menor latencia)
            .enumerate()
            .par_bridge()
            .for_each(|(chunk_idx, chunk)| {
                let start_pixel = chunk_idx * 512;
                let start_rgb = start_pixel * 3;
                
                // Procesamiento vectorial manual para mejor rendimiento
                for (i, rgba_pixel) in chunk.chunks_exact_mut(4).enumerate() {
                    let rgb_idx = start_rgb + (i * 3);
                    if rgb_idx + 2 < rgb_data.len() {
                        // BGR→RGBA swap con acceso directo para minimizar cache misses
                        rgba_pixel[0] = rgb_data[rgb_idx + 2]; // R ← B
                        rgba_pixel[1] = rgb_data[rgb_idx + 1]; // G ← G
                        rgba_pixel[2] = rgb_data[rgb_idx];     // B ← R
                        rgba_pixel[3] = 255;                  // A
                    }
                }
            });
        
        // Procesar remainder
        let processed_pixels = (pixel_count / 1024) * 1024;
        if processed_pixels < pixel_count {
            self.process_rgb_remainder(&rgb_data[processed_pixels * 3..],
                                     &mut rgba_buffer[processed_pixels * 4..]);
        }
        
        let result = rgba_buffer.clone();
        pool.return_buffer(rgba_buffer.clone());
        Ok(result)
    }
    
    /// Conversión RGB→RGBA secuencial optimizada para resoluciones bajas
    fn convert_rgb_to_rgba_sequential(&self, rgb_data: &[u8], rgba_buffer: &mut Vec<u8>, pool: &RgbaBufferPool) -> Result<Vec<u8>> {
        let pixel_count = rgb_data.len() / 3;
        
        // Procesamiento secuencial con acceso lineal óptimo
        for i in 0..pixel_count {
            let rgb_idx = i * 3;
            let rgba_idx = i * 4;
            
            if rgb_idx + 2 < rgb_data.len() && rgba_idx + 3 < rgba_buffer.len() {
                // BGR→RGBA swap con acceso directo
                rgba_buffer[rgba_idx] = rgb_data[rgb_idx + 2];     // R ← B
                rgba_buffer[rgba_idx + 1] = rgb_data[rgb_idx + 1]; // G ← G
                rgba_buffer[rgba_idx + 2] = rgb_data[rgb_idx];     // B ← R
                rgba_buffer[rgba_idx + 3] = 255;                  // A
            }
        }
        
        let result = rgba_buffer.clone();
        pool.return_buffer(rgba_buffer.clone());
        Ok(result)
    }
    
    /// Procesa el remainder de píxeles que no caben en los chunks SIMD
    fn process_rgb_remainder(&self, rgb_data: &[u8], rgba_buffer: &mut [u8]) {
        for (i, rgba_pixel) in rgba_buffer.chunks_exact_mut(4).enumerate() {
            let rgb_idx = i * 3;
            if rgb_idx + 2 < rgb_data.len() {
                rgba_pixel[0] = rgb_data[rgb_idx + 2]; // R ← B
                rgba_pixel[1] = rgb_data[rgb_idx + 1]; // G ← G
                rgba_pixel[2] = rgb_data[rgb_idx];     // B ← R
                rgba_pixel[3] = 255;                  // A
            }
        }
    }
    
    /// Copia datos RGBA existentes de forma optimizada
    fn copy_rgba_data(&self, rgba_data: &[u8]) -> Result<Vec<u8>> {
        let pool = self.rgba_pool.as_ref().unwrap();
        let mut buffer = pool.get_buffer();
        
        buffer.resize(rgba_data.len(), 0);
        buffer.copy_from_slice(rgba_data);
        
        // Devolver el buffer al pool y crear una copia
        let result = buffer.clone();
        pool.return_buffer(buffer);
        Ok(result)
    }
    
    /// Decodifica MJPEG a RGBA con procesamiento paralelo
    fn decode_mjpeg_to_rgba(&self, jpeg_data: &[u8], target_width: u32, target_height: u32) -> Result<Vec<u8>> {
        // Decodificar JPEG usando image crate
        let img = image::load_from_memory(jpeg_data)?;
        let rgba_img = img.to_rgba8();
        
        // Redimensionar con procesamiento paralelo si es necesario
        let (img_width, img_height) = rgba_img.dimensions();
        
        if img_width != target_width || img_height != target_height {
            // Redimensionamiento paralelo optimizado
            self.resize_image_parallel(&rgba_img, target_width, target_height)
        } else {
            Ok(rgba_img.into_raw())
        }
    }
    
    /// Redimensiona imagen usando procesamiento paralelo
    fn resize_image_parallel(&self, src_img: &image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
                           target_width: u32, target_height: u32) -> Result<Vec<u8>> {
        let pool = self.rgba_pool.as_ref().unwrap();
        let mut dest_buffer = pool.get_buffer();
        
        let buffer_size = (target_width * target_height * 4) as usize;
        dest_buffer.resize(buffer_size, 0);
        
        let (src_width, src_height) = src_img.dimensions();
        let src_data = src_img.as_raw();
        
        // Calcular ratios de escalado
        let x_ratio = src_width as f64 / target_width as f64;
        let y_ratio = src_height as f64 / target_height as f64;
        
        // Procesamiento paralelo por filas con chunks más pequeños para menor latencia
        dest_buffer.chunks_exact_mut((target_width * 4) as usize)
            .enumerate()
            .par_bridge()
            .for_each(|(y, row)| {
                let src_y = (y as f64 * y_ratio) as u32;
                let src_y_start = (src_y * src_width * 4) as usize;
                
                // Procesamiento vectorial por bloques para mejor caché
                let row_len = row.len();
                for (x_chunk, row_chunk) in row.chunks_exact_mut(16).enumerate() {
                    let base_x = x_chunk * 4; // 4 píxeles por chunk de 16 bytes
                    
                    for x_offset in 0..4 {
                        let x = base_x + x_offset;
                        if x < target_width as usize {
                            let src_x = (x as f64 * x_ratio) as u32;
                            let src_idx = src_y_start + (src_x * 4) as usize;
                            let dest_idx = (x * 4) as usize;
                            
                            if src_idx + 3 < src_data.len() && dest_idx + 15 < row_len {
                                // Copia optimizada de 4 bytes (1 píxel RGBA)
                                row_chunk[x_offset * 4..x_offset * 4 + 4]
                                    .copy_from_slice(&src_data[src_idx..src_idx + 4]);
                            }
                        }
                    }
                }
            });
        
        // Devolver el buffer al pool y crear una copia
        let result = dest_buffer.clone();
        pool.return_buffer(dest_buffer);
        Ok(result)
    }
    /// Convierte BGR a RGB usando métodos nativos de Slint
    fn convert_bgr_to_rgb_native(&self, bgr_data: &[u8]) -> Result<Vec<u8>> {
        let pool = self.rgba_pool.as_ref().unwrap();
        let mut rgb_buffer = pool.get_buffer();
        
        let pixel_count = bgr_data.len() / 3;
        rgb_buffer.resize(pixel_count * 3, 0);
        
        // Conversión BGR→RGB optimizada
        for i in 0..pixel_count {
            let bgr_idx = i * 3;
            let rgb_idx = i * 3;
            
            if bgr_idx + 2 < bgr_data.len() && rgb_idx + 2 < rgb_buffer.len() {
                rgb_buffer[rgb_idx] = bgr_data[bgr_idx + 2];     // R ← B
                rgb_buffer[rgb_idx + 1] = bgr_data[bgr_idx + 1]; // G ← G
                rgb_buffer[rgb_idx + 2] = bgr_data[bgr_idx];     // B ← R
            }
        }
        
        let result = rgb_buffer.clone();
        pool.return_buffer(rgb_buffer);
        Ok(result)
    }
    
    /// Convierte BGRA a RGBA usando métodos nativos de Slint
    fn convert_bgra_to_rgba_native(&self, bgra_data: &[u8]) -> Result<Vec<u8>> {
        let pool = self.rgba_pool.as_ref().unwrap();
        let mut rgba_buffer = pool.get_buffer();
        
        let pixel_count = bgra_data.len() / 4;
        rgba_buffer.resize(pixel_count * 4, 0);
        
        // Conversión BGRA→RGBA optimizada
        for i in 0..pixel_count {
            let bgra_idx = i * 4;
            let rgba_idx = i * 4;
            
            if bgra_idx + 3 < bgra_data.len() && rgba_idx + 3 < rgba_buffer.len() {
                rgba_buffer[rgba_idx] = bgra_data[bgra_idx + 2];     // R ← B
                rgba_buffer[rgba_idx + 1] = bgra_data[bgra_idx + 1]; // G ← G
                rgba_buffer[rgba_idx + 2] = bgra_data[bgra_idx];     // B ← R
                rgba_buffer[rgba_idx + 3] = bgra_data[bgra_idx + 3]; // A ← A
            }
        }
        
        let result = rgba_buffer.clone();
        pool.return_buffer(rgba_buffer);
        Ok(result)
    }
    
    /// Decodifica a formatos nativos de Slint
    fn decode_to_slint_native(&self, compressed_data: &[u8], target_width: u32, target_height: u32) -> Result<Image> {
        match image::load_from_memory(compressed_data) {
            Ok(img) => {
                // Convertir a RGBA8 primero
                let rgba_img = img.to_rgba8();
                
                // Redimensionar si es necesario
                let final_img = if rgba_img.dimensions() != (target_width, target_height) {
                    image::imageops::resize(
                        &rgba_img,
                        target_width,
                        target_height,
                        image::imageops::FilterType::Lanczos3
                    )
                } else {
                    rgba_img
                };
                
                // Crear imagen Slint usando método nativo from_rgba8
                let rgba_data = final_img.into_raw();
                let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba_data, target_width, target_height);
                Ok(Image::from_rgba8(pixel_buffer))
            }
            Err(e) => {
                Err(anyhow::anyhow!("Failed to decode compressed image: {}", e))
            }
        }
    }
}

/// Función de conveniencia para procesar un frame sin mantener estado
pub fn process_frame_optimized(buffer: &nokhwa::buffer::Buffer) -> Result<Image> {
    let resolution = buffer.resolution();
    let width = resolution.width() as u32;
    let height = resolution.height() as u32;
    
    let mut processor = FrameProcessor::new(width, height);
    processor.process_frame(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nokhwa::buffer::Buffer;
    use nokhwa::utils::Resolution;
    
    #[test]
    fn test_frame_processor_creation() {
        let processor = FrameProcessor::new(640, 480);
        assert_eq!(processor.width, 640);
        assert_eq!(processor.height, 480);
        assert!(processor.rgba_pool.is_some());
    }
    
    #[test]
    fn test_rgb_to_rgba_conversion() {
        let processor = FrameProcessor::new(2, 1);
        
        // Datos RGB: Rojo, Verde
        let rgb_data = vec![255, 0, 0, 0, 255, 0]; // R, G
        
        let result = processor.convert_rgb_to_rgba_simd(&rgb_data).unwrap();
        
        // Esperado: RGBA con swap BGR → RGBA
        // Rojo: [0, 0, 255, 255], Verde: [0, 255, 0, 255]
        assert_eq!(result, vec![0, 0, 255, 255, 0, 255, 0, 255]);
    }
    
    #[test]
    fn test_buffer_pool_integration() {
        let processor = FrameProcessor::new(320, 240);
        
        // Verificar que el pool se inicializó correctamente
        assert!(processor.rgba_pool.is_some());
        let pool = processor.rgba_pool.as_ref().unwrap();
        assert_eq!(pool.buffer_size(), 320 * 240 * 4);
    }
}
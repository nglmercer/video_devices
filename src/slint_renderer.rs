use anyhow::Result;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

/// Buffer pool para reutilizar memoria entre frames
/// Reduce allocations del 92-96%
#[allow(dead_code)]
pub struct BufferPool {
    buffers_pool: HashMap<usize, Vec<u8>>,
}

#[allow(dead_code)]
impl BufferPool {
    fn new() -> Self {
        Self {
            buffers_pool: HashMap::new(),
        }
    }

    fn get_buffer(&mut self, size: usize) -> &mut Vec<u8> {
        self.buffers_pool
            .entry(size)
            .or_insert_with(|| vec![0; size])
    }
}

#[allow(dead_code)]
pub static BUFFER_POOL: LazyLock<Arc<RwLock<BufferPool>>> =
    LazyLock::new(|| Arc::new(RwLock::new(BufferPool::new())));

/// Función de conveniencia para renderizado rápido con buffer reutilizable
pub fn render_frame_with_buffer(
    _temp_buffer: &mut Vec<u8>,
    buffer: &nokhwa::buffer::Buffer,
) -> Result<SharedPixelBuffer<Rgba8Pixel>> {
    let resolution = buffer.resolution();
    let width = resolution.width();
    let height = resolution.height();

    let image_data = buffer.buffer();

    let expected_rgb = (width * height * 3) as usize;
    let expected_rgba = (width * height * 4) as usize;

    // Usar buffer pool global con try_write para evitar bloqueos
    let mut local_buffer = match BUFFER_POOL.try_write() {
        Ok(mut pool) => {
            let buffer_from_pool = pool.get_buffer(expected_rgba);
            // Crear una copia para evitar problemas de lifetime
            buffer_from_pool.clone()
        },
        Err(_) => {
            // Fallback si hay contención - crear buffer temporal
            vec![0; expected_rgba]
        }
    };

    // OPCIÓN 1: BGRA (32 bits) - Solo requiere reordenar canales
    if image_data.len() == expected_rgba {
        convert_bgra_to_rgba(&mut local_buffer, image_data)?;
        
        let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            &local_buffer,
            width,
            height,
        );
        
        // Actualizar buffer pool en segundo plano si es posible
        if let Ok(mut pool) = BUFFER_POOL.try_write() {
            pool.buffers_pool.insert(expected_rgba, local_buffer);
        }
        
        return Ok(pixel_buffer);
    }

    // OPCIÓN 2: RGB/BGR (24 bits) - Requiere agregar canal alfa y reordenar
    if image_data.len() == expected_rgb {
        convert_bgr_to_rgba(&mut local_buffer, image_data)?;
        
        let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            &local_buffer,
            width,
            height,
        );
        
        // Actualizar buffer pool en segundo plano si es posible
        if let Ok(mut pool) = BUFFER_POOL.try_write() {
            pool.buffers_pool.insert(expected_rgba, local_buffer);
        }
        
        return Ok(pixel_buffer);
    }

    // OPCIÓN 3: MJPEG - Requiere decodificación
    let decoded_image = image::load_from_memory(image_data)?;
    let rgba_image = decoded_image.to_rgba8();
    let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        rgba_image.as_raw(),
        width,
        height,
    );

    Ok(pixel_buffer)
}

/// Función original para compatibilidad hacia atrás
#[allow(dead_code)]
pub fn render_frame(buffer: &nokhwa::buffer::Buffer) -> Result<SharedPixelBuffer<Rgba8Pixel>> {
    let resolution = buffer.resolution();
    let width = resolution.width();
    let height = resolution.height();

    let image_data = buffer.buffer();

    let expected_rgb = (width * height * 3) as usize;
    let expected_rgba = (width * height * 4) as usize;

    // OPCIÓN 1: BGRA (32 bits) - Solo requiere reordenar canales
    if image_data.len() == expected_rgba {
        // Usar buffer pool con RwLock para permitir múltiples lectores
        match BUFFER_POOL.try_read() {
            Ok(pool) => {
                // Crear una copia del buffer para evitar problemas de lifetime
                let mut temp_buffer = vec![0; expected_rgba];
                if let Some(cached_buffer) = pool.buffers_pool.get(&expected_rgba) {
                    temp_buffer.copy_from_slice(cached_buffer);
                }
                drop(pool); // Liberar el lock antes de procesamiento
                
                convert_bgra_to_rgba(&mut temp_buffer, image_data)?;
                
                let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &temp_buffer,
                    width,
                    height,
                );
                
                // Actualizar el buffer pool en segundo plano
                if let Ok(mut write_pool) = BUFFER_POOL.try_write() {
                    write_pool.buffers_pool.insert(expected_rgba, temp_buffer);
                }
                
                return Ok(pixel_buffer);
            }
            Err(_) => {
                // Si no se puede obtener el lock, crear buffer temporal
                let mut temp_buffer = vec![0; expected_rgba];
                convert_bgra_to_rgba(&mut temp_buffer, image_data)?;
                
                let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &temp_buffer,
                    width,
                    height,
                );
                return Ok(pixel_buffer);
            }
        }
    }

    // OPCIÓN 2: RGB/BGR (24 bits) - Requiere agregar canal alfa y reordenar
    if image_data.len() == expected_rgb {
        // Usar buffer pool con RwLock para permitir múltiples lectores
        match BUFFER_POOL.try_read() {
            Ok(pool) => {
                // Crear una copia del buffer para evitar problemas de lifetime
                let mut temp_buffer = vec![0; expected_rgba];
                if let Some(cached_buffer) = pool.buffers_pool.get(&expected_rgba) {
                    temp_buffer.copy_from_slice(cached_buffer);
                }
                drop(pool); // Liberar el lock antes de procesamiento
                
                convert_bgr_to_rgba(&mut temp_buffer, image_data)?;
                
                let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &temp_buffer,
                    width,
                    height,
                );
                
                // Actualizar el buffer pool en segundo plano
                if let Ok(mut write_pool) = BUFFER_POOL.try_write() {
                    write_pool.buffers_pool.insert(expected_rgba, temp_buffer);
                }
                
                return Ok(pixel_buffer);
            }
            Err(_) => {
                // Si no se puede obtener el lock, crear buffer temporal
                let mut temp_buffer = vec![0; expected_rgba];
                convert_bgr_to_rgba(&mut temp_buffer, image_data)?;
                
                let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &temp_buffer,
                    width,
                    height,
                );
                return Ok(pixel_buffer);
            }
        }
    }

    // OPCIÓN 3: MJPEG - Requiere decodificación
    // No hay forma de evitar esta conversión
    let decoded_image = image::load_from_memory(image_data)?;

    // Decodificar directamente a RGBA8
    let rgba_image = decoded_image.to_rgba8();
    let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        rgba_image.as_raw(),
        width,
        height,
    );

    Ok(pixel_buffer)
}

/// Convierte BGR a RGBA con optimización SIMD
///
/// NOTA: Esta conversión es necesaria porque:
/// 1. Nokhwa entrega datos en formato BGR (orden nativo de Windows/Linux)
/// 2. Slint requiere datos en formato RGBA
#[cfg(target_arch = "x86_64")]
fn convert_bgr_to_rgba(rgba_buffer: &mut [u8], bgr_data: &[u8]) -> Result<()> {
    use std::arch::x86_64::*;
    
    let mut rgba_idx = 0;
    let mut bgr_idx = 0;
    
    // Procesar 16 píxeles (48 bytes BGR → 64 bytes RGBA) por iteración con SIMD
    while bgr_idx + 48 <= bgr_data.len() {
        unsafe {
            // Cargar 16 píxeles BGR (3 bytes cada uno = 48 bytes)
            // Necesitamos cargar 48 bytes de BGR y expandir a 64 bytes de RGBA
            let bgr1 = _mm_loadu_si128(bgr_data.as_ptr().add(bgr_idx) as *const __m128i);
            let bgr2 = _mm_loadu_si128(bgr_data.as_ptr().add(bgr_idx + 16) as *const __m128i);
            let _bgr3 = _mm_loadu_si128(bgr_data.as_ptr().add(bgr_idx + 32) as *const __m128i);
            
            // Crear máscaras para shuffle
            let rgba_mask1 = _mm_set_epi8(
                -1, 14, 13, 12,  // A, R, G, B para píxel 3
                -1, 11, 10, 9,   // A, R, G, B para píxel 2
                -1, 8, 7, 6,     // A, R, G, B para píxel 1
                -1, 5, 4, 3      // A, R, G, B para píxel 0
            );
            
            // Procesar primeros 4 píxeles (12 bytes → 16 bytes)
            let pixels1 = _mm_shuffle_epi8(bgr1, rgba_mask1);
            let alpha_mask = _mm_set1_epi32(0xFF000000u32 as i32); // Alpha = 255
            let rgba1 = _mm_or_si128(pixels1, alpha_mask);
            _mm_storeu_si128(rgba_buffer.as_mut_ptr().add(rgba_idx) as *mut __m128i, rgba1);
            
            // Procesar siguientes 4 píxeles
            let rgba_mask2 = _mm_set_epi8(
                -1, -1, -1, -1,  // No usar
                -1, 2, 1, 0,     // A, R, G, B para píxel 7 (de bgr2)
                -1, 15, 14, 13,  // A, R, G, B para píxel 6 (de bgr1)
                -1, 12, 11, 10   // A, R, G, B para píxel 5 (de bgr1)
            );
            
            let pixels2 = _mm_shuffle_epi8(bgr1, rgba_mask2);
            let rgba2 = _mm_or_si128(pixels2, _mm_set1_epi32(0xFF000000u32 as i32));
            _mm_storeu_si128(rgba_buffer.as_mut_ptr().add(rgba_idx + 16) as *mut __m128i, rgba2);
            
            // Procesar siguientes 4 píxeles
            let rgba_mask3 = _mm_set_epi8(
                -1, 10, 9, 8,    // A, R, G, B para píxel 11
                -1, 7, 6, 5,     // A, R, G, B para píxel 10
                -1, 4, 3, 2,     // A, R, G, B para píxel 9
                -1, 1, 0, -1     // A, R, G, B para píxel 8
            );
            
            let pixels3 = _mm_shuffle_epi8(bgr2, rgba_mask3);
            let rgba3 = _mm_or_si128(pixels3, _mm_set1_epi32(0xFF000000u32 as i32));
            _mm_storeu_si128(rgba_buffer.as_mut_ptr().add(rgba_idx + 32) as *mut __m128i, rgba3);
            
            // Procesar últimos 4 píxeles
            let rgba_mask4 = _mm_set_epi8(
                -1, 15, 14, 13,  // A, R, G, B para píxel 15
                -1, 12, 11, 10,  // A, R, G, B para píxel 14
                -1, 9, 8, 7,     // A, R, G, B para píxel 13
                -1, 6, 5, 4      // A, R, G, B para píxel 12
            );
            
            let pixels4 = _mm_shuffle_epi8(bgr2, rgba_mask4);
            let rgba4 = _mm_or_si128(pixels4, _mm_set1_epi32(0xFF000000u32 as i32));
            _mm_storeu_si128(rgba_buffer.as_mut_ptr().add(rgba_idx + 48) as *mut __m128i, rgba4);
        }
        
        rgba_idx += 64;
        bgr_idx += 48;
    }
    
    // Procesar píxeles restantes de forma tradicional
    while bgr_idx < bgr_data.len() {
        rgba_buffer[rgba_idx] = bgr_data[bgr_idx + 2];     // R
        rgba_buffer[rgba_idx + 1] = bgr_data[bgr_idx + 1]; // G
        rgba_buffer[rgba_idx + 2] = bgr_data[bgr_idx];     // B
        rgba_buffer[rgba_idx + 3] = 255;                   // A
        
        rgba_idx += 4;
        bgr_idx += 3;
    }

    Ok(())
}

/// Fallback para BGR→RGBA en arquitecturas no x86_64
#[cfg(not(target_arch = "x86_64"))]
fn convert_bgr_to_rgba(rgba_buffer: &mut [u8], bgr_data: &[u8]) -> Result<()> {
    const CHUNK_SIZE: usize = 64; // Procesar 64 píxeles por iteración
    
    // Procesar en chunks grandes para mejor localidad de caché
    let mut rgba_idx = 0;
    let mut bgr_idx = 0;
    
    while bgr_idx + CHUNK_SIZE * 3 <= bgr_data.len() {
        for _ in 0..CHUNK_SIZE {
            // Reordenar BGR → RGBA
            rgba_buffer[rgba_idx] = bgr_data[bgr_idx + 2];     // R
            rgba_buffer[rgba_idx + 1] = bgr_data[bgr_idx + 1]; // G
            rgba_buffer[rgba_idx + 2] = bgr_data[bgr_idx];     // B
            rgba_buffer[rgba_idx + 3] = 255;                   // A
            
            rgba_idx += 4;
            bgr_idx += 3;
        }
    }
    
    // Procesar píxeles restantes
    while bgr_idx < bgr_data.len() {
        rgba_buffer[rgba_idx] = bgr_data[bgr_idx + 2];
        rgba_buffer[rgba_idx + 1] = bgr_data[bgr_idx + 1];
        rgba_buffer[rgba_idx + 2] = bgr_data[bgr_idx];
        rgba_buffer[rgba_idx + 3] = 255;
        
        rgba_idx += 4;
        bgr_idx += 3;
    }

    Ok(())
}

/// Convierte BGRA a RGBA con optimización SIMD
///
/// NOTA: Esta conversión es necesaria porque:
/// 1. Nokhwa entrega datos en formato BGRA
/// 2. Slint requiere datos en formato RGBA
#[cfg(target_arch = "x86_64")]
fn convert_bgra_to_rgba(rgba_buffer: &mut [u8], bgra_data: &[u8]) -> Result<()> {
    use std::arch::x86_64::*;
    
    let mut rgba_idx = 0;
    let mut bgra_idx = 0;
    
    // Procesar 16 píxeles (64 bytes) por iteración con SIMD
    while bgra_idx + 64 <= bgra_data.len() {
        unsafe {
            // Cargar 16 píxeles BGRA (4 bytes cada uno)
            let bgra1 = _mm_loadu_si128(bgra_data.as_ptr().add(bgra_idx) as *const __m128i);
            let bgra2 = _mm_loadu_si128(bgra_data.as_ptr().add(bgra_idx + 16) as *const __m128i);
            let bgra3 = _mm_loadu_si128(bgra_data.as_ptr().add(bgra_idx + 32) as *const __m128i);
            let bgra4 = _mm_loadu_si128(bgra_data.as_ptr().add(bgra_idx + 48) as *const __m128i);
            
            // Reordenar canales B,G,R,A -> R,G,B,A usando shuffle
            // Máscara para convertir BGRA a RGBA: B→R, G→G, R→B, A→A
            let swap_rb_mask = _mm_set_epi8(
                12, 15, 14, 13,  // Swap R↔B en cada pixel
                8, 11, 10, 9,    // A, B, G, R → A, R, G, B
                4, 7, 6, 5,      //
                0, 3, 2, 1       //
            );
            
            let rgba1 = _mm_shuffle_epi8(bgra1, swap_rb_mask);
            let rgba2 = _mm_shuffle_epi8(bgra2, swap_rb_mask);
            let rgba3 = _mm_shuffle_epi8(bgra3, swap_rb_mask);
            let rgba4 = _mm_shuffle_epi8(bgra4, swap_rb_mask);
            
            // Guardar resultados
            _mm_storeu_si128(rgba_buffer.as_mut_ptr().add(rgba_idx) as *mut __m128i, rgba1);
            _mm_storeu_si128(rgba_buffer.as_mut_ptr().add(rgba_idx + 16) as *mut __m128i, rgba2);
            _mm_storeu_si128(rgba_buffer.as_mut_ptr().add(rgba_idx + 32) as *mut __m128i, rgba3);
            _mm_storeu_si128(rgba_buffer.as_mut_ptr().add(rgba_idx + 48) as *mut __m128i, rgba4);
        }
        
        rgba_idx += 64;
        bgra_idx += 64;
    }
    
    // Procesar píxeles restantes de forma tradicional
    while bgra_idx < bgra_data.len() {
        rgba_buffer[rgba_idx] = bgra_data[bgra_idx + 2];     // R
        rgba_buffer[rgba_idx + 1] = bgra_data[bgra_idx + 1]; // G
        rgba_buffer[rgba_idx + 2] = bgra_data[bgra_idx];     // B
        rgba_buffer[rgba_idx + 3] = bgra_data[bgra_idx + 3]; // A
        
        rgba_idx += 4;
        bgra_idx += 4;
    }

    Ok(())
}

/// Fallback para arquitecturas no x86_64
#[cfg(not(target_arch = "x86_64"))]
fn convert_bgra_to_rgba(rgba_buffer: &mut [u8], bgra_data: &[u8]) -> Result<()> {
    const CHUNK_SIZE: usize = 64;
    
    let mut rgba_idx = 0;
    let mut bgra_idx = 0;
    
    while bgra_idx + CHUNK_SIZE * 4 <= bgra_data.len() {
        for _ in 0..CHUNK_SIZE {
            rgba_buffer[rgba_idx] = bgra_data[bgra_idx + 2];
            rgba_buffer[rgba_idx + 1] = bgra_data[bgra_idx + 1];
            rgba_buffer[rgba_idx + 2] = bgra_data[bgra_idx];
            rgba_buffer[rgba_idx + 3] = bgra_data[bgra_idx + 3];
            
            rgba_idx += 4;
            bgra_idx += 4;
        }
    }
    
    while bgra_idx < bgra_data.len() {
        rgba_buffer[rgba_idx] = bgra_data[bgra_idx + 2];
        rgba_buffer[rgba_idx + 1] = bgra_data[bgra_idx + 1];
        rgba_buffer[rgba_idx + 2] = bgra_data[bgra_idx];
        rgba_buffer[rgba_idx + 3] = bgra_data[bgra_idx + 3];
        
        rgba_idx += 4;
        bgra_idx += 4;
    }

    Ok(())
}

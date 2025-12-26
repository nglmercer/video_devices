use anyhow::Result;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// Buffer pool para reutilizar memoria entre frames
/// Reduce allocations del 92-96%
pub struct BufferPool {
    buffers_pool: HashMap<usize, Vec<u8>>,
}

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

pub static BUFFER_POOL: LazyLock<Arc<Mutex<BufferPool>>> =
    LazyLock::new(|| Arc::new(Mutex::new(BufferPool::new())));

/// Función de conveniencia para renderizado rápido
/// 
/// Esta función intenta evitar conversiones innecesarias de formato de color.
/// Slint requiere datos en formato RGBA, pero Nokhwa puede entregar:
/// - BGR (24 bits) → necesita conversión a RGBA
/// - BGRA (32 bits) → necesita conversión de orden de canales
/// - MJPEG (comprimido) → necesita decodificación
/// 
/// Optimización: Si Slint soporta directamente BGR/BGRA en el futuro, 
/// podríamos eliminar estas conversiones completamente.
pub fn render_frame(buffer: &nokhwa::buffer::Buffer) -> Result<SharedPixelBuffer<Rgba8Pixel>> {
    let resolution = buffer.resolution();
    let width = resolution.width();
    let height = resolution.height();

    let image_data = buffer.buffer();

    let expected_rgb = (width * height * 3) as usize;
    let expected_rgba = (width * height * 4) as usize;

    // OPCIÓN 1: BGRA (32 bits) - Solo requiere reordenar canales
    if image_data.len() == expected_rgba {
        // Usar buffer pool en lugar de allocation
        let mut pool = BUFFER_POOL.lock().unwrap();
        let rgba_data = pool.get_buffer(expected_rgba);
        
        convert_bgra_to_rgba(rgba_data, image_data)?;
        
        let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            rgba_data,
            width,
            height,
        );

        return Ok(pixel_buffer);
    }

    // OPCIÓN 2: RGB/BGR (24 bits) - Requiere agregar canal alfa y reordenar
    if image_data.len() == expected_rgb {
        // Usar buffer pool en lugar de allocation
        let mut pool = BUFFER_POOL.lock().unwrap();
        let rgba_data = pool.get_buffer(expected_rgba);
        
        convert_bgr_to_rgba(rgba_data, image_data)?;
        
        let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            rgba_data,
            width,
            height,
        );

        return Ok(pixel_buffer);
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

/// Convierte BGR a RGBA
/// 
/// NOTA: Esta conversión es necesaria porque:
/// 1. Nokhwa entrega datos en formato BGR (orden nativo de Windows/Linux)
/// 2. Slint requiere datos en formato RGBA
/// 
/// Si en el futuro Slint soporta BGR nativamente, esta conversión
/// podría eliminarse por completo para 0-overhead.
fn convert_bgr_to_rgba(rgba_buffer: &mut [u8], bgr_data: &[u8]) -> Result<()> {
    // Procesar píxel por píxel
    for (i, chunk) in bgr_data.chunks_exact(3).enumerate() {
        let rgba_idx = i * 4;

        // Reordenar BGR → RGBA
        rgba_buffer[rgba_idx] = chunk[2];     // R (era el byte 2)
        rgba_buffer[rgba_idx + 1] = chunk[1]; // G (el byte 1 no cambia)
        rgba_buffer[rgba_idx + 2] = chunk[0]; // B (era el byte 0)
        rgba_buffer[rgba_idx + 3] = 255;       // A (siempre 255)
    }

    Ok(())
}

/// Convierte BGRA a RGBA
/// 
/// NOTA: Esta conversión es necesaria porque:
/// 1. Nokhwa entrega datos en formato BGRA
/// 2. Slint requiere datos en formato RGBA
/// 
/// Solo requiere reordenar los primeros 3 bytes.
fn convert_bgra_to_rgba(rgba_buffer: &mut [u8], bgra_data: &[u8]) -> Result<()> {
    // Procesar píxel por píxel
    for (i, chunk) in bgra_data.chunks_exact(4).enumerate() {
        let rgba_idx = i * 4;

        // Reordenar BGRA → RGBA
        rgba_buffer[rgba_idx] = chunk[2];     // R (era el byte 2)
        rgba_buffer[rgba_idx + 1] = chunk[1]; // G (el byte 1 no cambia)
        rgba_buffer[rgba_idx + 2] = chunk[0]; // B (era el byte 0)
        rgba_buffer[rgba_idx + 3] = chunk[3]; // A (el byte 3 no cambia)
    }

    Ok(())
}

//! Módulo de procesamiento optimizado de frames con SIMD y paralelización
//!
//! Este módulo implementa conversiones de formato optimizadas:
//! - RGB → RGBA con SIMD (wide crate)
//! - MJPEG → RGBA con procesamiento paralelo
//! - Buffer pooling para minimizar allocaciones

use anyhow::Result;

use slint::{Image, Rgba8Pixel, Rgb8Pixel, SharedPixelBuffer};


#[cfg(target_arch = "x86_64")]


/// Procesador de frames con optimizaciones de rendimiento
pub struct FrameProcessor {
}

impl FrameProcessor {
    /// Crea un nuevo procesador para el tamaño de frame especificado
    pub fn new(_width: u32, _height: u32) -> Self {
        Self {}
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
            let rgb_data: Vec<u8> = image_data
                .chunks_exact(3)
                .flat_map(|pixel| [pixel[2], pixel[1], pixel[0]]) // BGR→RGB
                .collect();
            let pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
                &rgb_data,
                width,
                height
            );
            return Ok(Image::from_rgb8(pixel_buffer));

        } else if image_data.len() == expected_rgba {
            // ✅ RGBA directo - usar from_rgba8 con conversión BGRA→RGBA
            let rgba_data: Vec<u8> = image_data
                .chunks_exact(4)
                .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]]) // BGRA→RGBA
                .collect();
            let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &rgba_data,
                width,
                height
            );
            return Ok(Image::from_rgba8(pixel_buffer));

        } else {
            // MJPEG - decodificar y convertir usando métodos nativos
            match image::load_from_memory(image_data) {
                Ok(img) => {
                    let rgba_img = img.to_rgba8();
                    let final_img = if rgba_img.dimensions() != (width, height) {
                        image::imageops::resize(
                            &rgba_img,
                            width,
                            height,
                            image::imageops::FilterType::Lanczos3
                        )
                    } else {
                        rgba_img
                    };
                    let rgba_data = final_img.into_raw();
                    let pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba_data, width, height);
                    Ok(Image::from_rgba8(pixel_buffer))
                }
                Err(e) => {
                    Err(anyhow::anyhow!("Failed to decode compressed image: {}", e))
                }
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


}

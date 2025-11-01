use crate::buffer_pool::BUFFER_POOL;
use anyhow::Result;
use slint::{Image, Rgb8Pixel, Rgba8Pixel, SharedPixelBuffer};

/// Función de conveniencia para renderizado rápido
pub fn render_frame(buffer: &nokhwa::buffer::Buffer) -> Result<Image> {
    let resolution = buffer.resolution();
    let width = resolution.width() as u32;
    let height = resolution.height() as u32;

    let image_data = buffer.buffer();

    let expected_rgb = (width * height * 3) as usize;
    let expected_rgba = (width * height * 4) as usize;

    match image_data.len() {
        // RGB → usar from_rgb8 nativo con conversión BGR→RGB
        len if len == expected_rgb => {
            let mut pool = BUFFER_POOL.lock().unwrap();
            let rgb_buffer = pool.get_buffer(expected_rgb);

            convert_bgr_to_rgb(rgb_buffer, image_data)?;

            let pixel_buffer =
                SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(rgb_buffer, width, height);

            Ok(Image::from_rgb8(pixel_buffer))
        }

        // RGBA → usar from_rgba8 nativo con conversión BGRA→RGBA
        len if len == expected_rgba => {
            let mut pool = BUFFER_POOL.lock().unwrap();
            let rgba_buffer = pool.get_buffer(expected_rgba);

            convert_bgra_to_rgba(rgba_buffer, image_data)?;

            let pixel_buffer =
                SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba_buffer, width, height);

            Ok(Image::from_rgba8(pixel_buffer))
        }

        _ => {
            // MJPEG → decodificar y usar métodos nativos
            decode_to_slint(image_data, width, height)
        }
    }
}

fn convert_bgr_to_rgb(rgb_buffer: &mut [u8], bgr_data: &[u8]) -> Result<()> {
    let pixel_count = bgr_data.len() / 3;

    // Para resoluciones bajas, usar procesamiento secuencial
    if pixel_count <= 640 * 480 {
        for i in 0..pixel_count {
            let bgr_idx = i * 3;
            let rgb_idx = i * 3;

            rgb_buffer[rgb_idx] = bgr_data[bgr_idx + 2]; // R
            rgb_buffer[rgb_idx + 1] = bgr_data[bgr_idx + 1]; // G
            rgb_buffer[rgb_idx + 2] = bgr_data[bgr_idx]; // B
        }
    } else {
        // Para resoluciones altas, procesar en chunks
        for (i, chunk) in bgr_data.chunks_exact(3).enumerate() {
            let rgb_idx = i * 3;

            rgb_buffer[rgb_idx] = chunk[2]; // R
            rgb_buffer[rgb_idx + 1] = chunk[1]; // G
            rgb_buffer[rgb_idx + 2] = chunk[0]; // B
        }
    }

    Ok(())
}

fn convert_bgra_to_rgba(rgba_buffer: &mut [u8], bgra_data: &[u8]) -> Result<()> {
    for (i, chunk) in bgra_data.chunks_exact(4).enumerate() {
        let rgba_idx = i * 4;

        rgba_buffer[rgba_idx] = chunk[2]; // R
        rgba_buffer[rgba_idx + 1] = chunk[1]; // G
        rgba_buffer[rgba_idx + 2] = chunk[0]; // B
        rgba_buffer[rgba_idx + 3] = chunk[3]; // A
    }

    Ok(())
}

/// Decodificación MJPEG para Slint
fn decode_to_slint(image_data: &[u8], width: u32, height: u32) -> Result<Image> {
    let decoded_image = image::load_from_memory(image_data)?;
    // // Redimensionar si es necesario
    // let final_img = if decoded_image.width() != width || decoded_image.height() != height {
    //     decoded_image.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
    // } else {
    //     decoded_image
    // };

    let rgba_image = decoded_image.to_rgba8();
    let pixel_buffer =
        SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba_image.as_raw(), width, height);

    Ok(Image::from_rgba8(pixel_buffer))
}

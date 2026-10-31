//! Минимальный кодировщик PNG.
//!
//! Нужен, чтобы NovaOS мог сам генерировать иконки для приложений, которые пользователь
//! устанавливает «со стороны» (HTML-файл, ZIP, M3U): у webOS-приложения обязаны
//! быть `icon.png` (80×80) и `largeIcon.png` (130×130).
//!
//! Реализация без внешних зависимостей: zlib через `flate2`, CRC32 — свой.

use crate::error::{NovaError, NovaResult};

/// Таблица CRC-32 (полином 0xEDB88320).
fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut crc = index as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
        *slot = crc;
    }
    table
}

fn crc32(data: &[u8]) -> u32 {
    let table = crc_table();
    let mut crc: u32 = 0xFFFF_FFFF;
    for byte in data {
        crc = table[((crc ^ *byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// Кодирует RGBA-изображение в PNG.
pub fn encode_rgba(width: u32, height: u32, pixels: &[u8]) -> NovaResult<Vec<u8>> {
    if width == 0 || height == 0 {
        return Err(NovaError::Invalid("нулевой размер изображения".into()));
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| NovaError::Invalid("слишком большое изображение".into()))?;
    if pixels.len() != expected {
        return Err(NovaError::Invalid(format!(
            "ожидалось {expected} байт пикселей, получено {}",
            pixels.len()
        )));
    }

    // IDAT: каждая строка начинается с байта фильтра (0 = None).
    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in 0..height as usize {
        raw.push(0u8);
        raw.extend_from_slice(&pixels[row * stride..(row + 1) * stride]);
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    std::io::Write::write_all(&mut encoder, &raw)?;
    let compressed = encoder.finish()?;

    let mut out = Vec::with_capacity(compressed.len() + 128);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // глубина цвета
    ihdr.push(6); // тип: RGBA
    ihdr.push(0); // сжатие
    ihdr.push(0); // фильтр
    ihdr.push(0); // без чересстрочности
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &compressed);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Однотонная RGBA-текстура.
pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&rgba);
    }
    pixels
}

/// Генерирует иконку приложения: скруглённый квадрат с диагональным градиентом.
///
/// Детерминированная (одинаковый `seed` → одинаковая картинка), поэтому иконки
/// самодельных приложений не «прыгают» между запусками.
pub fn generated_icon(size: u32, seed: &str) -> NovaResult<Vec<u8>> {
    let hash = {
        let mut value: u64 = 1469598103934665603;
        for byte in seed.as_bytes() {
            value ^= *byte as u64;
            value = value.wrapping_mul(1099511628211);
        }
        value
    };
    let hue_a = (hash % 360) as f32;
    let hue_b = ((hash >> 12) % 360) as f32;
    let color_a = hsv_to_rgb(hue_a, 0.62, 0.92);
    let color_b = hsv_to_rgb(hue_b, 0.72, 0.55);

    let radius = (size as f32 * 0.22).max(4.0);
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let inside = rounded_rect_alpha(x as f32, y as f32, size as f32, radius);
            let t = (x as f32 + y as f32) / (2.0 * size as f32);
            let r = (color_a.0 as f32 + (color_b.0 as f32 - color_a.0 as f32) * t).round() as u8;
            let g = (color_a.1 as f32 + (color_b.1 as f32 - color_a.1 as f32) * t).round() as u8;
            let b = (color_a.2 as f32 + (color_b.2 as f32 - color_a.2 as f32) * t).round() as u8;
            let alpha = (inside * 255.0).round() as u8;
            pixels.extend_from_slice(&[r, g, b, alpha]);
        }
    }
    encode_rgba(size, size, &pixels)
}

/// Покрытие пикселя скруглённым прямоугольником (0.0…1.0) для сглаживания краёв.
fn rounded_rect_alpha(x: f32, y: f32, size: f32, radius: f32) -> f32 {
    // Знаковое расстояние до скруглённого прямоугольника (классический rounded-box SDF).
    let half = size / 2.0;
    let dx = (x + 0.5 - half).abs() - (half - radius);
    let dy = (y + 0.5 - half).abs() - (half - radius);
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let inside = dx.max(dy).min(0.0);
    let distance = outside + inside;
    (0.5 - distance).clamp(0.0, 1.0)
}

/// HSV → RGB (компоненты 0…255).
pub fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> (u8, u8, u8) {
    let hue = ((hue % 360.0) + 360.0) % 360.0;
    let c = value * saturation;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = value - c;
    let (r, g, b) = match hue as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_valid_png() {
        let png = encode_rgba(
            2,
            2,
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        )
        .unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes([png[16], png[17], png[18], png[19]]), 2);
        // IEND в конце.
        let tail = &png[png.len() - 12..];
        assert_eq!(&tail[4..8], b"IEND");
    }

    #[test]
    fn rejects_bad_pixel_buffer() {
        assert!(encode_rgba(2, 2, &[1, 2, 3]).is_err());
        assert!(encode_rgba(0, 4, &[]).is_err());
    }

    #[test]
    fn generated_icons_are_deterministic() {
        let a = generated_icon(80, "com.novaos.demo").unwrap();
        let b = generated_icon(80, "com.novaos.demo").unwrap();
        let c = generated_icon(80, "com.other.app").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(&a[..4], &[0x89, b'P', b'N', b'G']);
        // webOS требует 80×80 и 130×130.
        assert!(generated_icon(130, "com.novaos.demo").is_ok());
    }

    #[test]
    fn solid_and_hsv() {
        let pixels = solid(2, 2, [10, 20, 30, 255]);
        assert_eq!(pixels.len(), 16);
        assert_eq!(&pixels[..4], &[10, 20, 30, 255]);
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), (0, 255, 0));
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), (0, 0, 255));
    }

    #[test]
    fn rounded_corners_are_transparent() {
        assert!(
            rounded_rect_alpha(0.0, 0.0, 100.0, 22.0) < 0.5,
            "угол должен быть прозрачным"
        );
        assert!(
            rounded_rect_alpha(50.0, 50.0, 100.0, 22.0) > 0.5,
            "центр должен быть непрозрачным"
        );
    }
}

//! Программный рендерер NovaOS.
//!
//! Всё рисуется в обычный RGBA-буфер силами CPU: прямоугольники, скругления,
//! градиенты, тени, текст (растеризация `ab_glyph` с кэшем глифов) и изображения
//! (билинейное масштабирование). Такой подход выбран осознанно:
//!
//! * на LG webOS 3–6 (2016–2021) нет гарантированного GLES-контекста для
//!   сторонних приложений, зато всегда есть SDL2 с видеодрайвером webOS/Wayland;
//! * кадровый буфер можно отдавать в SDL-текстуру целиком или частями;
//! * интерфейс лаунчера статичен: перерисовка выполняется только когда что-то
//!   изменилось, поэтому в покое телевизор не тратит ресурсы вовсе.
//!
//! Для слабых SoC (Cortex-A9 в UM7300PLB) предусмотрено разрешение 1280×720
//! с аппаратным масштабированием до панели — см. `settings::Resolution::Fast`.

use std::collections::HashMap;
use std::path::Path;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};

/// Цвет с альфа-каналом.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    /// Красный.
    pub r: u8,
    /// Зелёный.
    pub g: u8,
    /// Синий.
    pub b: u8,
    /// Альфа (0 — прозрачный).
    pub a: u8,
}

impl Color {
    /// Создаёт цвет из компонентов.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Непрозрачный цвет.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Разбирает `#rgb`, `#rrggbb` или `#rrggbbaa`.
    pub fn hex(text: &str) -> Self {
        let text = text.trim().trim_start_matches('#');
        let bytes: Vec<u8> = (0..text.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&text[i..i + 2.min(text.len() - i)], 16).ok())
            .collect();
        match bytes.len() {
            0 => Self::rgb(0, 0, 0),
            1 | 2 => Self::rgb(bytes[0], bytes[0], bytes[0]),
            3 => Self::rgb(bytes[0], bytes[1], bytes[2]),
            _ => Self::rgba(bytes[0], bytes[1], bytes[2], bytes[3]),
        }
    }

    /// Меняет альфа-канал.
    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Умножает альфу на коэффициент (0.0…1.0).
    pub fn fade(self, k: f32) -> Self {
        self.with_alpha(((self.a as f32) * k.clamp(0.0, 1.0)).round() as u8)
    }

    /// Смешивает два цвета (для градиентов и подсветки).
    pub fn mix(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        Color {
            r: ((self.r as f32) * (1.0 - t) + (other.r as f32) * t).round() as u8,
            g: ((self.g as f32) * (1.0 - t) + (other.g as f32) * t).round() as u8,
            b: ((self.b as f32) * (1.0 - t) + (other.b as f32) * t).round() as u8,
            a: ((self.a as f32) * (1.0 - t) + (other.a as f32) * t).round() as u8,
        }
    }

    /// Осветляет или затемняет цвет.
    pub fn scale(self, k: f32) -> Color {
        Color {
            r: ((self.r as f32) * k).clamp(0.0, 255.0) as u8,
            g: ((self.g as f32) * k).clamp(0.0, 255.0) as u8,
            b: ((self.b as f32) * k).clamp(0.0, 255.0) as u8,
            a: self.a,
        }
    }
}

/// Прямоугольник в логических координатах.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Левый край.
    pub x: f32,
    /// Верхний край.
    pub y: f32,
    /// Ширина.
    pub w: f32,
    /// Высота.
    pub h: f32,
}

impl Rect {
    /// Создаёт прямоугольник.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Квадрат.
    pub const fn square(x: f32, y: f32, size: f32) -> Self {
        Self {
            x,
            y,
            w: size,
            h: size,
        }
    }

    /// Содержит ли точка.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// Внутренний отступ (отрицательный — расширение).
    pub fn inset(self, d: f32) -> Self {
        Self::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }

    /// Смещение.
    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// Центр.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Масштабирование относительно центра (анимация фокуса).
    pub fn scaled_about_center(self, k: f32) -> Self {
        let w = self.w * k;
        let h = self.h * k;
        let (cx, cy) = self.center();
        Self::new(cx - w / 2.0, cy - h / 2.0, w, h)
    }

    /// Линейная интерполяция (анимации перехода между экранами).
    pub fn lerp(self, other: Rect, t: f32) -> Rect {
        let t = t.clamp(0.0, 1.0);
        Rect::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
            self.w + (other.w - self.w) * t,
            self.h + (other.h - self.h) * t,
        )
    }

    /// Пересечение двух прямоугольников (для отсечения).
    pub fn intersect(self, other: Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.w).min(other.x + other.w);
        let bottom = (self.y + self.h).min(other.y + other.h);
        if right <= x || bottom <= y {
            None
        } else {
            Some(Rect::new(x, y, right - x, bottom - y))
        }
    }

    /// Целочисленные границы для попиксельных операций.
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        (
            self.x.floor() as i32,
            self.y.floor() as i32,
            (self.x + self.w).ceil() as i32,
            (self.y + self.h).ceil() as i32,
        )
    }
}

/// Растровое изображение RGBA.
#[derive(Debug, Clone)]
pub struct Image {
    /// Ширина.
    pub width: u32,
    /// Высота.
    pub height: u32,
    /// Пиксели RGBA.
    pub pixels: Vec<u8>,
}

impl Image {
    /// Однотонное изображение.
    pub fn solid(width: u32, height: u32, color: Color) -> Self {
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[color.r, color.g, color.b, color.a]);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Градиент сверху вниз.
    pub fn vertical_gradient(width: u32, height: u32, top: Color, bottom: Color) -> Self {
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            let t = if height > 1 {
                y as f32 / (height - 1) as f32
            } else {
                0.0
            };
            let color = top.mix(bottom, t);
            for x in 0..width {
                let at = ((y * width + x) * 4) as usize;
                pixels[at..at + 4].copy_from_slice(&[color.r, color.g, color.b, color.a]);
            }
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Декодирует PNG или JPEG.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let image = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (width, height) = image.dimensions();
        Some(Self {
            width,
            height,
            pixels: image.into_raw(),
        })
    }

    /// Загружает изображение с диска.
    pub fn load(path: &Path) -> Option<Self> {
        Self::decode(&std::fs::read(path).ok()?)
    }

    /// Билинейное масштабирование.
    pub fn resized(&self, width: u32, height: u32) -> Self {
        if width == 0 || height == 0 {
            return Self::solid(1, 1, Color::rgb(0, 0, 0));
        }
        if width == self.width && height == self.height {
            return self.clone();
        }
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let sx = self.width as f32 / width as f32;
        let sy = self.height as f32 / height as f32;
        for y in 0..height {
            let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (self.height - 1).max(0) as f32);
            let y0 = fy.floor() as u32;
            let y1 = (y0 + 1).min(self.height - 1);
            let wy = fy - y0 as f32;
            for x in 0..width {
                let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (self.width - 1).max(0) as f32);
                let x0 = fx.floor() as u32;
                let x1 = (x0 + 1).min(self.width - 1);
                let wx = fx - x0 as f32;
                let mut out = [0f32; 4];
                for channel in 0..4 {
                    let p00 = self.pixel_channel(x0, y0, channel);
                    let p10 = self.pixel_channel(x1, y0, channel);
                    let p01 = self.pixel_channel(x0, y1, channel);
                    let p11 = self.pixel_channel(x1, y1, channel);
                    let top = p00 + (p10 - p00) * wx;
                    let bottom = p01 + (p11 - p01) * wx;
                    out[channel] = top + (bottom - top) * wy;
                }
                let at = ((y * width + x) * 4) as usize;
                for channel in 0..4 {
                    pixels[at + channel] = out[channel].round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    fn pixel_channel(&self, x: u32, y: u32, channel: usize) -> f32 {
        let at = ((y * self.width + x) * 4) as usize + channel;
        self.pixels.get(at).copied().unwrap_or(0) as f32
    }

    /// Кадрирует изображение под пропорции прямоугольника (как `object-fit: cover`).
    pub fn cover(&self, target: Rect) -> Self {
        let target_w = target.w.max(1.0) as u32;
        let target_h = target.h.max(1.0) as u32;
        let target_ratio = target_w as f32 / target_h as f32;
        let source_ratio = self.width as f32 / self.height.max(1) as f32;
        let (crop_w, crop_h) = if source_ratio > target_ratio {
            let crop_w = (self.height as f32 * target_ratio) as u32;
            (crop_w.min(self.width), self.height)
        } else {
            let crop_h = (self.width as f32 / target_ratio) as u32;
            (self.width, crop_h.min(self.height))
        };
        let x0 = (self.width - crop_w) / 2;
        let y0 = (self.height - crop_h) / 2;
        let mut cropped = Self::solid(crop_w, crop_h, Color::rgb(0, 0, 0));
        for y in 0..crop_h {
            for x in 0..crop_w {
                let src = (((y + y0) * self.width + (x + x0)) * 4) as usize;
                let dst = ((y * crop_w + x) * 4) as usize;
                if src + 4 <= self.pixels.len() {
                    cropped.pixels[dst..dst + 4].copy_from_slice(&self.pixels[src..src + 4]);
                }
            }
        }
        cropped.resized(target_w, target_h)
    }
}

/// Начертание шрифта.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    /// Обычное.
    Regular,
    /// Среднее.
    Medium,
    /// Полужирное.
    SemiBold,
    /// Жирное.
    Bold,
}

impl Weight {
    /// Индекс в наборе шрифтов.
    pub fn index(self) -> usize {
        match self {
            Weight::Regular => 0,
            Weight::Medium => 1,
            Weight::SemiBold => 2,
            Weight::Bold => 3,
        }
    }
}

/// Набор шрифтов NovaOS (Inter с кириллицей, встроен в двоичный файл).
pub struct FontSet {
    fonts: [FontArc; 4],
}

/// Встроенные шрифты: подмножества Inter с латиницей и кириллицей (~50 КБ каждый).
pub const FONT_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Nova-Regular.ttf");
/// Среднее начертание.
pub const FONT_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Nova-Medium.ttf");
/// Полужирное начертание.
pub const FONT_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Nova-SemiBold.ttf");
/// Жирное начертание.
pub const FONT_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Nova-Bold.ttf");

impl FontSet {
    /// Загружает встроенные шрифты.
    pub fn embedded() -> Self {
        let load = |bytes: &'static [u8]| {
            FontArc::try_from_slice(bytes).expect("встроенный шрифт NovaOS повреждён")
        };
        Self {
            fonts: [
                load(FONT_REGULAR),
                load(FONT_MEDIUM),
                load(FONT_SEMIBOLD),
                load(FONT_BOLD),
            ],
        }
    }

    /// Шрифт нужного начертания.
    pub fn font(&self, weight: Weight) -> &FontArc {
        &self.fonts[weight.index()]
    }
}

/// Кэшированный глиф.
struct CachedGlyph {
    width: u32,
    height: u32,
    x_offset: f32,
    y_offset: f32,
    coverage: Vec<u8>,
}

/// Кэш глифов: (начертание, идентификатор глифа, размер).
#[derive(Default)]
pub struct TextCache {
    glyphs: HashMap<(u8, u16, u32), Option<Box<CachedGlyph>>>,
    advances: HashMap<(u8, u16, u32), f32>,
}

impl TextCache {
    /// Создаёт пустой кэш.
    pub fn new() -> Self {
        Self::default()
    }

    /// Количество закэшированных элементов (глифы + ширины).
    pub fn len(&self) -> usize {
        self.glyphs.len() + self.advances.len()
    }

    /// Пуст ли кэш.
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty() && self.advances.is_empty()
    }

    /// Количество растеризованных глифов.
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// Очищает кэш (например, при смене разрешения).
    pub fn clear(&mut self) {
        self.glyphs.clear();
        self.advances.clear();
    }
}

/// Кадровый буфер с primitives рисования.
pub struct Canvas {
    /// Ширина в пикселях.
    pub width: u32,
    /// Высота в пикселях.
    pub height: u32,
    /// Пиксели RGBA.
    pub pixels: Vec<u8>,
}

impl Canvas {
    /// Создаёт буфер указанного размера.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; (width * height * 4) as usize],
        }
    }

    /// Заполняет весь буфер цветом.
    pub fn clear(&mut self, color: Color) {
        if color.a == 255 {
            for pixel in self.pixels.chunks_exact_mut(4) {
                pixel[0] = color.r;
                pixel[1] = color.g;
                pixel[2] = color.b;
                pixel[3] = 255;
            }
        } else {
            let rect = Rect::new(0.0, 0.0, self.width as f32, self.height as f32);
            self.fill_rect(rect, color);
        }
    }

    /// Смешивает пиксель с цветом (учитывая альфу источника).
    #[inline]
    fn blend(&mut self, x: i32, y: i32, color: Color, coverage: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let alpha = (color.a as f32 / 255.0) * coverage.clamp(0.0, 1.0);
        if alpha <= 0.003 {
            return;
        }
        let at = (((y as u32) * self.width + (x as u32)) * 4) as usize;
        let dst = &mut self.pixels[at..at + 4];
        if alpha >= 0.999 {
            dst[0] = color.r;
            dst[1] = color.g;
            dst[2] = color.b;
            dst[3] = 255;
            return;
        }
        let inv = 1.0 - alpha;
        let out_a = alpha + (dst[3] as f32 / 255.0) * inv;
        dst[0] = ((color.r as f32 * alpha + dst[0] as f32 * inv) / out_a.max(0.001)).round() as u8;
        dst[1] = ((color.g as f32 * alpha + dst[1] as f32 * inv) / out_a.max(0.001)).round() as u8;
        dst[2] = ((color.b as f32 * alpha + dst[2] as f32 * inv) / out_a.max(0.001)).round() as u8;
        dst[3] = (out_a * 255.0).round() as u8;
    }

    /// Прямоугольник без скруглений.
    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        if color.a == 0 {
            return;
        }
        let (x0, y0, x1, y1) = rect.bounds();
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width as i32);
        let y1 = y1.min(self.height as i32);
        if color.a == 255 {
            for y in y0..y1 {
                let row = (y as u32 * self.width) as usize * 4;
                let from = row + (x0 as usize) * 4;
                let to = row + (x1 as usize) * 4;
                for pixel in self.pixels[from..to].chunks_exact_mut(4) {
                    pixel[0] = color.r;
                    pixel[1] = color.g;
                    pixel[2] = color.b;
                    pixel[3] = 255;
                }
            }
        } else {
            for y in y0..y1 {
                for x in x0..x1 {
                    self.blend(x, y, color, 1.0);
                }
            }
        }
    }

    /// Вертикальный градиент в прямоугольнике.
    pub fn gradient_rect(&mut self, rect: Rect, top: Color, bottom: Color) {
        let (x0, y0, x1, y1) = rect.bounds();
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width as i32);
        let y1 = y1.min(self.height as i32);
        let height = (y1 - y0).max(1) as f32;
        for y in y0..y1 {
            let t = (y - y0) as f32 / height;
            let color = top.mix(bottom, t);
            for x in x0..x1 {
                self.blend(x, y, color, 1.0);
            }
        }
    }

    /// Скруглённый прямоугольник (сглаживание через знаковое расстояние).
    pub fn fill_rounded(&mut self, rect: Rect, radius: f32, color: Color) {
        if color.a == 0 {
            return;
        }
        let radius = radius.min(rect.w / 2.0).min(rect.h / 2.0).max(0.0);
        if radius < 0.5 {
            self.fill_rect(rect, color);
            return;
        }
        let (x0, y0, x1, y1) = rect.inset(-1.0).bounds();
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = (x1 + 1).min(self.width as i32);
        let y1 = (y1 + 1).min(self.height as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                let coverage = rounded_coverage(x as f32, y as f32, rect, radius);
                if coverage > 0.0 {
                    self.blend(x, y, color, coverage);
                }
            }
        }
    }

    /// Скруглённый прямоугольник с вертикальным градиентом.
    pub fn gradient_rounded(&mut self, rect: Rect, radius: f32, top: Color, bottom: Color) {
        let radius = radius.min(rect.w / 2.0).min(rect.h / 2.0).max(0.0);
        let (x0, y0, x1, y1) = rect.inset(-1.0).bounds();
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = (x1 + 1).min(self.width as i32);
        let y1 = (y1 + 1).min(self.height as i32);
        let height = (y1 - y0).max(1) as f32;
        for y in y0..y1 {
            let t = (y - y0) as f32 / height;
            let color = top.mix(bottom, t);
            for x in x0..x1 {
                let coverage = if radius < 0.5 {
                    1.0
                } else {
                    rounded_coverage(x as f32, y as f32, rect, radius)
                };
                if coverage > 0.0 {
                    self.blend(x, y, color, coverage);
                }
            }
        }
    }

    /// Обводка скруглённого прямоугольника.
    pub fn stroke_rounded(&mut self, rect: Rect, radius: f32, thickness: f32, color: Color) {
        if color.a == 0 || thickness <= 0.0 {
            return;
        }
        let (x0, y0, x1, y1) = rect.inset(-thickness - 1.0).bounds();
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = (x1 + 1).min(self.width as i32);
        let y1 = (y1 + 1).min(self.height as i32);
        let inner = rect.inset(thickness);
        let inner_radius = (radius - thickness).max(0.0);
        for y in y0..y1 {
            for x in x0..x1 {
                let outer = rounded_coverage(x as f32, y as f32, rect, radius);
                if outer <= 0.0 {
                    continue;
                }
                let inner_coverage = if inner.w > 0.0 && inner.h > 0.0 {
                    rounded_coverage(x as f32, y as f32, inner, inner_radius)
                } else {
                    0.0
                };
                let coverage = (outer - inner_coverage).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    self.blend(x, y, color, coverage);
                }
            }
        }
    }

    /// Мягкая тень под скруглённым прямоугольником.
    ///
    /// Реализация «в лоб» (по пикселям), но область ограничена рамкой тени,
    /// поэтому стоимость пропорциональна размеру карточки, а не экрана.
    pub fn shadow(&mut self, rect: Rect, radius: f32, blur: f32, offset_y: f32, color: Color) {
        if blur <= 0.5 || color.a == 0 {
            return;
        }
        let spread = blur * 1.6;
        let area = rect.inset(-spread).offset(0.0, offset_y);
        let (x0, y0, x1, y1) = area.bounds();
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width as i32);
        let y1 = y1.min(self.height as i32);
        let shadow_rect = rect.offset(0.0, offset_y);
        for y in y0..y1 {
            for x in x0..x1 {
                let coverage = rounded_coverage(x as f32, y as f32, shadow_rect, radius);
                let distance = (1.0 - coverage).max(0.0);
                // Приближение гауссова размывания: плавное затухание вне силуэта.
                let falloff = (1.0 - (distance * spread).min(1.0)).powf(2.4);
                let alpha = falloff * (1.0 - coverage);
                if alpha > 0.003 {
                    self.blend(x, y, color, alpha);
                }
            }
        }
    }

    /// Радиальное свечение (подсветка фокуса, «ореол» вокруг карточки).
    pub fn radial_glow(&mut self, center: (f32, f32), radius: f32, color: Color) {
        if color.a == 0 || radius <= 0.0 {
            return;
        }
        let rect = Rect::new(
            center.0 - radius,
            center.1 - radius,
            radius * 2.0,
            radius * 2.0,
        );
        let (x0, y0, x1, y1) = rect.bounds();
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width as i32);
        let y1 = y1.min(self.height as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = x as f32 + 0.5 - center.0;
                let dy = y as f32 + 0.5 - center.1;
                let distance = (dx * dx + dy * dy).sqrt();
                let t = 1.0 - (distance / radius).min(1.0);
                let alpha = t.powi(3);
                if alpha > 0.003 {
                    self.blend(x, y, color, alpha);
                }
            }
        }
    }

    /// Горизонтальная линия.
    pub fn hline(&mut self, x: f32, y: f32, width: f32, thickness: f32, color: Color) {
        self.fill_rect(Rect::new(x, y, width, thickness), color);
    }

    /// Рисует изображение в прямоугольник (с округлением и прозрачностью).
    pub fn blit(&mut self, image: &Image, rect: Rect, alpha: f32, radius: Option<f32>) {
        if alpha <= 0.0 || image.width == 0 || image.height == 0 {
            return;
        }
        let (x0, y0, x1, y1) = rect.inset(-1.0).bounds();
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = (x1 + 1).min(self.width as i32);
        let y1 = (y1 + 1).min(self.height as i32);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let scaled = image.resized(rect.w.max(1.0) as u32, rect.h.max(1.0) as u32);
        let radius = radius.unwrap_or(0.0).min(rect.w / 2.0).min(rect.h / 2.0);
        for y in y0..y1 {
            let sy = (((y as f32 - rect.y) / rect.h) * scaled.height as f32) as i32;
            if sy < 0 || sy >= scaled.height as i32 {
                continue;
            }
            for x in x0..x1 {
                let sx = (((x as f32 - rect.x) / rect.w) * scaled.width as f32) as i32;
                if sx < 0 || sx >= scaled.width as i32 {
                    continue;
                }
                let at = (((sy as u32) * scaled.width + sx as u32) * 4) as usize;
                let color = Color::rgba(
                    scaled.pixels[at],
                    scaled.pixels[at + 1],
                    scaled.pixels[at + 2],
                    scaled.pixels[at + 3],
                );
                if color.a == 0 {
                    continue;
                }
                let coverage = if radius > 0.5 {
                    rounded_coverage(x as f32, y as f32, rect, radius)
                } else {
                    if rect.contains(x as f32 + 0.5, y as f32 + 0.5) {
                        1.0
                    } else {
                        0.0
                    }
                };
                if coverage > 0.0 {
                    self.blend(x, y, color, coverage * alpha);
                }
            }
        }
    }

    /// Измеряет ширину текста без отрисовки.
    pub fn measure(
        fonts: &FontSet,
        cache: &mut TextCache,
        size: f32,
        weight: Weight,
        text: &str,
    ) -> f32 {
        let font = fonts.font(weight);
        let scaled = font.as_scaled(PxScale::from(quantized_size(size)));
        let mut width = 0.0;
        let mut previous: Option<ab_glyph::GlyphId> = None;
        for ch in text.chars() {
            let glyph = scaled.scaled_glyph(ch);
            let id = glyph.id;
            if let Some(prev) = previous {
                width += scaled.kern(prev, id);
            }
            previous = Some(id);
            width += advance_cached(fonts, cache, size, weight, id);
        }
        width
    }

    /// Рисует текст и возвращает ширину строки.
    ///
    /// `baseline_y` — положение базовой линии (как в типографике).
    pub fn text(
        &mut self,
        fonts: &FontSet,
        cache: &mut TextCache,
        x: f32,
        baseline_y: f32,
        size: f32,
        weight: Weight,
        color: Color,
        text: &str,
    ) -> f32 {
        if color.a == 0 || text.is_empty() {
            return 0.0;
        }
        let font = fonts.font(weight).clone();
        let scaled = font.as_scaled(PxScale::from(size));
        let mut caret = x.round();
        let mut previous: Option<ab_glyph::GlyphId> = None;
        for ch in text.chars() {
            let glyph = scaled.scaled_glyph(ch);
            let id = glyph.id;
            if let Some(prev) = previous {
                caret += scaled.kern(prev, id);
            }
            previous = Some(id);
            let advance = advance_cached(fonts, cache, size, weight, id);
            let key = (weight.index() as u8, id.0, size_bits(size));
            if !cache.glyphs.contains_key(&key) {
                let rendered = rasterize_glyph(fonts.font(weight), quantized_size(size), id);
                cache.glyphs.insert(key, rendered);
            }
            if let Some(Some(cached)) = cache.glyphs.get(&key) {
                let origin_x = caret + cached.x_offset;
                let origin_y = baseline_y + cached.y_offset;
                for gy in 0..cached.height {
                    for gx in 0..cached.width {
                        let coverage =
                            cached.coverage[(gy * cached.width + gx) as usize] as f32 / 255.0;
                        if coverage > 0.003 {
                            self.blend(
                                (origin_x.round() as i32) + gx as i32,
                                (origin_y.round() as i32) + gy as i32,
                                color,
                                coverage,
                            );
                        }
                    }
                }
            }
            caret += advance;
        }
        caret - x
    }

    /// Текст с обрезкой по ширине и многоточием.
    pub fn text_ellipsis(
        &mut self,
        fonts: &FontSet,
        cache: &mut TextCache,
        x: f32,
        baseline_y: f32,
        size: f32,
        weight: Weight,
        color: Color,
        text: &str,
        max_width: f32,
    ) -> f32 {
        let shortened = ellipsize(fonts, cache, size, weight, text, max_width);
        self.text(fonts, cache, x, baseline_y, size, weight, color, &shortened)
    }

    /// Сохраняет кадр в PNG.
    pub fn save_png(&self, path: &Path) -> std::io::Result<()> {
        let bytes = nova_core::png::encode_rgba(self.width, self.height, &self.pixels)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(path, bytes)
    }
}

fn size_bits(size: f32) -> u32 {
    (size * 4.0).round() as u32
}

/// Размер, приведённый к шагу кэша (0.25 px) — так один глиф переиспользуется
/// между близкими размерами, а растеризация и измерение остаются согласованными.
fn quantized_size(size: f32) -> f32 {
    size_bits(size) as f32 / 4.0
}

fn advance_cached(
    fonts: &FontSet,
    cache: &mut TextCache,
    size: f32,
    weight: Weight,
    id: ab_glyph::GlyphId,
) -> f32 {
    let key = (weight.index() as u8, id.0, size_bits(size));
    if let Some(value) = cache.advances.get(&key) {
        return *value;
    }
    let font = fonts.font(weight);
    let scaled = font.as_scaled(PxScale::from(quantized_size(size)));
    let advance = scaled.h_advance(id);
    cache.advances.insert(key, advance);
    advance
}

/// Растеризует глиф в карту покрытия (антиалиасинг даёт `ab_glyph`).
fn rasterize_glyph(font: &FontArc, size: f32, id: ab_glyph::GlyphId) -> Option<Box<CachedGlyph>> {
    let glyph = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(0.0, 0.0));
    let outlined = font.outline_glyph(glyph)?;
    let bounds = outlined.px_bounds();
    let width = (bounds.max.x - bounds.min.x).ceil().max(0.0) as u32;
    let height = (bounds.max.y - bounds.min.y).ceil().max(0.0) as u32;
    if width == 0 || height == 0 {
        return None;
    }
    let mut coverage = vec![0u8; (width * height) as usize];
    outlined.draw(|x, y, c| {
        let index = (y * width + x) as usize;
        if index < coverage.len() {
            coverage[index] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    });
    Some(Box::new(CachedGlyph {
        width,
        height,
        x_offset: bounds.min.x,
        y_offset: bounds.min.y,
        coverage,
    }))
}

/// Обрезает строку по ширине, добавляя многоточие.
pub fn ellipsize(
    fonts: &FontSet,
    cache: &mut TextCache,
    size: f32,
    weight: Weight,
    text: &str,
    max_width: f32,
) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if Canvas::measure(fonts, cache, size, weight, text) <= max_width {
        return text.to_string();
    }
    let ellipsis = "…";
    let ellipsis_width = Canvas::measure(fonts, cache, size, weight, ellipsis);
    let budget = (max_width - ellipsis_width).max(0.0);
    let mut out = String::new();
    for ch in text.chars() {
        let candidate = format!("{out}{ch}");
        if Canvas::measure(fonts, cache, size, weight, &candidate) > budget {
            break;
        }
        out = candidate;
    }
    format!("{out}{ellipsis}")
}

/// Покрытие пикселя скруглённым прямоугольником (0.0…1.0).
fn rounded_coverage(px: f32, py: f32, rect: Rect, radius: f32) -> f32 {
    let x = px + 0.5;
    let y = py + 0.5;
    if radius < 0.5 {
        return if rect.contains(x, y) { 1.0 } else { 0.0 };
    }
    let half_w = rect.w / 2.0;
    let half_h = rect.h / 2.0;
    let cx = rect.x + half_w;
    let cy = rect.y + half_h;
    let radius = radius.min(half_w).min(half_h);
    let dx = (x - cx).abs() - (half_w - radius);
    let dy = (y - cy).abs() - (half_h - radius);
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let inside = dx.max(dy).min(0.0);
    let distance = outside + inside - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_parsing_and_mixing() {
        assert_eq!(Color::hex("#0b0d12"), Color::rgb(11, 13, 18));
        assert_eq!(Color::hex("#11223344"), Color::rgba(0x11, 0x22, 0x33, 0x44));
        assert_eq!(Color::hex("ffffff"), Color::rgb(255, 255, 255));
        let mixed = Color::rgb(0, 0, 0).mix(Color::rgb(200, 100, 50), 0.5);
        assert_eq!(mixed, Color::rgb(100, 50, 25));
        assert_eq!(Color::rgb(10, 20, 30).with_alpha(128).a, 128);
        assert_eq!(Color::rgba(10, 20, 30, 200).fade(0.5).a, 100);
        assert_eq!(Color::rgb(100, 100, 100).scale(1.5).r, 150);
    }

    #[test]
    fn rect_geometry() {
        let rect = Rect::new(100.0, 100.0, 200.0, 100.0);
        assert!(rect.contains(150.0, 150.0));
        assert!(!rect.contains(350.0, 150.0));
        assert_eq!(rect.center(), (200.0, 150.0));
        let scaled = rect.scaled_about_center(1.1);
        assert!((scaled.w - 220.0).abs() < 0.01);
        assert_eq!(scaled.center(), rect.center());
        assert_eq!(rect.inset(10.0), Rect::new(110.0, 110.0, 180.0, 80.0));
        let mid = rect.lerp(Rect::new(200.0, 200.0, 200.0, 100.0), 0.5);
        assert!((mid.x - 150.0).abs() < 0.01);
        assert!(rect.intersect(Rect::new(0.0, 0.0, 10.0, 10.0)).is_none());
        assert!(rect
            .intersect(Rect::new(150.0, 150.0, 100.0, 100.0))
            .is_some());
    }

    #[test]
    fn canvas_drawing_does_not_panic() {
        let mut canvas = Canvas::new(64, 48);
        canvas.clear(Color::rgb(8, 10, 14));
        canvas.fill_rect(Rect::new(4.0, 4.0, 20.0, 20.0), Color::rgba(255, 0, 0, 128));
        canvas.fill_rounded(
            Rect::new(30.0, 10.0, 24.0, 24.0),
            8.0,
            Color::rgb(0, 200, 120),
        );
        canvas.stroke_rounded(
            Rect::new(30.0, 10.0, 24.0, 24.0),
            8.0,
            2.0,
            Color::rgb(255, 255, 255),
        );
        canvas.gradient_rect(
            Rect::new(0.0, 0.0, 64.0, 48.0),
            Color::rgba(0, 0, 0, 200),
            Color::rgba(0, 0, 0, 0),
        );
        canvas.gradient_rounded(
            Rect::new(2.0, 2.0, 30.0, 30.0),
            6.0,
            Color::rgb(30, 60, 90),
            Color::rgb(10, 20, 30),
        );
        canvas.shadow(
            Rect::new(10.0, 10.0, 30.0, 20.0),
            6.0,
            12.0,
            6.0,
            Color::rgba(0, 0, 0, 180),
        );
        canvas.radial_glow((32.0, 24.0), 30.0, Color::rgba(76, 141, 255, 120));
        canvas.hline(0.0, 47.0, 64.0, 1.0, Color::rgb(255, 255, 255));
        // Пиксель в центре закрашен.
        assert!(canvas.pixels.iter().any(|b| *b > 0));
        let png = nova_core::png::encode_rgba(canvas.width, canvas.height, &canvas.pixels).unwrap();
        assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn rounded_coverage_sdf() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(rounded_coverage(50.0, 50.0, rect, 20.0) > 0.99);
        assert!(rounded_coverage(200.0, 50.0, rect, 20.0) < 0.01);
        assert!(
            rounded_coverage(0.0, 0.0, rect, 20.0) < 0.05,
            "угол скруглён"
        );
        assert!(
            rounded_coverage(10.0, 10.0, rect, 0.0) > 0.99,
            "без скруглений угол заполнен"
        );
    }

    #[test]
    fn image_operations() {
        let image = Image::solid(8, 8, Color::rgb(10, 20, 30));
        assert_eq!(image.pixels.len(), 8 * 8 * 4);
        let big = image.resized(16, 16);
        assert_eq!(big.width, 16);
        assert_eq!(big.pixels[0], 10);
        let gradient =
            Image::vertical_gradient(4, 4, Color::rgb(0, 0, 0), Color::rgb(255, 255, 255));
        assert_eq!(gradient.pixels[0], 0);
        assert!(gradient.pixels[gradient.pixels.len() - 4] > 200);
        let cover =
            Image::solid(20, 10, Color::rgb(1, 2, 3)).cover(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!((cover.width, cover.height), (10, 10));
    }

    #[test]
    fn image_png_roundtrip() {
        let image = Image::solid(6, 4, Color::rgb(200, 100, 50));
        let png = nova_core::png::encode_rgba(image.width, image.height, &image.pixels).unwrap();
        let decoded = Image::decode(&png).expect("декодирование PNG");
        assert_eq!((decoded.width, decoded.height), (6, 4));
        assert_eq!(decoded.pixels[0], 200);
        assert_eq!(decoded.pixels[1], 100);
    }

    #[test]
    fn embedded_fonts_have_cyrillic() {
        let fonts = FontSet::embedded();
        let mut cache = TextCache::new();
        let text = "Главная Магазин Приложения Настройки";
        let width = Canvas::measure(&fonts, &mut cache, 32.0, Weight::SemiBold, text);
        assert!(
            width > 100.0,
            "кириллица должна измеряться, получено {width}"
        );
        // Ширина растёт с размером шрифта.
        let small = Canvas::measure(&fonts, &mut cache, 16.0, Weight::Regular, text);
        assert!(small < width);
        // Все начертания загружаются.
        for weight in [
            Weight::Regular,
            Weight::Medium,
            Weight::SemiBold,
            Weight::Bold,
        ] {
            assert!(Canvas::measure(&fonts, &mut cache, 20.0, weight, "NovaOS") > 0.0);
        }
        assert!(!cache.is_empty());
    }

    #[test]
    fn text_rendering_and_ellipsis() {
        let fonts = FontSet::embedded();
        let mut cache = TextCache::new();
        let mut canvas = Canvas::new(320, 80);
        canvas.clear(Color::rgb(10, 12, 16));
        let width = canvas.text(
            &fonts,
            &mut cache,
            10.0,
            40.0,
            28.0,
            Weight::Bold,
            Color::rgb(255, 255, 255),
            "Установка приложений",
        );
        assert!(width > 100.0);
        // Пиксели текста появились.
        assert!(canvas.pixels.chunks(4).any(|p| p[0] > 200 && p[1] > 200));

        let shortened = ellipsize(
            &fonts,
            &mut cache,
            28.0,
            Weight::Bold,
            "Очень длинное название приложения",
            120.0,
        );
        assert!(shortened.ends_with('…'));
        assert!(Canvas::measure(&fonts, &mut cache, 28.0, Weight::Bold, &shortened) <= 120.0);
        let same = ellipsize(&fonts, &mut cache, 28.0, Weight::Bold, "Коротко", 400.0);
        assert_eq!(same, "Коротко");
    }

    #[test]
    fn blit_image_with_radius() {
        let fonts = FontSet::embedded();
        let _ = fonts;
        let mut canvas = Canvas::new(64, 64);
        canvas.clear(Color::rgb(0, 0, 0));
        let image = Image::solid(32, 32, Color::rgb(255, 128, 0));
        canvas.blit(&image, Rect::new(8.0, 8.0, 32.0, 32.0), 1.0, Some(8.0));
        let center = ((24 * 64 + 24) * 4) as usize;
        assert_eq!(canvas.pixels[center], 255);
        let corner = ((9 * 64 + 9) * 4) as usize;
        assert!(canvas.pixels[corner] < 200, "угол должен быть скруглён");
    }
}

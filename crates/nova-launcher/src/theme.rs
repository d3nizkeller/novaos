//! Тема оформления NovaOS.
//!
//! Дизайн-система вдохновлена Apple TV / tvOS: тёмный фон, крупные карточки,
//! мягкий «ореол» вокруг фокуса, заметное увеличение активного элемента и
//! спокойная типографика (Inter). Все размеры задаются в логических единицах
//! для экрана 1920×1080 и масштабируются под фактическое разрешение.

use nova_core::settings::Theme as ThemeChoice;

use crate::gfx::Color;

/// Логическая ширина холста, в которой заданы все размеры.
pub const LOGICAL_WIDTH: f32 = 1920.0;
/// Логическая высота холста.
pub const LOGICAL_HEIGHT: f32 = 1080.0;

/// Палитра темы.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Основной фон.
    pub background: Color,
    /// Фон нижнего слоя (градиент).
    pub background_deep: Color,
    /// Поверхность карточки.
    pub surface: Color,
    /// Поверхность при наведении/фокусе.
    pub surface_focus: Color,
    /// Тонкая граница.
    pub border: Color,
    /// Основной текст.
    pub text: Color,
    /// Вторичный текст.
    pub text_dim: Color,
    /// Едва заметный текст.
    pub text_faint: Color,
    /// Акцентный цвет.
    pub accent: Color,
    /// Акцент для фонов и подсветки.
    pub accent_soft: Color,
    /// Текст на акцентной подложке.
    pub on_accent: Color,
    /// Свечение вокруг элемента в фокусе.
    pub focus_glow: Color,
    /// Успех.
    pub success: Color,
    /// Предупреждение.
    pub warning: Color,
    /// Ошибка.
    pub danger: Color,
    /// Кольцо фокуса.
    pub focus_ring: Color,
    /// Затемнение под диалогами.
    pub overlay: Color,
    /// Верх градиента-подложки героя.
    pub scrim_top: Color,
    /// Низ градиента-подложки героя.
    pub scrim_bottom: Color,
}

/// Метрики интерфейса (в логических пикселях 1920×1080).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Отступ от края экрана.
    pub margin: f32,
    /// Расстояние между карточками в ряду.
    pub gutter: f32,
    /// Расстояние между рядами.
    pub row_gap: f32,
    /// Радиус скругления карточек.
    pub card_radius: f32,
    /// Радиус скругления плиток приложений.
    pub tile_radius: f32,
    /// Радиус скругления кнопок и панелей.
    pub control_radius: f32,
    /// Ширина карточки в ряду «Избранное».
    pub favorite_width: f32,
    /// Высота карточки в ряду «Избранное».
    pub favorite_height: f32,
    /// Размер плитки приложения в сетке.
    pub app_tile: f32,
    /// Количество колонок в сетке приложений.
    pub app_columns: usize,
    /// Высота строки списка.
    pub row_height: f32,
    /// Высота верхней панели.
    pub topbar_height: f32,
    /// Увеличение карточки в фокусе.
    pub focus_scale: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            margin: 96.0,
            gutter: 32.0,
            row_gap: 48.0,
            card_radius: 20.0,
            tile_radius: 28.0,
            control_radius: 14.0,
            favorite_width: 300.0,
            favorite_height: 170.0,
            app_tile: 168.0,
            app_columns: 6,
            row_height: 84.0,
            topbar_height: 128.0,
            focus_scale: 1.06,
        }
    }
}

/// Типографика (размеры шрифтов в логических пикселях).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Typography {
    /// Крупный заголовок героя.
    pub hero: f32,
    /// Заголовок экрана.
    pub title: f32,
    /// Подзаголовок раздела.
    pub section: f32,
    /// Основной текст.
    pub body: f32,
    /// Подпись под карточкой.
    pub caption: f32,
    /// Мелкий текст (статусы, метаданные).
    pub small: f32,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            hero: 92.0,
            title: 54.0,
            section: 34.0,
            body: 28.0,
            caption: 24.0,
            small: 20.0,
        }
    }
}

/// Полная тема: палитра + метрики + типографика.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Палитра.
    pub palette: Palette,
    /// Метрики.
    pub metrics: Metrics,
    /// Типографика.
    pub typography: Typography,
}

impl Theme {
    /// Тема по выбору пользователя.
    pub fn for_choice(choice: ThemeChoice) -> Self {
        Self {
            palette: palette_for(choice),
            metrics: Metrics::default(),
            typography: Typography::default(),
        }
    }

    /// Масштаб для фактической ширины холста.
    pub fn scale_for(width: u32) -> f32 {
        width as f32 / LOGICAL_WIDTH
    }
}

fn palette_for(choice: ThemeChoice) -> Palette {
    match choice {
        ThemeChoice::Nova => Palette {
            background: Color::hex("#0b0d12"),
            background_deep: Color::hex("#05070a"),
            surface: Color::hex("#171a21"),
            surface_focus: Color::hex("#232834"),
            border: Color::hex("#262b36"),
            text: Color::hex("#f5f7fa"),
            text_dim: Color::hex("#a4adbb"),
            text_faint: Color::hex("#6c7482"),
            accent: Color::hex("#4c8dff"),
            accent_soft: Color::hex("#1b6ef3"),
            on_accent: Color::hex("#050a14"),
            focus_glow: Color::hex("#4c8dff"),
            success: Color::hex("#33d69f"),
            warning: Color::hex("#ffb648"),
            danger: Color::hex("#ff5f6d"),
            focus_ring: Color::hex("#ffffff"),
            overlay: Color::rgba(4, 6, 10, 190),
            scrim_top: Color::rgba(11, 13, 18, 0),
            scrim_bottom: Color::rgba(6, 8, 12, 235),
        },
        ThemeChoice::Midnight => Palette {
            background: Color::hex("#07070a"),
            background_deep: Color::hex("#000000"),
            surface: Color::hex("#131319"),
            surface_focus: Color::hex("#1f1f28"),
            border: Color::hex("#23232c"),
            text: Color::hex("#f2f0ec"),
            text_dim: Color::hex("#a49f96"),
            text_faint: Color::hex("#6b665e"),
            accent: Color::hex("#ffb35c"),
            accent_soft: Color::hex("#c9772a"),
            on_accent: Color::hex("#1a0e02"),
            focus_glow: Color::hex("#ffb35c"),
            success: Color::hex("#8fd694"),
            warning: Color::hex("#ffd166"),
            danger: Color::hex("#ef6f6c"),
            focus_ring: Color::hex("#fff6e8"),
            overlay: Color::rgba(0, 0, 0, 200),
            scrim_top: Color::rgba(7, 7, 10, 0),
            scrim_bottom: Color::rgba(0, 0, 0, 240),
        },
        ThemeChoice::Graphite => Palette {
            background: Color::hex("#14161a"),
            background_deep: Color::hex("#0c0d10"),
            surface: Color::hex("#1e2126"),
            surface_focus: Color::hex("#2b2f36"),
            border: Color::hex("#31353d"),
            text: Color::hex("#eceff3"),
            text_dim: Color::hex("#a9b0ba"),
            text_faint: Color::hex("#737b86"),
            accent: Color::hex("#8ad0ff"),
            accent_soft: Color::hex("#3f7fa6"),
            on_accent: Color::hex("#04121c"),
            focus_glow: Color::hex("#8ad0ff"),
            success: Color::hex("#67d6a0"),
            warning: Color::hex("#ffc96b"),
            danger: Color::hex("#ff7a7a"),
            focus_ring: Color::hex("#ffffff"),
            overlay: Color::rgba(10, 11, 14, 185),
            scrim_top: Color::rgba(20, 22, 26, 0),
            scrim_bottom: Color::rgba(12, 13, 16, 230),
        },
        ThemeChoice::Cinema => Palette {
            background: Color::hex("#080c18"),
            background_deep: Color::hex("#03050c"),
            surface: Color::hex("#121a2e"),
            surface_focus: Color::hex("#1b2740"),
            border: Color::hex("#22304d"),
            text: Color::hex("#eef2ff"),
            text_dim: Color::hex("#9fadd0"),
            text_faint: Color::hex("#66739a"),
            accent: Color::hex("#7c6cff"),
            accent_soft: Color::hex("#3f36a8"),
            on_accent: Color::hex("#070a18"),
            focus_glow: Color::hex("#7c6cff"),
            success: Color::hex("#4fd1c5"),
            warning: Color::hex("#f6c177"),
            danger: Color::hex("#ff6b81"),
            focus_ring: Color::hex("#ffffff"),
            overlay: Color::rgba(3, 5, 12, 195),
            scrim_top: Color::rgba(8, 12, 24, 0),
            scrim_bottom: Color::rgba(3, 5, 12, 240),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_themes_are_dark_and_readable() {
        for choice in [
            ThemeChoice::Nova,
            ThemeChoice::Midnight,
            ThemeChoice::Graphite,
            ThemeChoice::Cinema,
        ] {
            let theme = Theme::for_choice(choice);
            let bg = theme.palette.background;
            let text = theme.palette.text;
            assert!(
                bg.r < 60 && bg.g < 60 && bg.b < 60,
                "{choice:?}: фон должен быть тёмным"
            );
            assert!(
                text.r > 200 && text.g > 200,
                "{choice:?}: текст должен быть светлым"
            );
            // Контрастность по относительной ярности не ниже 7:1 (WCAG AAA).
            let contrast = relative_luminance(text) + 0.05;
            let background_luma = relative_luminance(bg) + 0.05;
            assert!(
                contrast / background_luma > 7.0,
                "{choice:?}: недостаточный контраст"
            );
            assert!(theme.palette.accent.a == 255);
            // Текст на акцентной кнопке тоже обязан читаться (WCAG AA).
            let on = theme.palette.on_accent;
            let accent = theme.palette.accent;
            let ratio = {
                let a = relative_luminance(on) + 0.05;
                let b = relative_luminance(accent) + 0.05;
                if a > b {
                    a / b
                } else {
                    b / a
                }
            };
            assert!(ratio > 4.5, "{choice:?}: on_accent контраст {ratio:.2}");
        }
    }

    fn relative_luminance(color: Color) -> f64 {
        let channel = |v: u8| {
            let c = v as f64 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
    }

    #[test]
    fn scale_matches_resolution() {
        assert!((Theme::scale_for(1920) - 1.0).abs() < 1e-6);
        assert!((Theme::scale_for(1280) - 2.0 / 3.0).abs() < 1e-6);
        assert!((Theme::scale_for(3840) - 2.0).abs() < 1e-6);
        let metrics = Metrics::default();
        assert_eq!(metrics.app_columns, 6);
        assert!(metrics.favorite_width * 4.0 + metrics.gutter * 3.0 < LOGICAL_WIDTH);
    }
}

//! Генерация файлов телевизионной упаковки в `tv/`.
//!
//! Пример держит `appinfo.json`, иконки и скрипты подмены Home в одном месте с
//! кодом: манифест собирается из констант пакета, иконки рисует собственный
//! рендерер, а `home.sh` и хук автозагрузки берутся из [`nova_core::home`].
//!
//! ```bash
//! cargo run -p nova-launcher --example tvfiles
//! ```

use std::path::Path;

use nova_core::home;
use nova_launcher::gfx::{Canvas, Color, FontSet, Rect, TextCache, Weight};

/// Каталог приложения на телевизоре.
const APP_DIR: &str = "/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher";
/// Имя исполняемого файла внутри пакета.
const EXECUTABLE: &str = "/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher/nova";

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("tv"));
    std::fs::create_dir_all(root.join("init.d")).expect("не удалось создать tv/");

    std::fs::write(root.join("appinfo.json"), appinfo()).expect("appinfo.json");
    std::fs::write(
        root.join("home.sh"),
        home::replace_script(APP_DIR, EXECUTABLE, false),
    )
    .expect("home.sh");
    std::fs::write(
        root.join("home-executable-mount.sh"),
        home::replace_script(APP_DIR, EXECUTABLE, true),
    )
    .expect("home-executable-mount.sh");
    std::fs::write(
        root.join("autostart.sh"),
        home::autostart_script(APP_DIR, EXECUTABLE),
    )
    .expect("autostart.sh");
    std::fs::write(
        root.join("init.d").join("50-novaos-home"),
        home::boot_hook(),
    )
    .expect("init.d/50-novaos-home");
    std::fs::write(root.join("install-directories.txt"), install_directories())
        .expect("install-directories.txt");

    let large = render_icon(320);
    save_png(&large, &root.join("largeIcon.png"));
    let small = large.resized(160, 160);
    save_png(&small, &root.join("icon.png"));

    println!("tv/ обновлён: appinfo.json, иконки, home.sh, init.d/50-novaos-home");
}

fn install_directories() -> String {
    let mut text = String::from("# Куда webOS может установить NovaOS (по приоритету):\n");
    for dir in home::INSTALL_DIRECTORIES {
        text.push_str(dir);
        text.push('\n');
    }
    text
}

fn appinfo() -> String {
    let description = "Быстрый нативный лаунчер NovaOS: плитки, избранное, магазин IPK и установка своих пакетов.";
    format!(
        r##"{{
  "id": "com.novaos.launcher",
  "version": "{version}",
  "vendor": "NovaOS",
  "type": "native",
  "main": "nova",
  "title": "NovaOS",
  "icon": "icon.png",
  "largeIcon": "largeIcon.png",
  "appDescription": "{description}",
  "resolution": "1920x1080",
  "appCategory": ["home"],
  "requiredACG": ["application.launcher", "application.query", "lunabus.query"],
  "handlesRelaunch": false,
  "iconColor": "#3b82f6",
  "bgColor": "#0b0e14"
}}
"##,
        version = nova_core::VERSION,
    )
}

/// Иконка NovaOS: скруглённый квадрат, градиент и созвездие из трёх точек.
fn render_icon(size: u32) -> nova_launcher::gfx::Image {
    use nova_core::settings::Theme as ThemeChoice;
    use nova_launcher::theme::Theme;
    use nova_launcher::widgets::Ctx;

    let mut canvas = Canvas::new(size, size);
    let fonts = FontSet::embedded();
    let mut cache = TextCache::new();
    let palette = Theme::for_choice(ThemeChoice::Nova).palette;
    let mut ctx = Ctx::new(
        &mut canvas,
        &fonts,
        &mut cache,
        Theme::for_choice(ThemeChoice::Nova),
        0.0,
    );

    let full = Rect::new(0.0, 0.0, size as f32, size as f32);
    let radius = size as f32 * 0.24;
    ctx.clear(Color::rgba(0, 0, 0, 0));
    ctx.gradient(full, radius, palette.accent, palette.background_deep);
    ctx.glow(
        (size as f32 * 0.30, size as f32 * 0.28),
        size as f32 * 0.55,
        palette.accent_soft.fade(0.55),
    );

    let letter = Rect::new(0.0, size as f32 * 0.12, size as f32, size as f32 * 0.82);
    ctx.text_center(letter, size as f32 * 0.60, Weight::Bold, palette.text, "N");

    // Три точки «созвездия» — отсылка к названию Nova.
    let dots = [
        (0.72, 0.24, 0.050),
        (0.82, 0.36, 0.034),
        (0.68, 0.44, 0.024),
    ];
    for (x, y, r) in dots {
        let dot = Rect::new(
            size as f32 * x - size as f32 * r,
            size as f32 * y - size as f32 * r,
            size as f32 * r * 2.0,
            size as f32 * r * 2.0,
        );
        ctx.rect(dot, size as f32 * r, palette.text);
    }

    nova_launcher::gfx::Image {
        width: canvas.width,
        height: canvas.height,
        pixels: canvas.pixels,
    }
}

fn save_png(image: &nova_launcher::gfx::Image, path: &Path) {
    let bytes = nova_core::png::encode_rgba(image.width, image.height, &image.pixels)
        .expect("PNG кодируется");
    std::fs::write(path, bytes).expect("запись PNG");
}

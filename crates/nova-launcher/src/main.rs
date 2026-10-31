//! Точка входа NovaOS.
//!
//! На телевизоре бинарник `nova` запускает оконный цикл SDL-webOS и подменяет
//! штатный Home (см. `tv/home.sh`). На desktop те же аргументы дают прогон без
//! окна: `nova --frames 120 --out /tmp/nova` сохранит кадры в PNG.

use std::path::PathBuf;

use nova_core::settings::Settings;
use nova_launcher::backend::Options;
use nova_launcher::state::{AppState, WorkerEnv};

const USAGE: &str = "\
NovaOS — лаунчер для LG webOS

Использование: nova [ПАРАМЕТРЫ]

  --backend ИМЯ        sdl | offscreen | auto (по умолчанию auto)
  --frames N           остановить цикл после N кадров
  --every N            сохранять каждый N-й кадр (с --out)
  --out КАТАЛОГ        каталог для PNG-снимков кадров
  --keys СПИСОК        сценарий нажатий через запятую: Right,Ok,Down
  --key-period N       кадров между нажатиями сценария (по умолчанию 20)
  --resolution WxH     размер кадра (по умолчанию из настроек)
  --fps N              целевая частота кадров (по умолчанию 60)
  --offline            не трогать систему и сеть (desktop-превью)
  --online             разрешить системные вызовы и сеть (телевизор)
  --smoke [КАТАЛОГ]    проверить запуск на устройстве: 45 кадров в PNG
                       (по умолчанию /tmp/nova-smoke) и сообщение об ошибках
  --extract-ipk ФАЙЛ   распаковать data.tar.gz пакета и выйти
  --dest КАТАЛОГ       каталог распаковки (по умолчанию ./extracted)
  --version            версия NovaOS
  --help               эта справка

Клавиши desktop: стрелки — навигация, Enter — OK, Escape — назад,
F1..F4 — цветные кнопки, F5 — обновить, Q — выход.
";

fn main() {
    // Журнал аварий на устройстве: на ТВ stderr никто не видит, а файл
    // переживает перезапуск и помогает понять, почему лаунчер не стартовал.
    std::panic::set_hook(Box::new(|info| {
        crash_log(&format!("panic: {info}"));
    }));

    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut backend = "auto".to_string();
    let mut options = Options::default();
    let mut offline = !cfg!(feature = "webos");
    let mut extract: Option<PathBuf> = None;
    let mut smoke: Option<PathBuf> = None;
    let mut dest = PathBuf::from("extracted");
    let mut resolution_override: Option<(u32, u32)> = None;

    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        let mut value = |name: &str| -> String {
            let value = args.get(index).cloned().unwrap_or_else(|| {
                eprintln!("nova: для {name} нужно значение");
                std::process::exit(2);
            });
            index += 1;
            value
        };
        match arg {
            "--help" | "-h" => {
                print!("{USAGE}");
                return;
            }
            "--version" | "-V" => {
                println!("NovaOS {}", nova_core::VERSION);
                return;
            }
            "--backend" => backend = value("--backend"),
            "--frames" => options.frames = Some(parse_u64(&value("--frames"))),
            "--every" => options.every = parse_u64(&value("--every")).max(1),
            "--out" => options.out_dir = Some(PathBuf::from(value("--out"))),
            "--keys" => {
                options.keys = value("--keys")
                    .split(',')
                    .map(|part| part.trim().to_string())
                    .filter(|part| !part.is_empty())
                    .collect();
            }
            "--key-period" => options.key_period = parse_u64(&value("--key-period")).max(1),
            "--resolution" => {
                resolution_override = Some(parse_size(&value("--resolution")));
            }
            "--fps" => options.fps = parse_u64(&value("--fps")).clamp(1, 240) as u32,
            "--offline" => offline = true,
            "--online" => offline = false,
            "--extract-ipk" => extract = Some(PathBuf::from(value("--extract-ipk"))),
            "--smoke" => {
                // Значение необязательно: смотриим следующий аргумент-путь.
                let next = args.get(index).cloned();
                let path = match next {
                    Some(candidate) if !candidate.starts_with("--") => {
                        index += 1;
                        PathBuf::from(candidate)
                    }
                    _ => PathBuf::from("/tmp/nova-smoke"),
                };
                smoke = Some(path);
            }
            "--dest" => dest = PathBuf::from(value("--dest")),
            other => {
                eprintln!("nova: неизвестный параметр {other}\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    if let Some(ipk) = extract {
        std::process::exit(extract_ipk(&ipk, &dest));
    }

    // Проверка запуска на устройстве: короткая отрисовка в PNG и отчёт.
    if let Some(dir) = smoke {
        std::process::exit(smoke_run(&dir));
    }

    let settings = Settings::load_default();
    let (width, height) = resolution_override.unwrap_or_else(|| settings.resolution.size());
    options.width = width;
    options.height = height;
    if options.out_dir.is_some() && options.frames.is_none() && backend != "sdl" {
        // Без лимита кадров головless-прогон бесконечен — ограничиваем минутой.
        options.frames = Some(u64::from(options.fps) * 60);
    }

    let env = WorkerEnv::from_settings(&settings, offline);
    let mut state = AppState::spawn_env(settings, env.clone());

    let chosen = choose_backend(&backend);
    eprintln!(
        "nova: NovaOS {} · {} · кадр {}x{} · бэкенд {chosen}",
        nova_core::VERSION,
        if env.offline {
            "офлайн"
        } else {
            "боевой режим"
        },
        width,
        height
    );

    match chosen {
        #[cfg(feature = "sdl")]
        "sdl" => match nova_launcher::backend::sdl::run(&mut state, options.clone()) {
            Ok(frames) => eprintln!("nova: кадров отрисовано: {frames}"),
            Err(error) => {
                crash_log(&format!("sdl: {error}"));
                eprintln!("nova: SDL не запустился ({error}), пишу кадры в PNG");
                nova_launcher::backend::offscreen::run(&mut state, fallback(options));
            }
        },
        _ => {
            let frames = nova_launcher::backend::offscreen::run(&mut state, options);
            eprintln!("nova: кадров отрисовано: {frames}");
        }
    }
}

/// Короткая проверка запуска на телевизоре: 45 кадров в PNG без окна.
///
/// Команда для SSH после установки: `nova --smoke` — если в каталоге появились
/// кадры, связка «бинарник + SDL2 + рендерер» на устройстве работает.
fn smoke_run(dir: &std::path::Path) -> i32 {
    let settings = Settings::load_default();
    let env = WorkerEnv::from_settings(&settings, true);
    let mut state = AppState::spawn_env(settings, env);
    let options = Options {
        width: 960,
        height: 540,
        fps: 30,
        frames: Some(45),
        out_dir: Some(dir.to_path_buf()),
        every: 15,
        realtime: false,
        ..Options::default()
    };
    match std::fs::create_dir_all(dir) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("nova: не создать каталог {}: {error}", dir.display());
            crash_log(&format!("smoke: mkdir: {error}"));
            return 1;
        }
    }
    let frames = nova_launcher::backend::offscreen::run(&mut state, options);
    let pngs = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
                .count()
        })
        .unwrap_or(0);
    println!("NovaOS {} · кадров: {frames} · PNG: {pngs} · каталог: {}", nova_core::VERSION, dir.display());
    if pngs == 0 {
        crash_log("smoke: кадры не записаны");
        return 1;
    }
    println!("NOVA_SMOKE_OK");
    0
}

/// Пишет строку в аварийный журнал устройства (куда сможем записать).
fn crash_log(message: &str) {
    use std::io::Write;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("[{stamp}] {message}\n");
    for path in [
        "/tmp/nova-crash.log",
        "/var/lib/novaos/crash.log",
        "./nova-crash.log",
    ] {
        if let Some(dir) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

/// Распаковка data.tar.gz пакета вручную (стратегия ManualExtract).
fn extract_ipk(ipk: &std::path::Path, dest: &std::path::Path) -> i32 {
    let bytes = match std::fs::read(ipk) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("nova: не удалось прочитать {}: {error}", ipk.display());
            return 1;
        }
    };
    let info = match nova_core::ipk::parse(&bytes) {
        Ok(info) => info,
        Err(error) => {
            eprintln!("nova: {} не похож на IPK: {error}", ipk.display());
            return 1;
        }
    };
    match nova_core::ipk::extract_data(&bytes, dest) {
        Ok(count) => {
            println!("{} {}", info.app_id(), info.version());
            println!("файлов распаковано: {count}");
            println!("каталог: {}", dest.display());
            println!(
                "установка вручную: {}",
                nova_core::install::manual_install_command(
                    std::path::Path::new("/usr/bin/nova"),
                    ipk,
                    info.app_id()
                )
            );
            0
        }
        Err(error) => {
            eprintln!("nova: распаковка не удалась: {error}");
            1
        }
    }
}

fn choose_backend(requested: &str) -> &'static str {
    match requested {
        "offscreen" => "offscreen",
        "sdl" => {
            if cfg!(feature = "sdl") {
                "sdl"
            } else {
                eprintln!("nova: собрано без SDL, использую offscreen");
                "offscreen"
            }
        }
        _ => {
            if cfg!(feature = "sdl") && std::env::var_os("DISPLAY").is_some()
                || cfg!(feature = "sdl") && std::env::var_os("WAYLAND_DISPLAY").is_some()
            {
                "sdl"
            } else {
                "offscreen"
            }
        }
    }
}

#[cfg(feature = "sdl")]
fn fallback(mut options: Options) -> Options {
    options.out_dir = Some(PathBuf::from("/tmp/nova-fallback"));
    options.frames = options.frames.or(Some(30));
    options
}

fn parse_u64(text: &str) -> u64 {
    text.trim().parse().unwrap_or_else(|_| {
        eprintln!("nova: «{text}» не число");
        std::process::exit(2);
    })
}

fn parse_size(text: &str) -> (u32, u32) {
    let mut parts = text.split('x');
    let width: u32 = parts
        .next()
        .and_then(|part| part.trim().parse().ok())
        .unwrap_or_else(|| {
            eprintln!("nova: разрешение «{text}» должно быть вида 1920x1080");
            std::process::exit(2);
        });
    let height: u32 = parts
        .next()
        .and_then(|part| part.trim().parse().ok())
        .unwrap_or(1080);
    (width, height)
}

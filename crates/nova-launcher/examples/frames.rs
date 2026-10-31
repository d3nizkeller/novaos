//! Рендер демонстрационных кадров NovaOS в PNG.
//!
//! Пример собирает состояние из демо-каталога и (опционально) настоящего
//! `repo.json`, затем рисует каждый экран в логическом разрешении 1920×1080.
//!
//! ```bash
//! cargo run -p nova-launcher --example frames -- /tmp/frames /путь/к/repo.json
//! ```

use std::path::{Path, PathBuf};

use nova_core::settings::Settings;
use nova_launcher::screens::{handle_key, Key, Renderer, SEC_NAV};
use nova_launcher::state::{AppState, Focus, Screen, TextField, WorkerEnv};
use nova_launcher::widgets::ToastKind;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/nova-frames"));
    let repo = args.next().map(PathBuf::from);
    std::fs::create_dir_all(&out).expect("не удалось создать каталог кадров");

    let work = out.join("work");
    std::fs::create_dir_all(&work).ok();
    std::env::set_var("NOVA_HOME", &work);

    let mut settings = Settings::default();
    settings.work_dir = Some(work.to_string_lossy().into_owned());
    settings.auto_updates = false;
    settings.screensaver_minutes = 0;

    let mut state = AppState::inline(settings, WorkerEnv::sandbox(&work));
    state.catalog = nova_core::catalog::demo_catalog();
    state.settings.favorites = vec![
        "com.novaos.store".into(),
        "youtube.leanback.v4".into(),
        "ru.yandex.kinopoisk".into(),
    ];
    state.catalog.apply_favorites(&state.settings.favorites);
    state.sysinfo = nova_core::sysinfo::SysInfo::mock_um7300();
    state.clock_text = "21:40".into();
    state.date_text = "суббота, 31 октября".into();

    if let Some(path) = repo {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                match nova_core::repo::parse(&value, "webosbrew.org") {
                    Ok(items) => {
                        println!("репозиторий: {} пакетов", items.len());
                        // Настоящий список используем, только если он содержательный.
                        if items.len() >= 6 {
                            state.store = items;
                        }
                    }
                    Err(e) => println!("репозиторий не разобран: {e}"),
                }
            }
        }
    }
    if state.store.is_empty() {
        state.store = demo_store();
        state.store_sync = nova_launcher::state::StoreSync::Ready(state.store.len());
    }

    let mut renderer = Renderer::new(1920, 1080);
    let mut saved: Vec<PathBuf> = Vec::new();

    // Основные экраны.
    for (index, screen) in Screen::ALL.iter().enumerate() {
        state.goto(*screen);
        state.set_focus(Focus::new(SEC_NAV, index));
        saved.push(shoot(
            &mut renderer,
            &mut state,
            &out,
            &format!("{:02}-{}", index + 1, slug(screen.title_ru())),
        ));
    }

    // Карточка приложения.
    state.goto(Screen::Apps);
    state.set_focus(Focus::new(2, 0));
    handle_key(&mut state, Key::Blue);
    saved.push(shoot(&mut renderer, &mut state, &out, "07-app-info"));
    state.close_modal();

    // Карточка пакета магазина.
    state.goto(Screen::Store);
    state.set_focus(Focus::new(2, 0));
    handle_key(&mut state, Key::Ok);
    saved.push(shoot(&mut renderer, &mut state, &out, "08-store-info"));
    handle_key(&mut state, Key::Ok);
    saved.push(shoot(&mut renderer, &mut state, &out, "09-store-confirm"));
    state.close_modal();

    // Экранная клавиатура.
    state.goto(Screen::Sideload);
    state.open_keyboard(TextField::SideloadUrl, "https://".into());
    saved.push(shoot(&mut renderer, &mut state, &out, "10-keyboard"));
    state.close_modal();

    // Приём файлов с телефона (QR-код).
    state.handle(nova_launcher::state::Intent::ToggleUpload);
    state.notify("Приёмник запущен", ToastKind::Ok);
    saved.push(shoot(&mut renderer, &mut state, &out, "11-upload"));
    state.handle(nova_launcher::state::Intent::ToggleUpload);

    // Прогресс установки.
    state.progress.start("Установка пакета");
    state.progress.stage = "Загрузка data.tar.gz".into();
    state.progress.fraction = 0.42;
    state.progress.detail = Some("18,4 МБ из 43,7 МБ".into());
    saved.push(shoot(&mut renderer, &mut state, &out, "12-progress"));
    state.progress.active = false;

    // Альтернативная тема.
    state.handle(nova_launcher::state::Intent::CycleTheme);
    state.goto(Screen::Home);
    state.set_focus(Focus::new(1, 1));
    saved.push(shoot(&mut renderer, &mut state, &out, "13-theme-alt"));

    // Заставка.
    state.screensaver = true;
    saved.push(shoot(&mut renderer, &mut state, &out, "14-screensaver"));

    for path in &saved {
        println!("{}", path.display());
    }
}

fn shoot(renderer: &mut Renderer, state: &mut AppState, out: &Path, name: &str) -> PathBuf {
    renderer.frame(state);
    let path = out.join(format!("{name}.png"));
    renderer.save_png(&path).expect("не удалось сохранить PNG");
    path
}

fn slug(title: &str) -> String {
    let map = [
        ("Главная", "home"),
        ("Приложения", "apps"),
        ("Магазин", "store"),
        ("Установка", "sideload"),
        ("Настройки", "settings"),
        ("О системе", "about"),
    ];
    map.iter()
        .find(|(ru, _)| *ru == title)
        .map(|(_, en)| en.to_string())
        .unwrap_or_else(|| "screen".into())
}

fn demo_store() -> Vec<nova_core::repo::StoreItem> {
    let items = [
        (
            "org.webosbrew.hbchannel",
            "Homebrew Channel",
            "1.5.0",
            true,
            "Магазин домашних приложений для webOS: установка, запуск от root, терминал.",
        ),
        (
            "com.ecklf.luma",
            "Luma",
            "0.4.2",
            false,
            "Быстрый нативный лаунчер для LG webOS с плитками, поиском и избранным.",
        ),
        (
            "org.webosbrew.sesame",
            "Sesame",
            "0.3.1",
            false,
            "Минималистичный плеер для локальной медиатеки по SMB и WebDAV.",
        ),
        (
            "com.novaos.store",
            "Nova Store",
            "1.0.0",
            false,
            "Каталог пакетов NovaOS с проверкой подписи и контрольных сумм.",
        ),
        (
            "ru.yandex.kinopoisk",
            "Кинопоиск",
            "2.7.4",
            false,
            "Фильмы и сериалы: каталог, подборки, продолжение просмотра.",
        ),
        (
            "youtube.leanback.v4",
            "YouTube",
            "2.202",
            false,
            "YouTube для большого экрана с управлением пультом.",
        ),
        (
            "org.webosbrew.terminal",
            "Терминал",
            "1.1.0",
            true,
            "Консоль webOS с SSH-клиентом и базовыми утилитами.",
        ),
        (
            "com.moonlight.tv",
            "Moonlight",
            "0.9.5",
            false,
            "Стриминг игр с ПК по сети через протокол Moonlight.",
        ),
        (
            "org.samba.explorer",
            "Сетевые папки",
            "1.2.3",
            false,
            "Просмотр SMB-шар и USB-накопителей телевизора.",
        ),
        (
            "com.novaos.radio",
            "Радио",
            "0.6.0",
            false,
            "Интернет-радио с русскими станциями и таймером сна.",
        ),
    ];
    items
        .into_iter()
        .enumerate()
        .map(
            |(index, (id, title, version, root, description))| nova_core::repo::StoreItem {
                id: id.into(),
                title: title.into(),
                version: version.into(),
                app_type: if root { "native" } else { "web" }.into(),
                description: description.into(),
                icon_url: String::new(),
                ipk_url: format!("https://repo.webosbrew.org/api/v1/packages/{id}.ipk"),
                sha256: None,
                size: Some(2_400_000 + index as u64 * 917_000),
                source_url: format!("https://github.com/webosbrew/{id}"),
                root_required: root,
                webos_requirement: None,
                screenshots: vec![],
                featured: index < 3,
                repo_title: "webosbrew.org".into(),
            },
        )
        .collect()
}

//! Состояние лаунчера: данные, навигация и фоновые задачи.
//!
//! Всё, что делает интерфейс, проходит через [`AppState`]: экраны читают из
//! него данные и шлют ему [`Intent`], а долгая работа (сканирование каталога,
//! синхронизация магазина, установка пакетов, загрузка иконок) уходит в
//! фоновый поток и возвращается событиями [`Event`]. Главный цикл при этом
//! продолжает рисовать 60 кадров в секунду — телевизор не «зависает» на
//! установке большого IPK.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

use nova_core::app::InstalledApp;
use nova_core::catalog::{Catalog, ScanOptions, ScanPaths};
use nova_core::clock;
use nova_core::home;
use nova_core::install::{InstallReport, InstallSource, InstallStage, Installer, Strategy};
use nova_core::luna::LunaClient;
use nova_core::net::format_size;
use nova_core::repo::StoreItem;
use nova_core::settings::{
    HomeMode, InstallBackend, RepoRef, Resolution, Settings, Theme as ThemeChoice, Wallpaper,
};
use nova_core::sysinfo::{Compatibility, SysInfo};
use nova_core::upload::{UploadHandle, UploadServer, DEFAULT_PORT};

use crate::gfx::Image;
use crate::theme::Theme;
use crate::widgets::{Button, Keyboard, ToastKind};

/// Размер иконки в кэше (логические пиксели с запасом на увеличение в фокусе).
pub const ICON_SIZE: u32 = 256;

// ============================ Экраны ============================

/// Экраны лаунчера.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    /// Главная: избранное, часы, быстрые действия.
    Home,
    /// Все установленные приложения.
    Apps,
    /// Магазин (репозитории).
    Store,
    /// Установка своих пакетов (URL, файл, QR с телефона).
    Sideload,
    /// Настройки.
    Settings,
    /// О системе.
    About,
}

impl Screen {
    /// Все экраны в порядке следования в меню.
    pub const ALL: [Screen; 6] = [
        Screen::Home,
        Screen::Apps,
        Screen::Store,
        Screen::Sideload,
        Screen::Settings,
        Screen::About,
    ];

    /// Строковый идентификатор (сохраняется в настройках).
    pub fn id(&self) -> &'static str {
        match self {
            Screen::Home => "home",
            Screen::Apps => "apps",
            Screen::Store => "store",
            Screen::Sideload => "sideload",
            Screen::Settings => "settings",
            Screen::About => "about",
        }
    }

    /// Экран по идентификатору.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// Русский заголовок.
    pub fn title_ru(&self) -> &'static str {
        match self {
            Screen::Home => "Главная",
            Screen::Apps => "Приложения",
            Screen::Store => "Магазин",
            Screen::Sideload => "Установка",
            Screen::Settings => "Настройки",
            Screen::About => "О системе",
        }
    }

    /// Короткое описание под заголовком.
    pub fn subtitle_ru(&self) -> &'static str {
        match self {
            Screen::Home => "Избранное и быстрый запуск",
            Screen::Apps => "Всё, что установлено на телевизоре",
            Screen::Store => "Приложения из репозиториев NovaOS",
            Screen::Sideload => "Свои IPK по ссылке, с телефона или с USB",
            Screen::Settings => "Оформление, репозитории, система",
            Screen::About => "Устройство, версия и диагностика",
        }
    }

    /// Пиктограмма из встроенного шрифта.
    pub fn glyph(&self) -> &'static str {
        match self {
            Screen::Home => "■",
            Screen::Apps => "◆",
            Screen::Store => "★",
            Screen::Sideload => "↓",
            Screen::Settings => "●",
            Screen::About => "◇",
        }
    }

    /// Следующий экран (циклически).
    pub fn next(&self) -> Self {
        let index = Self::ALL.iter().position(|s| s == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// Предыдущий экран (циклически).
    pub fn prev(&self) -> Self {
        let index = Self::ALL.iter().position(|s| s == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

// ============================ Фокус и навигация ============================

/// Положение фокуса: раздел и элемент внутри него.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Focus {
    /// Индекс раздела (ряда, секции, группы настроек).
    pub section: usize,
    /// Индекс элемента в разделе.
    pub item: usize,
}

impl Focus {
    /// Создаёт фокус.
    pub fn new(section: usize, item: usize) -> Self {
        Self { section, item }
    }
}

/// Двигает фокус по разделам.
///
/// `counts` — число элементов в каждом разделе. Вправо/влево перемещается
/// внутри раздела (с циклическим переходом), вверх/вниз — между непустыми
/// разделами с сохранением позиции по горизонтали.
pub fn move_focus(focus: &mut Focus, dx: i32, dy: i32, counts: &[usize]) {
    if counts.is_empty() {
        focus.section = 0;
        focus.item = 0;
        return;
    }
    let non_empty: Vec<usize> = counts
        .iter()
        .enumerate()
        .filter(|(_, c)| **c > 0)
        .map(|(i, _)| i)
        .collect();
    if non_empty.is_empty() {
        focus.section = 0;
        focus.item = 0;
        return;
    }
    if focus.section >= counts.len() || counts[focus.section] == 0 {
        focus.section = *non_empty.first().unwrap_or(&0);
        focus.item = 0;
    }

    if dx != 0 {
        let count = counts[focus.section].max(1) as i32;
        focus.item = ((focus.item as i32 + dx).rem_euclid(count)) as usize;
    }
    if dy != 0 {
        let position = non_empty
            .iter()
            .position(|i| *i == focus.section)
            .unwrap_or(0) as i32;
        let next = (position + dy).rem_euclid(non_empty.len() as i32) as usize;
        let target = non_empty[next];
        focus.section = target;
        focus.item = focus.item.min(counts[target].saturating_sub(1));
    }
    focus.item = focus.item.min(counts[focus.section].saturating_sub(1));
}

// ============================ Команды и события ============================

/// Задача для фонового потока.
#[derive(Debug, Clone)]
pub enum Command {
    /// Обновить конфигурацию работника (после изменения настроек).
    Configure(WorkerEnv),
    /// Определить устройство, root, Homebrew Channel, статус Home.
    DetectSystem,
    /// Пересканировать каталог приложений.
    Rescan,
    /// Загрузить иконки приложений: (id, путь).
    LoadIcons(Vec<(String, PathBuf)>),
    /// Синхронизировать репозитории.
    SyncStore,
    /// Загрузить иконки магазина: (id, url).
    LoadStoreIcons(Vec<(String, String)>),
    /// Установить пакет.
    Install {
        /// Источник.
        source: InstallSource,
        /// Ожидаемая SHA-256 (если известна).
        sha256: Option<String>,
    },
    /// Удалить приложение.
    Uninstall(String),
    /// Запустить приложение.
    Launch(String),
    /// Применить режим интеграции с кнопкой Home.
    ApplyHome(HomeMode),
    /// Загрузить обои.
    LoadWallpaper(WallpaperSource),
    /// Остановить работника.
    Shutdown,
}

/// Источник обоев для фоновой загрузки.
#[derive(Debug, Clone, PartialEq)]
pub enum WallpaperSource {
    /// Файл на устройстве.
    File(PathBuf),
    /// URL (нужна фича `net`).
    Url(String),
}

/// Событие из фонового потока.
#[derive(Debug)]
pub enum Event {
    /// Определены сведения об устройстве.
    SysInfo(SysInfo),
    /// Статус интеграции с Home.
    HomeStatus(home::HomeStatus),
    /// Каталог приложений готов.
    Catalog(Catalog),
    /// Загружены иконки приложений.
    Icons(Vec<(String, Image)>),
    /// Магазин синхронизирован.
    Store(Vec<StoreItem>),
    /// Ошибка синхронизации магазина.
    StoreError(String),
    /// Загружены иконки магазина.
    StoreIcons(Vec<(String, Image)>),
    /// Стадия установки.
    InstallStage(InstallStage),
    /// Установка завершена.
    InstallDone(InstallReport),
    /// Установка не удалась.
    InstallFailed(String),
    /// Приложение удалено.
    Uninstalled {
        /// Идентификатор.
        id: String,
        /// Способ удаления.
        strategy: Strategy,
    },
    /// Не удалось удалить.
    UninstallFailed(String),
    /// Приложение запущено.
    Launched(String),
    /// Не удалось запустить.
    LaunchFailed(String),
    /// Интеграция с Home применена.
    HomeApplied(String),
    /// Интеграция с Home не удалась.
    HomeFailed(String),
    /// Обои загружены.
    Wallpaper(Image),
    /// Короткое сообщение пользователю.
    Notice(String, ToastKind),
}

/// Конфигурация фонового работника.
#[derive(Debug, Clone)]
pub struct WorkerEnv {
    /// Не выполнять системных вызовов (desktop-превью, тесты).
    pub offline: bool,
    /// Рабочий каталог для загрузки пакетов.
    pub work_dir: PathBuf,
    /// Способ установки.
    pub backend: InstallBackend,
    /// Пути сканирования каталога.
    pub scan_paths: ScanPaths,
    /// Параметры сканирования.
    pub scan_options: ScanOptions,
    /// Избранные приложения (порядок).
    pub favorites: Vec<String>,
    /// Репозитории магазина.
    pub repos: Vec<RepoRef>,
}

impl WorkerEnv {
    /// Конфигурация из настроек.
    pub fn from_settings(settings: &Settings, offline: bool) -> Self {
        Self {
            offline,
            work_dir: settings.work_dir(),
            backend: settings.install_backend,
            scan_paths: ScanPaths::default(),
            scan_options: ScanOptions {
                include_system: settings.show_system_apps,
                ..ScanOptions::default()
            },
            favorites: settings.favorites.clone(),
            repos: settings.enabled_repos().into_iter().cloned().collect(),
        }
    }

    /// Конфигурация для тестов: всё в временном каталоге, без системы.
    pub fn sandbox(root: impl AsRef<Path>) -> Self {
        Self {
            offline: true,
            work_dir: root.as_ref().join("work"),
            backend: InstallBackend::Auto,
            scan_paths: ScanPaths::for_root(root),
            scan_options: ScanOptions {
                include_system: true,
                measure_size: false,
                hide_broken: false,
            },
            favorites: Vec::new(),
            repos: Vec::new(),
        }
    }
}

fn luna(env: &WorkerEnv) -> LunaClient {
    if env.offline {
        LunaClient::offline()
    } else {
        LunaClient::new()
    }
}

fn installer(env: &WorkerEnv) -> Installer {
    if env.offline {
        Installer::dry_run(env.work_dir.clone())
    } else {
        Installer::new(luna(env), env.work_dir.clone(), env.backend)
    }
}

/// Выполняет одну команду и отправляет события.
pub fn handle_command(command: Command, tx: &Sender<Event>, env: &mut WorkerEnv) {
    let send = |event: Event| {
        let _ = tx.send(event);
    };
    match command {
        Command::Configure(new_env) => {
            let need_rescan = new_env.scan_options.include_system
                != env.scan_options.include_system
                || new_env.favorites != env.favorites;
            *env = new_env;
            if need_rescan {
                handle_command(Command::Rescan, tx, env);
            }
        }
        Command::DetectSystem => {
            let client = luna(env);
            let info = if env.offline {
                SysInfo::mock_um7300()
            } else {
                SysInfo::detect(&client)
            };
            send(Event::SysInfo(info));
            send(Event::HomeStatus(home::detect_status()));
        }
        Command::Rescan => {
            let mut catalog = Catalog::scan(&env.scan_paths, env.scan_options);
            let client = luna(env);
            if let Ok(apps) = client.list_apps() {
                catalog.merge_luna(apps);
            }
            if let Some(running) = client.foreground_app() {
                catalog.mark_running(&[running]);
            }
            catalog.apply_favorites(&env.favorites);
            send(Event::Catalog(catalog));
        }
        Command::LoadIcons(items) => {
            let mut icons = Vec::with_capacity(items.len());
            for (id, path) in items {
                if let Some(image) = Image::load(&path) {
                    icons.push((id, image.resized(ICON_SIZE, ICON_SIZE)));
                }
            }
            if !icons.is_empty() {
                send(Event::Icons(icons));
            }
        }
        Command::SyncStore => {
            if env.repos.is_empty() {
                send(Event::StoreError(
                    "не включено ни одного репозитория".into(),
                ));
                return;
            }
            let mut items: Vec<StoreItem> = Vec::new();
            let mut errors: Vec<String> = Vec::new();
            for repo in &env.repos {
                match nova_core::repo::fetch(&repo.url, &repo.title) {
                    Ok(mut fetched) => {
                        items.append(&mut fetched);
                    }
                    Err(e) => errors.push(format!("{}: {}", repo.title, e)),
                }
            }
            items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
            items.dedup_by(|a, b| a.id == b.id && a.version == b.version);
            if items.is_empty() {
                send(Event::StoreError(
                    errors
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "репозитории пусты".into()),
                ));
            } else {
                send(Event::Store(items));
                if !errors.is_empty() {
                    send(Event::Notice(
                        format!("Часть репозиториев недоступна: {}", errors.len()),
                        ToastKind::Warn,
                    ));
                }
            }
        }
        Command::LoadStoreIcons(items) => {
            let mut icons = Vec::new();
            for (id, url) in items {
                if url.is_empty() {
                    continue;
                }
                if let Ok(downloaded) = nova_core::net::download(&url, |_| {}) {
                    if let Some(image) = Image::decode(&downloaded.bytes) {
                        icons.push((id, image.resized(ICON_SIZE, ICON_SIZE)));
                    }
                }
            }
            if !icons.is_empty() {
                send(Event::StoreIcons(icons));
            }
        }
        Command::Install { source, sha256 } => {
            let installer = installer(env);
            let progress_tx = tx.clone();
            match installer.install(&source, sha256.as_deref(), move |stage| {
                let _ = progress_tx.send(Event::InstallStage(stage));
            }) {
                Ok(report) => send(Event::InstallDone(report)),
                Err(e) => send(Event::InstallFailed(format!("{}: {e}", e.short()))),
            }
            handle_command(Command::Rescan, tx, env);
        }
        Command::Uninstall(id) => {
            let installer = installer(env);
            match installer.uninstall(&id) {
                Ok(strategy) => send(Event::Uninstalled { id, strategy }),
                Err(e) => send(Event::UninstallFailed(format!("{}: {e}", e.short()))),
            }
            handle_command(Command::Rescan, tx, env);
        }
        Command::Launch(id) => {
            let client = luna(env);
            match client.launch(&id, &serde_json::json!({})) {
                Ok(()) => send(Event::Launched(id)),
                Err(e) => send(Event::LaunchFailed(format!("{}: {e}", e.short()))),
            }
        }
        Command::ApplyHome(mode) => {
            let client = luna(env);
            let info = if env.offline {
                SysInfo::mock_um7300()
            } else {
                SysInfo::detect(&client)
            };
            let app_dir = nova_core::settings::app_dir().unwrap_or_else(|| {
                PathBuf::from("/media/cryptofs/apps/usr/palm/applications/").join(nova_core::APP_ID)
            });
            let executable = app_dir.join("nova");
            let plan = home::plan(mode, &info, &app_dir, &executable);
            match home::apply(&client, &plan) {
                Ok(message) => {
                    send(Event::HomeApplied(message));
                    send(Event::HomeStatus(home::detect_status()));
                }
                Err(e) => send(Event::HomeFailed(format!("{}: {e}", e.short()))),
            }
        }
        Command::LoadWallpaper(source) => {
            let image = match source {
                WallpaperSource::File(path) => Image::load(&path),
                WallpaperSource::Url(url) => nova_core::net::download(&url, |_| {})
                    .ok()
                    .and_then(|d| Image::decode(&d.bytes)),
            };
            match image {
                Some(img) => send(Event::Wallpaper(img)),
                None => send(Event::Notice(
                    "не удалось загрузить обои".to_string(),
                    ToastKind::Warn,
                )),
            }
        }
        Command::Shutdown => {}
    }
}

// ============================ Модальные окна ============================

/// Поле ввода, для которого открывается экранная клавиатура.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextField {
    /// Ссылка на пакет (экран «Установка»).
    SideloadUrl,
    /// Путь к файлу на устройстве/USB.
    SideloadPath,
    /// Адрес репозитория (настройки).
    RepoUrl,
    /// Адрес обоев.
    WallpaperUrl,
}

impl TextField {
    /// Заголовок окна ввода.
    pub fn title_ru(&self) -> &'static str {
        match self {
            TextField::SideloadUrl => "Ссылка на пакет",
            TextField::SideloadPath => "Путь к файлу",
            TextField::RepoUrl => "Адрес репозитория",
            TextField::WallpaperUrl => "Адрес изображения",
        }
    }

    /// Подсказка в пустом поле.
    pub fn hint_ru(&self) -> &'static str {
        match self {
            TextField::SideloadUrl => "https://…/app.ipk",
            TextField::SideloadPath => "/media/usb/app.ipk",
            TextField::RepoUrl => "https://…/repo.json",
            TextField::WallpaperUrl => "https://…/wallpaper.png",
        }
    }

    /// Латинская раскладка по умолчанию (все поля — адреса и пути).
    pub fn prefers_latin(&self) -> bool {
        true
    }
}

/// Действие, которое выполняется после подтверждения.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    /// Ничего не делать.
    None,
    /// Установить пакет.
    Install {
        /// Источник.
        source: InstallSource,
        /// Ожидаемая SHA-256.
        sha256: Option<String>,
    },
    /// Удалить приложение.
    Uninstall(String),
    /// Сменить режим кнопки Home.
    SetHome(HomeMode),
    /// Перезагрузить телевизор.
    Reboot,
    /// Выключить телевизор.
    PowerOff,
    /// Выйти из NovaOS (вернуться в штатный интерфейс).
    Quit,
    /// Добавить репозиторий.
    AddRepo(String),
    /// Удалить репозиторий.
    RemoveRepo(usize),
    /// Включить/выключить репозиторий.
    ToggleRepo(usize),
    /// Сбросить избранное.
    ResetFavorites,
}

/// Диалог подтверждения.
#[derive(Debug, Clone)]
pub struct ConfirmModal {
    /// Заголовок.
    pub title: String,
    /// Текст.
    pub body: String,
    /// Кнопки.
    pub buttons: Vec<Button>,
    /// Действие по основной кнопке (индекс совпадает с позицией кнопки).
    pub actions: Vec<ConfirmAction>,
}

/// Окно ввода текста.
#[derive(Debug, Clone)]
pub struct KeyboardModal {
    /// Какое поле редактируем.
    pub field: TextField,
    /// Текущее значение.
    pub value: String,
    /// Клавиатура.
    pub keyboard: Keyboard,
}

/// Модальное окно.
#[derive(Debug, Clone)]
pub enum Modal {
    /// Нет окна.
    None,
    /// Подтверждение действия.
    Confirm(ConfirmModal),
    /// Карточка установленного приложения.
    AppInfo(String),
    /// Карточка пакета в магазине.
    StoreInfo(String),
    /// Ввод текста.
    Keyboard(KeyboardModal),
    /// Информационное сообщение.
    Message {
        /// Заголовок.
        title: String,
        /// Текст.
        body: String,
    },
}

impl Modal {
    /// Число фокусируемых элементов (для навигации).
    pub fn focus_count(&self) -> usize {
        match self {
            Modal::None | Modal::Message { .. } => 1,
            Modal::Confirm(c) => c.buttons.len().max(1),
            Modal::AppInfo(_) | Modal::StoreInfo(_) => 4,
            Modal::Keyboard(_) => 1,
        }
    }

    /// Открыто ли окно.
    pub fn is_open(&self) -> bool {
        !matches!(self, Modal::None)
    }
}

// ============================ Прогресс и уведомления ============================

/// Состояние длительной операции.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    /// Идёт ли операция.
    pub active: bool,
    /// Заголовок («Установка Luma»).
    pub title: String,
    /// Текущая стадия по-русски.
    pub stage: String,
    /// Доля выполнения 0…1; отрицательная — «неизвестно».
    pub fraction: f32,
    /// Подробности (размер, путь).
    pub detail: Option<String>,
    /// Ошибка, если операция сорвалась.
    pub error: Option<String>,
}

impl Progress {
    /// Начинает операцию.
    pub fn start(&mut self, title: impl Into<String>) {
        *self = Self {
            active: true,
            title: title.into(),
            stage: "Подготовка".into(),
            fraction: -1.0,
            detail: None,
            error: None,
        };
    }

    /// Завершает операцию успешно.
    pub fn finish(&mut self, stage: impl Into<String>) {
        self.stage = stage.into();
        self.fraction = 1.0;
        self.active = false;
    }

    /// Отмечает ошибку.
    pub fn fail(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
        self.active = false;
    }

    /// Применяет стадию установки.
    pub fn apply_stage(&mut self, stage: &InstallStage) {
        match stage {
            InstallStage::Detecting => {
                self.stage = "Определяем формат пакета".into();
                self.fraction = -1.0;
            }
            InstallStage::Downloading {
                progress,
                received,
                total,
            } => {
                self.stage = "Загружаем пакет".into();
                self.fraction = if *progress > 0.0 { *progress } else { -1.0 };
                self.detail = Some(match total {
                    Some(total) if *total > 0 => {
                        format!("{} из {}", format_size(*received), format_size(*total))
                    }
                    _ => format_size(*received),
                });
            }
            InstallStage::Verifying => {
                self.stage = "Проверяем контрольную сумму".into();
                self.fraction = self.fraction.max(0.0);
            }
            InstallStage::Converting { note } => {
                self.stage = "Собираем приложение webOS".into();
                self.detail = Some(note.clone());
                self.fraction = 0.35;
            }
            InstallStage::Inspecting => {
                self.stage = "Разбираем пакет".into();
                self.fraction = 0.55;
            }
            InstallStage::Writing { path } => {
                self.stage = "Записываем IPK на устройство".into();
                self.detail = Some(path.clone());
                self.fraction = 0.7;
            }
            InstallStage::Registering { strategy, status } => {
                self.stage = format!("Регистрируем: {}", strategy.label_ru());
                self.detail = Some(status.clone());
                self.fraction = 0.9;
            }
            InstallStage::Finished => {
                self.stage = "Готово".into();
                self.fraction = 1.0;
            }
        }
    }
}

/// Всплывающее уведомление.
#[derive(Debug, Clone)]
pub struct ToastState {
    /// Текст.
    pub message: String,
    /// Тип.
    pub kind: ToastKind,
    /// Сколько секунд уже показывается.
    pub age: f32,
    /// Сколько секунд показывать.
    pub duration: f32,
}

impl ToastState {
    /// Прозрачность (появление/исчезновение).
    pub fn alpha(&self) -> f32 {
        let fade_in = (self.age / 0.22).min(1.0);
        let remaining = self.duration - self.age;
        let fade_out = (remaining / 0.35).clamp(0.0, 1.0);
        fade_in.min(fade_out)
    }
}

/// Состояние синхронизации магазина.
#[derive(Debug, Clone, PartialEq)]
pub enum StoreSync {
    /// Ещё не синхронизировали.
    Idle,
    /// Идёт загрузка.
    Syncing,
    /// Готово.
    Ready(usize),
    /// Ошибка.
    Error(String),
}

impl StoreSync {
    /// Русский текст состояния.
    pub fn label_ru(&self) -> String {
        match self {
            StoreSync::Idle => "Нажмите «Обновить», чтобы загрузить список".into(),
            StoreSync::Syncing => "Обновляем репозитории…".into(),
            StoreSync::Ready(count) => format!(
                "{} в репозиториях",
                nova_core::i18n::plural_lang(
                    nova_core::i18n::Lang::Ru,
                    *count as u64,
                    ("пакет", "пакета", "пакетов"),
                    ("package", "packages")
                )
            ),
            StoreSync::Error(e) => format!("Ошибка: {e}"),
        }
    }
}

/// Фильтр в магазине.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreFilter {
    /// Всё подряд.
    All,
    /// Рекомендуемые.
    Featured,
    /// Есть обновление.
    Updates,
    /// Уже установлено.
    Installed,
    /// Веб-приложения.
    Web,
    /// Нативные приложения.
    Native,
}

impl StoreFilter {
    /// Все фильтры.
    pub const ALL: [StoreFilter; 6] = [
        StoreFilter::All,
        StoreFilter::Featured,
        StoreFilter::Updates,
        StoreFilter::Installed,
        StoreFilter::Web,
        StoreFilter::Native,
    ];

    /// Русская подпись.
    pub fn label_ru(&self) -> &'static str {
        match self {
            StoreFilter::All => "Все",
            StoreFilter::Featured => "Рекомендуемые",
            StoreFilter::Updates => "Обновления",
            StoreFilter::Installed => "Установленные",
            StoreFilter::Web => "Веб",
            StoreFilter::Native => "Нативные",
        }
    }
}

// ============================ Намерения ============================

/// Действие, которое экран просит выполнить.
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    /// Перейти на экран.
    Goto(Screen),
    /// Запустить приложение.
    Launch(String),
    /// Показать карточку приложения.
    AppInfo(String),
    /// Показать карточку пакета магазина.
    StoreInfo(String),
    /// Установить пакет из магазина.
    InstallStore(String),
    /// Переключить «избранное».
    ToggleFavorite(String),
    /// Удалить приложение.
    Uninstall(String),
    /// Обновить магазин.
    SyncStore,
    /// Обновить каталог.
    Rescan,
    /// Сменить тему.
    SetTheme(ThemeChoice),
    /// Следующая тема (циклически).
    CycleTheme,
    /// Сменить разрешение.
    SetResolution(Resolution),
    /// Показать/скрыть системные приложения.
    SetShowSystem(bool),
    /// Включить/выключить анимации.
    ToggleAnimations,
    /// 12/24-часовые часы.
    SetClock24h(bool),
    /// Сменить режим кнопки Home.
    SetHomeMode(HomeMode),
    /// Сменить обои.
    SetWallpaper(Wallpaper),
    /// Открыть ввод текста.
    OpenKeyboard(TextField),
    /// Добавить репозиторий.
    AddRepoDialog,
    /// Удалить репозиторий.
    RemoveRepo(usize),
    /// Включить/выключить репозиторий.
    ToggleRepo(usize),
    /// Запустить/остановить приёмник файлов.
    ToggleUpload,
    /// Сканировать USB-накопители на пакеты.
    ScanUsb,
    /// Перезагрузка.
    Reboot,
    /// Выключение.
    PowerOff,
    /// Сбросить избранное.
    ResetFavorites,
    /// Сбросить настройки.
    ResetSettings,
    /// Показать сообщение.
    Message {
        /// Заголовок.
        title: String,
        /// Текст.
        body: String,
    },
    /// Выйти из лаунчера.
    Quit,
}

// ============================ Состояние ============================

/// Информация о работающем приёмнике файлов.
#[derive(Debug)]
pub struct UploadSession {
    /// Порт.
    pub port: u16,
    /// Адрес для QR-кода.
    pub url: String,
    /// Последний принятый файл.
    pub last: Option<String>,
    handle: UploadHandle,
}

/// Всё состояние лаунчера.
pub struct AppState {
    /// Настройки.
    pub settings: Settings,
    /// Сведения об устройстве.
    pub sysinfo: SysInfo,
    /// Статус интеграции с Home.
    pub home_status: home::HomeStatus,
    /// Каталог установленных приложений.
    pub catalog: Catalog,
    /// Иконки приложений (id → изображение).
    pub icons: HashMap<String, Image>,
    /// Пакеты магазина.
    pub store: Vec<StoreItem>,
    /// Иконки магазина.
    pub store_icons: HashMap<String, Image>,
    /// Состояние синхронизации магазина.
    pub store_sync: StoreSync,
    /// Фильтр магазина.
    pub store_filter: StoreFilter,
    /// Поисковая строка в приложениях.
    pub apps_query: String,
    /// Поисковая строка в магазине.
    pub store_query: String,
    /// Текущий экран.
    pub screen: Screen,
    /// Фокус на каждом экране.
    pub focus: HashMap<Screen, Focus>,
    /// Модальное окно.
    pub modal: Modal,
    /// Фокус внутри модального окна.
    pub modal_focus: usize,
    /// Уведомление.
    pub toast: Option<ToastState>,
    /// Прогресс установки.
    pub progress: Progress,
    /// Тема оформления.
    pub theme: Theme,
    /// Время в секундах с запуска (для анимаций).
    pub now: f32,
    /// Плавность появления фокуса 0…1.
    pub focus_anim: f32,
    /// Строка часов.
    pub clock_text: String,
    /// Строка даты.
    pub date_text: String,
    /// Обои.
    pub wallpaper: Option<Image>,
    /// Приёмник файлов (экран «Установка»).
    pub upload: Option<UploadSession>,
    /// Пакеты, найденные на USB.
    pub usb_packages: Vec<PathBuf>,
    /// Индекс выбранного USB-пакета.
    pub usb_focus: usize,
    /// Просьба выйти из приложения.
    pub quit: bool,
    /// Время бездействия в секундах.
    pub idle: f32,
    /// Заставка активна.
    pub screensaver: bool,
    /// Смещение часового пояса в минутах.
    pub tz_offset: i32,

    env: WorkerEnv,
    cmd_tx: Sender<Command>,
    ev_rx: Receiver<Event>,
    inline_tx: Option<Sender<Event>>,
    worker: Option<JoinHandle<()>>,
    icons_pending: HashSet<String>,
    store_icons_pending: HashSet<String>,
    clock_at: f32,
}

impl AppState {
    /// Состояние с фоновым потоком (реальный запуск на ТВ и в окне SDL).
    pub fn spawn(settings: Settings) -> Self {
        let offline = !std::path::Path::new("/usr/bin/luna-send-pub").exists()
            && !std::path::Path::new("/usr/bin/luna-send").exists();
        let env = WorkerEnv::from_settings(&settings, offline);
        Self::with_env(settings, env, false)
    }

    /// Состояние с фоновым потоком и явной конфигурацией работника.
    pub fn spawn_env(settings: Settings, env: WorkerEnv) -> Self {
        Self::with_env(settings, env, false)
    }

    /// Состояние без потока: команды выполняются синхронно (тесты, снимки экранов).
    pub fn inline(settings: Settings, env: WorkerEnv) -> Self {
        Self::with_env(settings, env, true)
    }

    fn with_env(settings: Settings, env: WorkerEnv, inline: bool) -> Self {
        let theme = Theme::for_choice(settings.theme);
        let screen = settings
            .last_screen
            .as_deref()
            .and_then(Screen::from_id)
            .unwrap_or(Screen::Home);
        let (cmd_tx, cmd_rx) = channel();
        let (ev_tx, ev_rx) = channel();

        let worker = if inline {
            None
        } else {
            let tx = ev_tx.clone();
            let env = env.clone();
            Some(
                std::thread::Builder::new()
                    .name("nova-worker".into())
                    .spawn(move || worker_loop(cmd_rx, tx, env))
                    .expect("не удалось запустить фоновый поток NovaOS"),
            )
        };

        let mut state = Self {
            settings,
            sysinfo: SysInfo::default(),
            home_status: home::HomeStatus::default(),
            catalog: Catalog::default(),
            icons: HashMap::new(),
            store: Vec::new(),
            store_icons: HashMap::new(),
            store_sync: StoreSync::Idle,
            store_filter: StoreFilter::All,
            apps_query: String::new(),
            store_query: String::new(),
            screen,
            focus: HashMap::new(),
            modal: Modal::None,
            modal_focus: 0,
            toast: None,
            progress: Progress::default(),
            theme,
            now: 0.0,
            focus_anim: 1.0,
            clock_text: String::new(),
            date_text: String::new(),
            wallpaper: None,
            upload: None,
            usb_packages: Vec::new(),
            usb_focus: 0,
            quit: false,
            idle: 0.0,
            screensaver: false,
            tz_offset: clock::system_offset_minutes(),
            env,
            cmd_tx,
            ev_rx,
            inline_tx: inline.then_some(ev_tx),
            worker,
            icons_pending: HashSet::new(),
            store_icons_pending: HashSet::new(),
            clock_at: -100.0,
        };
        state.update_clock();
        state.load_wallpaper();
        state
    }

    /// Завершает работу: останавливает потоки и приёмник файлов.
    pub fn shutdown(&mut self) {
        if let Some(upload) = self.upload.take() {
            upload.handle.stop();
        }
        let _ = self.cmd_tx.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    /// Конфигурация работника (для диагностики и тестов).
    pub fn env(&self) -> &WorkerEnv {
        &self.env
    }

    // ---------------------------- команды ----------------------------

    fn send(&mut self, command: Command) {
        // В синхронном режиме команда выполняется сразу, и её события
        // применяются немедленно — так тесты и снимки экранов детерминированы.
        let inline = self.inline_tx.clone();
        if let Some(tx) = inline {
            handle_command(command, &tx, &mut self.env);
            self.poll();
        } else {
            let _ = self.cmd_tx.send(command);
        }
    }

    /// Пересылает настройки в фоновый поток.
    fn push_config(&mut self) {
        let mut env = WorkerEnv::from_settings(&self.settings, self.env.offline);
        env.scan_paths = self.env.scan_paths.clone();
        self.env = env.clone();
        self.send(Command::Configure(env));
    }

    /// Первичная загрузка данных при старте.
    pub fn bootstrap(&mut self) {
        self.send(Command::DetectSystem);
        self.send(Command::Rescan);
        if self.settings.auto_updates {
            self.request_store_sync();
        }
    }

    /// Запрашивает синхронизацию магазина.
    pub fn request_store_sync(&mut self) {
        if matches!(self.store_sync, StoreSync::Syncing) {
            return;
        }
        self.store_sync = StoreSync::Syncing;
        self.send(Command::SyncStore);
    }

    /// Запрашивает пересканирование каталога.
    pub fn request_rescan(&mut self) {
        self.send(Command::Rescan);
    }

    /// Подгружает иконки по списку запросов (id, путь), пропуская известные.
    pub fn ensure_icons(&mut self, requests: Vec<(String, PathBuf)>) {
        let mut wanted: Vec<(String, PathBuf)> = Vec::new();
        for (id, path) in requests {
            if self.icons.contains_key(&id) || self.icons_pending.contains(&id) {
                continue;
            }
            self.icons_pending.insert(id.clone());
            wanted.push((id, path));
        }
        if !wanted.is_empty() {
            self.send(Command::LoadIcons(wanted));
        }
    }

    /// Подгружает иконки пакетов магазина по списку (id, url).
    pub fn ensure_store_icons(&mut self, requests: Vec<(String, String)>) {
        let mut wanted: Vec<(String, String)> = Vec::new();
        for (id, url) in requests {
            if url.is_empty()
                || self.store_icons.contains_key(&id)
                || self.store_icons_pending.contains(&id)
            {
                continue;
            }
            self.store_icons_pending.insert(id.clone());
            wanted.push((id, url));
        }
        if !wanted.is_empty() {
            self.send(Command::LoadStoreIcons(wanted));
        }
    }

    /// Иконка приложения (если загружена).
    pub fn icon(&self, id: &str) -> Option<&Image> {
        self.icons.get(id)
    }

    /// Иконка пакета магазина.
    pub fn store_icon(&self, id: &str) -> Option<&Image> {
        self.store_icons.get(id)
    }

    // ---------------------------- данные ----------------------------

    /// Избранные приложения.
    pub fn favorites(&self) -> Vec<&InstalledApp> {
        self.catalog.favorites()
    }

    /// Приложения для экрана «Приложения» (с учётом фильтра и поиска).
    pub fn visible_apps(&self) -> Vec<&InstalledApp> {
        let query = self.apps_query.trim().to_lowercase();
        let mut apps: Vec<&InstalledApp> = self
            .catalog
            .apps
            .iter()
            .filter(|app| {
                if self.settings.show_system_apps {
                    true
                } else {
                    !matches!(app.source, nova_core::app::AppSource::System)
                }
            })
            .filter(|app| {
                query.is_empty()
                    || app.title().to_lowercase().contains(&query)
                    || app.info.id.to_lowercase().contains(&query)
            })
            .collect();
        apps.sort_by(|a, b| {
            b.favorite_order
                .is_some()
                .cmp(&a.favorite_order.is_some())
                .then_with(|| a.title().to_lowercase().cmp(&b.title().to_lowercase()))
        });
        apps
    }

    /// Пакеты магазина с учётом фильтра и поиска.
    pub fn visible_store(&self) -> Vec<&StoreItem> {
        let query = self.store_query.trim().to_lowercase();
        self.store
            .iter()
            .filter(|item| {
                if query.is_empty() {
                    true
                } else {
                    item.title.to_lowercase().contains(&query)
                        || item.id.to_lowercase().contains(&query)
                        || item.description.to_lowercase().contains(&query)
                }
            })
            .filter(|item| match self.store_filter {
                StoreFilter::All => true,
                StoreFilter::Featured => item.featured,
                StoreFilter::Web => item.app_type == "web",
                StoreFilter::Native => item.app_type == "native",
                StoreFilter::Updates => {
                    matches!(
                        item.state(self.installed_version(&item.id)),
                        nova_core::repo::StoreState::Update
                    )
                }
                StoreFilter::Installed => self.installed_version(&item.id).is_some(),
            })
            .filter(|item| {
                item.supports_webos(self.sysinfo.webos_release.as_deref())
                    || self.store_filter == StoreFilter::All
            })
            .collect()
    }

    /// Версия установленного приложения.
    pub fn installed_version(&self, id: &str) -> Option<&str> {
        self.catalog
            .by_id(id)
            .and_then(|app| app.info.version.as_deref())
            .filter(|v| !v.is_empty())
    }

    /// Пакет магазина по идентификатору.
    pub fn store_item(&self, id: &str) -> Option<&StoreItem> {
        self.store.iter().find(|item| item.id == id)
    }

    /// Русская подпись состояния пакета в магазине («Установить», «Обновить»…).
    pub fn store_action(&self, item: &StoreItem) -> &'static str {
        match item.state(self.installed_version(&item.id)) {
            nova_core::repo::StoreState::NotInstalled => "Установить",
            nova_core::repo::StoreState::Update => "Обновить",
            nova_core::repo::StoreState::Installed => "Открыть",
            nova_core::repo::StoreState::Newer => "Открыть",
        }
    }

    /// Приложение в избранном?
    pub fn is_favorite(&self, id: &str) -> bool {
        self.settings.favorites.iter().any(|f| f == id)
    }

    // ---------------------------- цикл ----------------------------

    /// Кадр: время, анимации, часы, события из потока, приёмник файлов.
    pub fn tick(&mut self, dt: f32) {
        self.now += dt;
        self.idle += dt;
        let limit = self.settings.screensaver_minutes as f32 * 60.0;
        self.screensaver = limit > 0.0 && self.idle > limit;

        let speed = if self.settings.animations { 11.0 } else { 60.0 };
        self.focus_anim = (self.focus_anim + dt * speed).min(1.0);

        if let Some(toast) = &mut self.toast {
            toast.age += dt;
            if toast.age >= toast.duration {
                self.toast = None;
            }
        }
        if self.now - self.clock_at > 15.0 {
            self.update_clock();
        }
        self.poll();
        // Сначала забираем все принятые файлы, потом обрабатываем:
        // `on_upload` меняет состояние, а приёмник одолжен неизменяемо.
        let mut received = Vec::new();
        if let Some(upload) = &self.upload {
            while let Some(file) = upload.handle.try_recv() {
                received.push(file);
            }
        }
        for file in received {
            self.on_upload(file);
        }
    }

    /// Сбрасывает таймер бездействия (вызывается на любое нажатие).
    pub fn activity(&mut self) {
        self.idle = 0.0;
        self.screensaver = false;
        self.focus_anim = if self.settings.animations { 0.0 } else { 1.0 };
    }

    /// Обновляет строки часов и даты.
    pub fn update_clock(&mut self) {
        self.clock_at = self.now;
        let parts = clock::parts_from_unix(clock::now_unix(), self.tz_offset);
        self.clock_text = clock::format_clock(&parts, self.settings.clock_24h);
        self.date_text = clock::format_date(&parts);
    }

    /// Принимает события из фонового потока.
    pub fn poll(&mut self) {
        loop {
            match self.ev_rx.try_recv() {
                Ok(event) => self.apply(event),
                Err(_) => break,
            }
        }
    }

    /// Применяет одно событие.
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::SysInfo(info) => {
                self.tz_offset = clock::system_offset_minutes();
                self.sysinfo = info;
            }
            Event::HomeStatus(status) => self.home_status = status,
            Event::Catalog(catalog) => {
                self.catalog = catalog;
                self.icons_pending.clear();
            }
            Event::Icons(icons) => {
                for (id, image) in icons {
                    self.icons_pending.remove(&id);
                    self.icons.insert(id, image);
                }
            }
            Event::Store(items) => {
                let count = items.len();
                self.store = items;
                self.store_sync = StoreSync::Ready(count);
                self.store_icons_pending.clear();
                for repo in &mut self.settings.repos {
                    repo.last_sync = Some(clock::format_short_date(&clock::parts_from_unix(
                        clock::now_unix(),
                        self.tz_offset,
                    )));
                }
                self.notify(format!("Магазин обновлён: {count}"), ToastKind::Ok);
            }
            Event::StoreError(message) => {
                self.store_sync = StoreSync::Error(message.clone());
                self.notify(message, ToastKind::Error);
            }
            Event::StoreIcons(icons) => {
                for (id, image) in icons {
                    self.store_icons_pending.remove(&id);
                    self.store_icons.insert(id, image);
                }
            }
            Event::InstallStage(stage) => {
                if !self.progress.active {
                    self.progress.start("Установка");
                }
                self.progress.apply_stage(&stage);
            }
            Event::InstallDone(report) => {
                self.progress.finish("Установлено");
                let title = report.title.clone();
                let strategy = report.strategy;
                let notes = report.notes.clone();
                self.notify(
                    match strategy {
                        Strategy::DryRun => {
                            format!("{title}: пакет проверен (установка недоступна)")
                        }
                        _ => format!("{title} установлено"),
                    },
                    if strategy == Strategy::DryRun {
                        ToastKind::Warn
                    } else {
                        ToastKind::Ok
                    },
                );
                for note in notes {
                    self.notify(note, ToastKind::Warn);
                }
                self.modal = Modal::Message {
                    title: "Установка завершена".into(),
                    body: install_summary(&report),
                };
                self.modal_focus = 0;
                self.progress.detail = None;
                self.settings.add_favorite(&report.app_id);
                self.save_settings();
                self.push_config();
            }
            Event::InstallFailed(message) => {
                self.progress.fail(message.clone());
                self.notify(message, ToastKind::Error);
            }
            Event::Uninstalled { id, strategy } => {
                self.settings.remove_favorite(&id);
                self.save_settings();
                self.push_config();
                self.notify(
                    match strategy {
                        Strategy::DryRun => format!("{id}: удаление недоступно в этом режиме"),
                        _ => format!("{id} удалено"),
                    },
                    if strategy == Strategy::DryRun {
                        ToastKind::Warn
                    } else {
                        ToastKind::Ok
                    },
                );
                self.modal = Modal::None;
            }
            Event::UninstallFailed(message) => self.notify(message, ToastKind::Error),
            Event::Launched(id) => {
                self.catalog.mark_running(&[id]);
            }
            Event::LaunchFailed(message) => self.notify(message, ToastKind::Error),
            Event::HomeApplied(message) => {
                self.notify(message, ToastKind::Ok);
                self.sysinfo.nova_home_enabled = self.settings.home_mode == HomeMode::Replaced;
            }
            Event::HomeFailed(message) => self.notify(message, ToastKind::Error),
            Event::Wallpaper(image) => self.wallpaper = Some(image),
            Event::Notice(message, kind) => self.notify(message, kind),
        }
    }

    /// Показывает уведомление.
    pub fn notify(&mut self, message: impl Into<String>, kind: ToastKind) {
        self.toast = Some(ToastState {
            message: message.into(),
            kind,
            age: 0.0,
            duration: 3.6,
        });
    }

    /// Файл пришёл с телефона — сразу предлагаем установить.
    fn on_upload(&mut self, file: nova_core::upload::Upload) {
        let name = file.filename.clone();
        let size = format_size(file.len() as u64);
        if let Some(session) = &mut self.upload {
            session.last = Some(name.clone());
        }
        let bytes = file.bytes;
        let install_name = name.clone();
        self.modal = Modal::Confirm(ConfirmModal {
            title: "Файл получен".into(),
            body: format!("{name} · {size}\n\nУстановить этот пакет на телевизор?"),
            buttons: vec![Button::primary("Установить"), Button::secondary("Позже")],
            actions: vec![
                ConfirmAction::Install {
                    source: InstallSource::Memory {
                        name: install_name,
                        bytes,
                    },
                    sha256: None,
                },
                ConfirmAction::None,
            ],
        });
        self.modal_focus = 0;
        self.notify(format!("Принят файл {name} ({size})"), ToastKind::Info);
    }

    // ---------------------------- настройки ----------------------------

    /// Сохраняет настройки на диск.
    pub fn save_settings(&mut self) {
        self.settings.last_screen = Some(self.screen.id().to_string());
        if let Err(e) = self.settings.save_default() {
            self.notify(
                format!("Не удалось сохранить настройки: {e}"),
                ToastKind::Warn,
            );
        }
    }

    fn apply_theme(&mut self) {
        self.theme = Theme::for_choice(self.settings.theme);
    }

    /// Загружает обои согласно настройкам.
    pub fn load_wallpaper(&mut self) {
        match self.settings.wallpaper.clone() {
            Wallpaper::File { path } => self.send(Command::LoadWallpaper(WallpaperSource::File(
                PathBuf::from(path),
            ))),
            Wallpaper::Url { url } => self.send(Command::LoadWallpaper(WallpaperSource::Url(url))),
            Wallpaper::Solid { color } => {
                let color = crate::gfx::Color::hex(&color);
                self.wallpaper = Some(Image::solid(64, 64, color).resized(1920, 1080));
            }
            Wallpaper::BuiltIn { .. } => self.wallpaper = None,
        }
    }

    // ---------------------------- намерения ----------------------------

    /// Выполняет действие интерфейса.
    pub fn handle(&mut self, intent: Intent) {
        self.activity();
        match intent {
            Intent::Goto(screen) => self.goto(screen),
            Intent::Launch(id) => self.launch(&id),
            Intent::AppInfo(id) => {
                self.modal = Modal::AppInfo(id);
                self.modal_focus = 0;
            }
            Intent::StoreInfo(id) => {
                self.modal = Modal::StoreInfo(id);
                self.modal_focus = 0;
            }
            Intent::InstallStore(id) => self.install_store(&id),
            Intent::ToggleFavorite(id) => self.toggle_favorite(&id),
            Intent::Uninstall(id) => self.confirm_uninstall(&id),
            Intent::SyncStore => self.request_store_sync(),
            Intent::Rescan => self.request_rescan(),
            Intent::SetTheme(theme) => {
                self.settings.theme = theme;
                self.apply_theme();
                self.save_settings();
                self.notify(format!("Тема: {}", theme.label_ru()), ToastKind::Info);
            }
            Intent::CycleTheme => {
                let themes = [
                    ThemeChoice::Nova,
                    ThemeChoice::Midnight,
                    ThemeChoice::Graphite,
                    ThemeChoice::Cinema,
                ];
                let index = themes
                    .iter()
                    .position(|t| *t == self.settings.theme)
                    .unwrap_or(0);
                let next = themes[(index + 1) % themes.len()];
                self.handle(Intent::SetTheme(next));
            }
            Intent::SetResolution(resolution) => {
                self.settings.resolution = resolution;
                self.save_settings();
                self.notify(
                    format!("Разрешение: {}", resolution.label_ru()),
                    ToastKind::Info,
                );
            }
            Intent::SetShowSystem(value) => {
                self.settings.show_system_apps = value;
                self.save_settings();
                self.push_config();
                self.request_rescan();
            }
            Intent::ToggleAnimations => {
                self.settings.animations = !self.settings.animations;
                self.save_settings();
                self.notify(
                    if self.settings.animations {
                        "Анимации включены"
                    } else {
                        "Анимации выключены"
                    },
                    ToastKind::Info,
                );
            }
            Intent::SetClock24h(value) => {
                self.settings.clock_24h = value;
                self.save_settings();
                self.update_clock();
            }
            Intent::SetHomeMode(mode) => self.confirm_home_mode(mode),
            Intent::SetWallpaper(wallpaper) => {
                self.settings.wallpaper = wallpaper;
                self.save_settings();
                self.load_wallpaper();
            }
            Intent::OpenKeyboard(field) => self.open_keyboard(field, String::new()),
            Intent::AddRepoDialog => self.open_keyboard(TextField::RepoUrl, String::new()),
            Intent::RemoveRepo(index) => {
                if index < self.settings.repos.len() {
                    let title = self.settings.repos[index].title.clone();
                    self.settings.repos.remove(index);
                    self.save_settings();
                    self.push_config();
                    self.notify(format!("Репозиторий «{title}» удалён"), ToastKind::Info);
                }
            }
            Intent::ToggleRepo(index) => {
                if let Some(repo) = self.settings.repos.get_mut(index) {
                    repo.enabled = !repo.enabled;
                    let title = repo.title.clone();
                    let enabled = repo.enabled;
                    self.save_settings();
                    self.push_config();
                    self.notify(
                        if enabled {
                            format!("«{title}» включён")
                        } else {
                            format!("«{title}» выключен")
                        },
                        ToastKind::Info,
                    );
                }
            }
            Intent::ToggleUpload => self.toggle_upload(),
            Intent::ScanUsb => self.scan_usb(),
            Intent::Reboot => self.confirm_system(ConfirmAction::Reboot),
            Intent::PowerOff => self.confirm_system(ConfirmAction::PowerOff),
            Intent::ResetFavorites => {
                self.settings.favorites.clear();
                self.save_settings();
                self.push_config();
                self.request_rescan();
                self.notify("Избранное очищено", ToastKind::Info);
            }
            Intent::ResetSettings => {
                let favorites = self.settings.favorites.clone();
                let repos = self.settings.repos.clone();
                self.settings = Settings::default();
                self.settings.favorites = favorites;
                self.settings.repos = repos;
                self.apply_theme();
                self.save_settings();
                self.push_config();
                self.notify("Настройки сброшены", ToastKind::Ok);
            }
            Intent::Message { title, body } => {
                self.modal = Modal::Message { title, body };
                self.modal_focus = 0;
            }
            Intent::Quit => {
                self.modal = Modal::Confirm(ConfirmModal {
                    title: "Выйти из NovaOS?".into(),
                    body: "Вы вернётесь в штатный интерфейс телевизора.".into(),
                    buttons: vec![Button::primary("Выйти"), Button::secondary("Остаться")],
                    actions: vec![ConfirmAction::Quit, ConfirmAction::None],
                });
                self.modal_focus = 0;
            }
        }
    }

    /// Переход на экран.
    pub fn goto(&mut self, screen: Screen) {
        if self.screen != screen {
            self.screen = screen;
            self.focus.entry(screen).or_default();
            self.focus_anim = if self.settings.animations { 0.0 } else { 1.0 };
            self.save_settings();
            if screen == Screen::Store
                && self.store.is_empty()
                && self.store_sync == StoreSync::Idle
            {
                self.request_store_sync();
            }
            if screen == Screen::Sideload && self.usb_packages.is_empty() {
                self.scan_usb();
            }
        }
    }

    /// Фокус текущего экрана.
    pub fn focus(&self) -> Focus {
        self.focus.get(&self.screen).copied().unwrap_or_default()
    }

    /// Устанавливает фокус текущего экрана.
    pub fn set_focus(&mut self, focus: Focus) {
        if self.focus.get(&self.screen) != Some(&focus) {
            self.focus.insert(self.screen, focus);
            self.activity();
        }
    }

    /// Двигает фокус текущего экрана.
    pub fn move_focus(&mut self, dx: i32, dy: i32, counts: &[usize]) {
        let mut focus = self.focus();
        let before = focus;
        move_focus(&mut focus, dx, dy, counts);
        if focus != before {
            self.set_focus(focus);
        }
    }

    /// Запускает приложение.
    pub fn launch(&mut self, id: &str) {
        if let Some(app) = self.catalog.by_id(id) {
            let title = app.title();
            self.notify(format!("Запускаем {title}…"), ToastKind::Info);
        }
        self.send(Command::Launch(id.to_string()));
    }

    /// Переключает избранное.
    pub fn toggle_favorite(&mut self, id: &str) {
        if self.is_favorite(id) {
            self.settings.remove_favorite(id);
            self.notify("Убрано из избранного", ToastKind::Info);
        } else {
            self.settings.add_favorite(id);
            self.notify("Добавлено в избранное", ToastKind::Ok);
        }
        self.catalog.apply_favorites(&self.settings.favorites);
        self.save_settings();
        self.push_config();
    }

    /// Просит установить пакет из магазина.
    pub fn install_store(&mut self, id: &str) {
        let Some(item) = self.store_item(id).cloned() else {
            self.notify("Пакет не найден в магазине", ToastKind::Error);
            return;
        };
        if let Some(version) = self.installed_version(&item.id) {
            if version == item.version {
                self.launch(&item.id);
                return;
            }
        }
        if !item.supports_webos(self.sysinfo.webos_release.as_deref()) {
            self.modal = Modal::Message {
                title: "Не подходит для этого телевизора".into(),
                body: format!(
                    "Пакет «{}» требует webOS {:?}, а на устройстве {:?}.",
                    item.title, item.webos_requirement, self.sysinfo.webos_release
                ),
            };
            self.modal_focus = 0;
            return;
        }
        let size = item
            .size
            .map(format_size)
            .unwrap_or_else(|| "размер неизвестен".into());
        let source = InstallSource::Url(item.ipk_url.clone());
        let sha256 = item.sha256.clone();
        self.modal = Modal::Confirm(ConfirmModal {
            title: format!("Установить {}?", item.title),
            body: format!(
                "Версия {} · {} · {}\n\n{}",
                item.version,
                size,
                item.repo_title,
                if item.root_required {
                    "Нужен root-доступ (Homebrew Channel)."
                } else {
                    "Устанавливается через системный сервис webOS."
                }
            ),
            buttons: vec![Button::primary("Установить"), Button::secondary("Отмена")],
            actions: vec![
                ConfirmAction::Install { source, sha256 },
                ConfirmAction::None,
            ],
        });
        self.modal_focus = 0;
    }

    /// Диалог удаления приложения.
    pub fn confirm_uninstall(&mut self, id: &str) {
        let title = self
            .catalog
            .by_id(id)
            .map(|app| app.title())
            .unwrap_or_else(|| id.to_string());
        let removable = self
            .catalog
            .by_id(id)
            .map(|app| app.removable())
            .unwrap_or(false);
        if !removable {
            self.modal = Modal::Message {
                title: "Нельзя удалить".into(),
                body: format!("«{title}» — штатное приложение webOS, оно защищено системой."),
            };
            self.modal_focus = 0;
            return;
        }
        self.modal = Modal::Confirm(ConfirmModal {
            title: format!("Удалить {title}?"),
            body: "Приложение и его данные будут стёрты с телевизора.".into(),
            buttons: vec![Button::danger("Удалить"), Button::secondary("Отмена")],
            actions: vec![
                ConfirmAction::Uninstall(id.to_string()),
                ConfirmAction::None,
            ],
        });
        self.modal_focus = 0;
    }

    /// Диалог смены режима кнопки Home.
    pub fn confirm_home_mode(&mut self, mode: HomeMode) {
        if mode == self.settings.home_mode {
            return;
        }
        let app_dir = nova_core::settings::app_dir().unwrap_or_else(|| {
            PathBuf::from("/media/cryptofs/apps/usr/palm/applications").join(nova_core::APP_ID)
        });
        let plan = home::plan(mode, &self.sysinfo, &app_dir, &app_dir.join("nova"));
        let body = if plan.steps.is_empty() {
            "Изменения применятся сразу.".to_string()
        } else {
            let mut text = plan.steps.join("\n");
            for warning in &plan.warnings {
                text.push_str(&format!("\n\n⚠ {warning}"));
            }
            text
        };
        self.modal = Modal::Confirm(ConfirmModal {
            title: format!("Кнопка Home: {}", mode.label_ru()),
            body,
            buttons: vec![Button::primary("Применить"), Button::secondary("Отмена")],
            actions: vec![ConfirmAction::SetHome(mode), ConfirmAction::None],
        });
        self.modal_focus = 0;
    }

    /// Диалог перезагрузки/выключения/выхода.
    pub fn confirm_system(&mut self, action: ConfirmAction) {
        let (title, body, verb) = match action {
            ConfirmAction::Reboot => (
                "Перезагрузить телевизор?",
                "Экран погаснет на несколько секунд, NovaOS запустится снова.",
                "Перезагрузить",
            ),
            ConfirmAction::PowerOff => (
                "Выключить телевизор?",
                "Питание будет отключено сразу после подтверждения.",
                "Выключить",
            ),
            ConfirmAction::Quit => (
                "Выйти из NovaOS?",
                "Вы вернётесь в штатный интерфейс телевизора.",
                "Выйти",
            ),
            _ => return,
        };
        self.modal = Modal::Confirm(ConfirmModal {
            title: title.into(),
            body: body.into(),
            buttons: vec![Button::primary(verb), Button::secondary("Отмена")],
            actions: vec![action, ConfirmAction::None],
        });
        self.modal_focus = 0;
    }

    /// Открывает экранную клавиатуру.
    pub fn open_keyboard(&mut self, field: TextField, value: String) {
        let keyboard = if field == TextField::SideloadUrl {
            Keyboard::url()
        } else {
            Keyboard::russian()
        };
        self.modal = Modal::Keyboard(KeyboardModal {
            field,
            value,
            keyboard,
        });
        self.modal_focus = 0;
    }

    /// Число кнопок в текущем модальном окне (для навигации).
    pub fn modal_button_count(&self) -> usize {
        match &self.modal {
            Modal::None | Modal::Message { .. } => 1,
            Modal::Confirm(confirm) => confirm.buttons.len().max(1),
            Modal::Keyboard(_) => 1,
            Modal::AppInfo(id) => {
                let removable = self
                    .catalog
                    .by_id(id)
                    .map(|app| app.removable())
                    .unwrap_or(false);
                if removable {
                    4
                } else {
                    3
                }
            }
            Modal::StoreInfo(_) => 2,
        }
    }

    /// Подпись основной кнопки карточки приложения.
    pub fn app_button_labels(&self, id: &str) -> Vec<String> {
        let app = self.catalog.by_id(id);
        let running = app.map(|a| a.running).unwrap_or(false);
        let favorite = self.is_favorite(id);
        let removable = app.map(|a| a.removable()).unwrap_or(false);
        let mut labels = vec![
            if running {
                "Запущено".to_string()
            } else {
                "Запустить".to_string()
            },
            if favorite {
                "Убрать из избранного".to_string()
            } else {
                "В избранное".to_string()
            },
        ];
        if removable {
            labels.push("Удалить".to_string());
        }
        labels.push("Закрыть".to_string());
        labels
    }

    /// Перемещает курсор экранной клавиатуры.
    pub fn modal_keyboard_move(&mut self, dx: i32, dy: i32) {
        if let Modal::Keyboard(input) = &mut self.modal {
            input.keyboard.move_focus(dx, dy);
        }
    }

    /// Стирает символ в поле ввода.
    pub fn modal_backspace(&mut self) {
        if let Modal::Keyboard(input) = &mut self.modal {
            input.value.pop();
        }
    }

    /// Добавляет символ в поле ввода.
    pub fn modal_push_char(&mut self, ch: char) {
        if let Modal::Keyboard(input) = &mut self.modal {
            input.value.push(ch);
        }
    }

    /// Текущее значение поля ввода.
    pub fn modal_value(&self) -> String {
        match &self.modal {
            Modal::Keyboard(input) => input.value.clone(),
            _ => String::new(),
        }
    }

    /// Клавиатура текущего окна ввода.
    pub fn modal_keyboard(&self) -> Option<Keyboard> {
        match &self.modal {
            Modal::Keyboard(input) => Some(input.keyboard.clone()),
            _ => None,
        }
    }

    /// Закрывает модальное окно.
    pub fn close_modal(&mut self) {
        self.modal = Modal::None;
        self.modal_focus = 0;
    }

    /// Нажатие «ОК» в модальном окне.
    pub fn modal_activate(&mut self) {
        let modal = std::mem::replace(&mut self.modal, Modal::None);
        match modal {
            Modal::None => {}
            Modal::Message { .. } => {}
            Modal::Confirm(confirm) => {
                let index = self
                    .modal_focus
                    .min(confirm.actions.len().saturating_sub(1));
                let action = confirm
                    .actions
                    .get(index)
                    .cloned()
                    .unwrap_or(ConfirmAction::None);
                self.run_confirm(action);
            }
            Modal::AppInfo(id) => {
                let index = self.modal_focus;
                let running = self
                    .catalog
                    .by_id(&id)
                    .map(|app| app.running)
                    .unwrap_or(false);
                let removable = self
                    .catalog
                    .by_id(&id)
                    .map(|app| app.removable())
                    .unwrap_or(false);
                self.close_modal();
                match index {
                    0 if running => self.notify("Приложение уже запущено", ToastKind::Info),
                    0 => self.handle(Intent::Launch(id)),
                    1 => self.handle(Intent::ToggleFavorite(id)),
                    2 if removable => self.handle(Intent::Uninstall(id)),
                    _ => {}
                }
                return;
            }
            Modal::StoreInfo(id) => {
                let index = self.modal_focus;
                self.close_modal();
                if index == 0 {
                    self.handle(Intent::InstallStore(id));
                }
                return;
            }
            Modal::Keyboard(mut input) => {
                let key = input.keyboard.key().to_string();
                match key.as_str() {
                    crate::widgets::KEY_DONE => {
                        let value = std::mem::take(&mut input.value);
                        let field = input.field;
                        self.submit_text(field, value);
                        return;
                    }
                    crate::widgets::KEY_CLEAR => {
                        input.value.clear();
                        self.modal = Modal::Keyboard(input);
                        return;
                    }
                    crate::widgets::KEY_BACK => {
                        input.value.pop();
                        self.modal = Modal::Keyboard(input);
                        return;
                    }
                    crate::widgets::KEY_SPACE => {
                        input.value.push(' ');
                        self.modal = Modal::Keyboard(input);
                        return;
                    }
                    crate::widgets::KEY_LAYOUT => {
                        input.keyboard = if matches!(input.keyboard.rows.get(1).and_then(|r| r.first()), Some(k) if k.as_str() == "й")
                        {
                            Keyboard::english()
                        } else {
                            Keyboard::russian()
                        };
                        self.modal = Modal::Keyboard(input);
                        return;
                    }
                    _ => {
                        if let Some(ch) = key.chars().next() {
                            input.value.push(ch);
                        }
                        self.modal = Modal::Keyboard(input);
                        return;
                    }
                }
            }
        }
        self.modal_focus = 0;
    }

    /// Выполняет подтверждённое действие.
    pub fn run_confirm(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::None => {}
            ConfirmAction::Install { source, sha256 } => self.start_install(source, sha256),
            ConfirmAction::Uninstall(id) => {
                self.progress.start(format!("Удаление {id}"));
                self.send(Command::Uninstall(id));
            }
            ConfirmAction::SetHome(mode) => {
                self.settings.home_mode = mode;
                self.save_settings();
                self.progress.start("Настройка кнопки Home");
                self.send(Command::ApplyHome(mode));
            }
            ConfirmAction::Reboot => {
                let client = if self.env.offline {
                    LunaClient::offline()
                } else {
                    LunaClient::new()
                };
                match home::reboot(&client) {
                    Ok(()) => self.notify("Перезагрузка…", ToastKind::Info),
                    Err(e) => {
                        self.notify(format!("Не удалось перезагрузить: {e}"), ToastKind::Error)
                    }
                }
            }
            ConfirmAction::PowerOff => {
                let client = if self.env.offline {
                    LunaClient::offline()
                } else {
                    LunaClient::new()
                };
                match home::power_off(&client) {
                    Ok(()) => self.notify("Выключение…", ToastKind::Info),
                    Err(e) => self.notify(format!("Не удалось выключить: {e}"), ToastKind::Error),
                }
            }
            ConfirmAction::Quit => self.quit = true,
            ConfirmAction::AddRepo(url) => self.add_repo(&url),
            ConfirmAction::RemoveRepo(index) => self.handle(Intent::RemoveRepo(index)),
            ConfirmAction::ToggleRepo(index) => self.handle(Intent::ToggleRepo(index)),
            ConfirmAction::ResetFavorites => self.handle(Intent::ResetFavorites),
        }
    }

    /// Начинает установку пакета.
    pub fn start_install(&mut self, source: InstallSource, sha256: Option<String>) {
        let name = source.display_name();
        self.progress.start(format!("Установка {name}"));
        self.send(Command::Install { source, sha256 });
    }

    /// Обрабатывает введённый текст.
    pub fn submit_text(&mut self, field: TextField, value: String) {
        let value = value.trim().to_string();
        if value.is_empty() {
            self.notify("Пустое значение", ToastKind::Warn);
            return;
        }
        match field {
            TextField::SideloadUrl => {
                let url = normalize_url(&value);
                self.start_install(InstallSource::Url(url), None);
            }
            TextField::SideloadPath => {
                let path = PathBuf::from(&value);
                if !path.exists() {
                    self.notify(format!("Файл не найден: {value}"), ToastKind::Error);
                    return;
                }
                self.start_install(InstallSource::File(path), None);
            }
            TextField::RepoUrl => self.add_repo(&value),
            TextField::WallpaperUrl => {
                self.settings.wallpaper = Wallpaper::Url {
                    url: normalize_url(&value),
                };
                self.save_settings();
                self.load_wallpaper();
                self.notify("Обои обновлены", ToastKind::Ok);
            }
        }
    }

    /// Добавляет репозиторий.
    pub fn add_repo(&mut self, url: &str) {
        let url = normalize_url(url);
        let title = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("репозиторий")
            .to_string();
        let repo = RepoRef {
            title: title.clone(),
            url: url.clone(),
            enabled: true,
            trusted: false,
            last_sync: None,
        };
        if self.settings.add_repo(repo) {
            self.save_settings();
            self.push_config();
            self.notify(format!("Репозиторий «{title}» добавлен"), ToastKind::Ok);
            self.request_store_sync();
        } else {
            self.notify("Такой репозиторий уже есть", ToastKind::Warn);
        }
    }

    /// Включает/выключает приёмник файлов и обновляет QR-адрес.
    pub fn toggle_upload(&mut self) {
        if let Some(session) = self.upload.take() {
            session.handle.stop();
            self.notify("Приём файлов выключен", ToastKind::Info);
            return;
        }
        let dir = self.env.work_dir.join("upload");
        match UploadServer::start(dir, DEFAULT_PORT)
            .or_else(|_| UploadServer::start(self.env.work_dir.join("upload"), 0))
        {
            Ok(server) => {
                let port = server.port();
                let handle = server.spawn();
                let url = handle.url(self.sysinfo.ip.as_deref());
                self.notify(format!("Приём файлов: {url}"), ToastKind::Ok);
                self.upload = Some(UploadSession {
                    port,
                    url,
                    last: None,
                    handle,
                });
            }
            Err(e) => self.notify(
                format!("Не удалось запустить приёмник: {e}"),
                ToastKind::Error,
            ),
        }
    }

    /// Адрес страницы приёма файлов (для QR-кода).
    pub fn upload_url(&self) -> Option<&str> {
        self.upload.as_ref().map(|s| s.url.as_str())
    }

    /// Сканирует съёмные накопители на пакеты.
    pub fn scan_usb(&mut self) {
        let candidates = nova_core::install::scan_removable(None);
        self.usb_packages = candidates
            .into_iter()
            .map(|c| c.path)
            .filter(|p| {
                let ext = p
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                matches!(
                    ext.as_str(),
                    "ipk" | "apk" | "zip" | "html" | "htm" | "m3u" | "m3u8"
                )
            })
            .collect();
        self.usb_focus = self
            .usb_focus
            .min(self.usb_packages.len().saturating_sub(1));
    }

    /// Устанавливает пакет с USB.
    pub fn install_usb(&mut self, index: usize) {
        if let Some(path) = self.usb_packages.get(index).cloned() {
            self.start_install(InstallSource::File(path), None);
        }
    }

    /// Текст «О системе» (экран About).
    pub fn about_lines(&self) -> Vec<(String, String)> {
        let compat = self.sysinfo.compatibility();
        let mut lines = vec![
            ("Совместимость".into(), compatibility_text(&compat)),
            ("Версия NovaOS".into(), nova_core::VERSION.into()),
            (
                "Устройство".into(),
                self.sysinfo
                    .model
                    .clone()
                    .unwrap_or_else(|| "неизвестно".into()),
            ),
            (
                "webOS".into(),
                self.sysinfo
                    .webos_release
                    .clone()
                    .unwrap_or_else(|| "—".into()),
            ),
            ("Архитектура".into(), self.sysinfo.arch.to_string()),
            (
                "Root-доступ".into(),
                if self.sysinfo.rooted {
                    "есть".into()
                } else {
                    "нет".into()
                },
            ),
            (
                "Homebrew Channel".into(),
                if self.sysinfo.homebrew_channel {
                    "установлен".into()
                } else {
                    "не найден".into()
                },
            ),
            (
                "IP-адрес".into(),
                self.sysinfo.ip.clone().unwrap_or_else(|| "—".into()),
            ),
            (
                "Часовой пояс".into(),
                format!(
                    "UTC{:+03}:{:02}",
                    self.tz_offset / 60,
                    self.tz_offset.rem_euclid(60)
                ),
            ),
            (
                "Свободное место".into(),
                match (
                    self.sysinfo.storage_free_bytes,
                    self.sysinfo.storage_total_bytes,
                ) {
                    (Some(free), Some(total)) if total > 0 => {
                        format!("{} из {}", format_size(free), format_size(total))
                    }
                    _ => "—".into(),
                },
            ),
            (
                "Установлено приложений".into(),
                self.catalog.len().to_string(),
            ),
            (
                "Настройка Home".into(),
                self.settings.home_mode.label_ru().to_string(),
            ),
        ];
        lines.push((
            "Кнопка Home".into(),
            match (
                self.home_status.enabled,
                self.home_status.boot_hook_installed,
            ) {
                (true, true) => "NovaOS вместо штатного Home".into(),
                (true, false) => "подмена активна до перезагрузки".into(),
                (false, true) => "автозапуск настроен".into(),
                (false, false) => "штатный Home LG".into(),
            },
        ));
        if let Some(path) = &self.home_status.lg_home_path {
            lines.push(("Home LG".into(), path.display().to_string()));
        }
        for note in &self.home_status.notes {
            lines.push(("Диагностика".into(), note.clone()));
        }
        lines
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_loop(rx: Receiver<Command>, tx: Sender<Event>, env: WorkerEnv) {
    let mut env = env;
    while let Ok(command) = rx.recv() {
        if matches!(command, Command::Shutdown) {
            break;
        }
        handle_command(command, &tx, &mut env);
    }
}

/// Человекочитаемый вердикт совместимости устройства.
pub fn compatibility_text(compat: &Compatibility) -> String {
    let base = if compat.supported {
        "поддерживается"
    } else {
        "не поддерживается"
    };
    match compat.warnings.first().or_else(|| compat.notes.first()) {
        Some(note) => format!("{base}: {note}"),
        None => base.to_string(),
    }
}

/// Собирает запросы иконок для списка приложений (без заимствования состояния).
pub fn icon_requests<'a, I>(apps: I) -> Vec<(String, PathBuf)>
where
    I: IntoIterator<Item = &'a InstalledApp>,
{
    apps.into_iter()
        .filter_map(|app| {
            app.icon_path
                .as_ref()
                .map(|path| (app.info.id.clone(), path.clone()))
        })
        .collect()
}

/// Краткая сводка об установленном пакете (для диалога «Установка завершена»).
pub fn install_summary(report: &InstallReport) -> String {
    let mut text = format!(
        "{} · версия {}\n{}\nСпособ: {}\nSHA-256: {}…",
        report.title,
        report.version,
        format_size(report.size_bytes),
        report.strategy.label_ru(),
        report.sha256.chars().take(12).collect::<String>()
    );
    if let Some(path) = &report.ipk_path {
        text.push_str(&format!("\nФайл: {}", path.display()));
    }
    for note in &report.notes {
        text.push_str(&format!("\n\n⚠ {note}"));
    }
    text
}

/// Приводит введённый адрес к URL (добавляет схему, если её нет).
pub fn normalize_url(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    format!("https://{trimmed}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::{Mutex, MutexGuard};
    use std::time::Duration;

    /// Все тесты состояния работают с настройками на диске — не мешаем друг другу.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Временный каталог, удаляется сам.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir()
                .join(format!("nova-state-{tag}-{}-{unique}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("временный каталог");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Состояние в синхронном режиме: команды выполняются сразу, без потоков.
    fn sandbox_state(tag: &str) -> (AppState, TempDir, MutexGuard<'static, ()>) {
        let guard = lock();
        let dir = TempDir::new(tag);
        std::env::set_var("NOVA_HOME", dir.path());
        let mut settings = Settings::default();
        settings.work_dir = Some(dir.path().join("work").to_string_lossy().into_owned());
        settings.auto_updates = false;
        settings.screensaver_minutes = 0;
        settings.repos = vec![RepoRef::new(
            "Локальный",
            "https://example.invalid/repo.json",
        )];
        let env = WorkerEnv::sandbox(dir.path());
        let state = AppState::inline(settings, env);
        (state, dir, guard)
    }

    /// Кладёт в песочницу «установленное» приложение webOS.
    fn fake_app(root: &Path, id: &str, title: &str, version: &str) {
        let dir = root
            .join("media/cryptofs/apps/usr/palm/applications")
            .join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("appinfo.json"),
            format!(
                r#"{{"id":"{id}","version":"{version}","type":"web","title":"{title}","main":"index.html"}}"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("index.html"), "<!doctype html><title>x</title>").unwrap();
    }

    fn store_item(id: &str, title: &str, app_type: &str, featured: bool) -> StoreItem {
        StoreItem {
            id: id.into(),
            title: title.into(),
            version: "1.0.0".into(),
            app_type: app_type.into(),
            description: format!("описание {title}"),
            icon_url: String::new(),
            ipk_url: format!("https://example.invalid/{id}.ipk"),
            sha256: None,
            size: Some(2048),
            source_url: String::new(),
            root_required: false,
            webos_requirement: None,
            screenshots: vec![],
            featured,
            repo_title: "Локальный".into(),
        }
    }

    #[test]
    fn screens_have_ids_and_titles() {
        for screen in Screen::ALL {
            assert!(!screen.id().is_empty());
            assert!(!screen.title_ru().is_empty());
            assert!(!screen.subtitle_ru().is_empty());
            assert!(!screen.glyph().is_empty());
            assert_eq!(Screen::from_id(screen.id()), Some(screen));
            assert_eq!(screen.next().prev(), screen);
        }
        assert_eq!(Screen::from_id("нет-такого"), None);
        assert_eq!(Screen::ALL.len(), 6);
    }

    #[test]
    fn focus_moves_within_and_between_sections() {
        let mut focus = Focus::default();
        let counts = [3usize, 0, 4, 2];
        move_focus(&mut focus, 1, 0, &counts);
        assert_eq!(focus, Focus::new(0, 1));
        move_focus(&mut focus, 5, 0, &counts);
        assert_eq!(focus, Focus::new(0, 0), "циклический переход вправо");
        move_focus(&mut focus, -1, 0, &counts);
        assert_eq!(focus, Focus::new(0, 2), "циклический переход влево");
        move_focus(&mut focus, 0, 1, &counts);
        assert_eq!(focus, Focus::new(2, 2), "пустой раздел пропускается");
        move_focus(&mut focus, 0, 1, &counts);
        assert_eq!(
            focus,
            Focus::new(3, 1),
            "индекс ужимается под размер раздела"
        );
        move_focus(&mut focus, 0, 1, &counts);
        assert_eq!(
            focus,
            Focus::new(0, 1),
            "цикл по вертикали сохраняет колонку"
        );
        move_focus(&mut focus, 1, 1, &[]);
        assert_eq!(focus, Focus::new(0, 0));
        // Все разделы пусты — фокус не выходит за границы.
        move_focus(&mut focus, 3, 3, &[0, 0]);
        assert_eq!(focus, Focus::new(0, 0));
    }

    #[test]
    fn inline_worker_rescans_catalog() {
        let (mut state, dir, _guard) = sandbox_state("rescan");
        fake_app(dir.path(), "com.test.demo", "Демо", "1.2.3");
        state.bootstrap();
        assert_eq!(state.catalog.len(), 1, "каталог должен найтись");
        let app = state.catalog.by_id("com.test.demo").expect("приложение");
        assert_eq!(app.title(), "Демо");
        assert_eq!(app.info.version.as_deref(), Some("1.2.3"));
        // В desktop-режиме устройство определяется как тестовый UM7300.
        assert_eq!(state.sysinfo.model.as_deref(), Some("UM7300PLB"));
    }

    #[test]
    fn store_filter_and_search() {
        let (mut state, _dir, _guard) = sandbox_state("store");
        state.store = vec![
            store_item("com.a", "Alpha", "web", true),
            store_item("com.b", "Beta", "native", false),
        ];
        state.store_sync = StoreSync::Ready(2);
        assert_eq!(state.visible_store().len(), 2);
        assert!(StoreSync::Ready(2).label_ru().contains("пакет"));

        state.store_filter = StoreFilter::Featured;
        assert_eq!(state.visible_store().len(), 1);
        assert_eq!(state.visible_store()[0].id, "com.a");

        state.store_filter = StoreFilter::Native;
        assert_eq!(state.visible_store()[0].id, "com.b");

        state.store_filter = StoreFilter::All;
        state.store_query = "альфа".into();
        assert!(state.visible_store().is_empty());
        state.store_query = "alph".into();
        assert_eq!(state.visible_store().len(), 1);
        state.store_query = String::new();

        assert_eq!(state.store_action(&state.store[0]), "Установить");
        state.catalog.apps.push(InstalledApp {
            info: nova_core::app::WebOsAppInfo {
                id: "com.a".into(),
                version: Some("0.9.0".into()),
                title: Some("Alpha".into()),
                ..Default::default()
            },
            path: PathBuf::from("/tmp/com.a"),
            source: nova_core::app::AppSource::User,
            size_bytes: None,
            icon_path: None,
            running: false,
            favorite_order: None,
        });
        assert_eq!(state.store_action(&state.store[0]), "Обновить");
        state.store_filter = StoreFilter::Updates;
        assert_eq!(state.visible_store().len(), 1);
        state.store_filter = StoreFilter::Installed;
        assert_eq!(state.visible_store().len(), 1);
    }

    #[test]
    fn install_from_url_confirms_then_starts() {
        let (mut state, _dir, _guard) = sandbox_state("install-url");
        state.store = vec![store_item("com.test.pkg", "Тест", "web", false)];
        state.handle(Intent::InstallStore("com.test.pkg".into()));
        assert!(matches!(state.modal, Modal::Confirm(_)));
        assert_eq!(state.modal.focus_count(), 2);
        state.modal_activate();
        // Пакет недоступен (нет сети в сборке без `net`) — операция завершается ошибкой.
        assert!(!state.progress.active);
        assert!(
            state.progress.error.is_some() || state.progress.fraction >= 0.0,
            "прогресс должен отреагировать"
        );
        assert!(state.toast.is_some());
    }

    #[test]
    fn install_store_of_installed_app_launches_it() {
        let (mut state, dir, _guard) = sandbox_state("install-installed");
        fake_app(dir.path(), "com.test.same", "Такое же", "1.0.0");
        state.request_rescan();
        let mut item = store_item("com.test.same", "Такое же", "web", false);
        item.ipk_url = "https://example.invalid/same.ipk".into();
        state.store = vec![item];
        state.handle(Intent::InstallStore("com.test.same".into()));
        assert!(
            matches!(state.modal, Modal::None),
            "диалог не нужен — приложение уже установлено"
        );
    }

    #[test]
    fn keyboard_input_builds_value() {
        let (mut state, _dir, _guard) = sandbox_state("keyboard");
        state.handle(Intent::OpenKeyboard(TextField::SideloadUrl));
        assert!(matches!(state.modal, Modal::Keyboard(_)));
        if let Modal::Keyboard(input) = &mut state.modal {
            input.keyboard = Keyboard::url();
            input.keyboard.row = 1;
            input.keyboard.col = 1;
        }
        for _ in 0..3 {
            state.modal_activate();
        }
        let value = match &state.modal {
            Modal::Keyboard(input) => input.value.clone(),
            _ => panic!("клавиатура должна оставаться открытой"),
        };
        assert_eq!(value, "www");

        // «Очистить» и «Стереть» работают.
        if let Modal::Keyboard(input) = &mut state.modal {
            input.keyboard.row = 5;
            input.keyboard.col = 0; // Очистить
        }
        state.modal_activate();
        match &mut state.modal {
            Modal::Keyboard(input) => assert!(input.value.is_empty()),
            _ => panic!("окно ввода пропало"),
        }

        // Подтверждение пустого значения — предупреждение.
        if let Modal::Keyboard(input) = &mut state.modal {
            input.keyboard.row = 5;
            input.keyboard.col = 2; // Готово
        }
        state.modal_activate();
        assert!(matches!(state.modal, Modal::None));
        assert!(state.toast.is_some());
    }

    #[test]
    fn url_and_path_submission() {
        let (mut state, dir, _guard) = sandbox_state("submit");
        state.submit_text(TextField::SideloadUrl, "example.com/app.ipk".into());
        assert!(
            !state.progress.active,
            "без сети установка сразу завершается"
        );

        let file = dir.path().join("demo.ipk");
        std::fs::write(&file, b"not a package").unwrap();
        state.submit_text(TextField::SideloadPath, file.to_string_lossy().into_owned());
        assert!(state.toast.is_some());

        state.submit_text(TextField::SideloadPath, "/нет/такого/файла.ipk".into());
        assert!(matches!(state.toast, Some(ref t) if t.kind == ToastKind::Error));

        state.submit_text(TextField::SideloadUrl, "   ".into());
        assert!(matches!(state.toast, Some(ref t) if t.kind == ToastKind::Warn));

        assert_eq!(
            normalize_url("example.com/a.ipk"),
            "https://example.com/a.ipk"
        );
        assert_eq!(normalize_url("http://x/a.ipk"), "http://x/a.ipk");
        assert_eq!(normalize_url("  https://y  "), "https://y");
    }

    #[test]
    fn favorites_round_trip() {
        let (mut state, dir, _guard) = sandbox_state("favorites");
        fake_app(dir.path(), "com.test.fav", "Избранное", "1.0.0");
        state.request_rescan();
        assert!(!state.is_favorite("com.test.fav"));
        state.handle(Intent::ToggleFavorite("com.test.fav".into()));
        assert!(state.is_favorite("com.test.fav"));
        assert_eq!(state.favorites().len(), 1);
        assert_eq!(state.visible_apps().len(), 1);
        state.handle(Intent::ToggleFavorite("com.test.fav".into()));
        assert!(!state.is_favorite("com.test.fav"));
        assert!(state.favorites().is_empty());

        state.handle(Intent::ToggleFavorite("com.test.fav".into()));
        state.handle(Intent::ResetFavorites);
        assert!(state.settings.favorites.is_empty());
    }

    #[test]
    fn apps_query_filters_catalog() {
        let (mut state, dir, _guard) = sandbox_state("query");
        fake_app(dir.path(), "com.test.luma", "Luma", "2.16.0");
        fake_app(dir.path(), "com.test.hb", "Homebrew Channel", "0.7.3");
        state.request_rescan();
        assert_eq!(state.visible_apps().len(), 2);
        state.apps_query = "luma".into();
        let apps = state.visible_apps();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].info.id, "com.test.luma");
        state.apps_query = "com.test.hb".into();
        assert_eq!(state.visible_apps().len(), 1);
        state.apps_query = "ничего".into();
        assert!(state.visible_apps().is_empty());
    }

    #[test]
    fn uninstall_requires_confirmation() {
        let (mut state, dir, _guard) = sandbox_state("uninstall");
        fake_app(dir.path(), "com.test.rm", "Удаляемое", "1.0.0");
        state.request_rescan();
        state.handle(Intent::Uninstall("com.test.rm".into()));
        assert!(matches!(state.modal, Modal::Confirm(_)));
        state.modal_focus = 1; // «Отмена»
        state.modal_activate();
        assert_eq!(state.catalog.len(), 1, "приложение должно остаться");

        state.handle(Intent::Uninstall("com.test.rm".into()));
        state.modal_focus = 0; // «Удалить»
        state.modal_activate();
        // В desktop-режиме удаление недоступно, но каталог пересканирован.
        assert!(state.toast.is_some());
    }

    #[test]
    fn toast_fades_and_expires() {
        let (mut state, _dir, _guard) = sandbox_state("toast");
        state.notify("Проверка", ToastKind::Info);
        assert!(
            state.toast.as_ref().unwrap().alpha() < 1.0,
            "появление плавное"
        );
        state.toast.as_mut().unwrap().age = 1.0;
        assert!((state.toast.as_ref().unwrap().alpha() - 1.0).abs() < 1e-3);
        state.toast.as_mut().unwrap().age = 10.0;
        state.tick(0.016);
        assert!(state.toast.is_none(), "уведомление должно исчезнуть");
    }

    #[test]
    fn tick_updates_clock_and_screensaver() {
        let (mut state, _dir, _guard) = sandbox_state("tick");
        state.settings.screensaver_minutes = 1;
        state.settings.animations = true;
        state.activity();
        state.tick(0.5);
        assert!(!state.clock_text.is_empty());
        assert!(!state.date_text.is_empty());
        assert!(state.focus_anim > 0.0 && state.focus_anim <= 1.0);
        for _ in 0..200 {
            state.tick(0.5);
        }
        assert!(state.screensaver, "заставка должна включиться");
        state.activity();
        assert!(!state.screensaver);
        state.settings.animations = false;
        state.activity();
        assert!(
            (state.focus_anim - 1.0).abs() < 1e-6,
            "без анимаций фокус мгновенный"
        );
    }

    #[test]
    fn progress_maps_install_stages() {
        let mut progress = Progress::default();
        assert!(!progress.active);
        progress.start("Установка Luma");
        assert!(progress.active);
        progress.apply_stage(&InstallStage::Detecting);
        assert!(progress.fraction < 0.0);
        progress.apply_stage(&InstallStage::Downloading {
            progress: 0.5,
            received: 1024,
            total: Some(2048),
        });
        assert!((progress.fraction - 0.5).abs() < 1e-6);
        assert!(progress.detail.as_deref().unwrap().contains("из"));
        progress.apply_stage(&InstallStage::Converting {
            note: "HTML → IPK".into(),
        });
        assert!(progress.stage.contains("webOS"));
        progress.apply_stage(&InstallStage::Registering {
            strategy: Strategy::HomebrewExec,
            status: "устанавливаем".into(),
        });
        assert!(progress.stage.contains("Homebrew"));
        progress.apply_stage(&InstallStage::Finished);
        assert!((progress.fraction - 1.0).abs() < 1e-6);
        progress.finish("Готово");
        assert!(!progress.active);
        progress.fail("сбой");
        assert_eq!(progress.error.as_deref(), Some("сбой"));
    }

    #[test]
    fn repo_management() {
        let (mut state, _dir, _guard) = sandbox_state("repos");
        state.settings.repos.clear();
        state.add_repo("example.com/repo.json");
        assert_eq!(state.settings.repos.len(), 1);
        assert!(state.settings.repos[0].url.starts_with("https://"));
        assert!(state.settings.repos[0].enabled);
        state.add_repo("https://example.com/repo.json");
        assert_eq!(state.settings.repos.len(), 1, "дубликаты не добавляются");
        state.handle(Intent::ToggleRepo(0));
        assert!(!state.settings.repos[0].enabled);
        state.handle(Intent::RemoveRepo(0));
        assert!(state.settings.repos.is_empty());
        state.handle(Intent::RemoveRepo(99));
        assert!(state.settings.repos.is_empty());
    }

    #[test]
    fn upload_session_starts_and_stops() {
        let (mut state, _dir, _guard) = sandbox_state("upload-session");
        assert!(state.upload.is_none());
        assert!(state.upload_url().is_none());
        state.handle(Intent::ToggleUpload);
        let port = {
            let session = state.upload.as_ref().expect("приёмник запущен");
            assert!(session.port > 0);
            assert!(session.url.starts_with("http://"), "{}", session.url);
            session.port
        };
        assert!(state.upload_url().is_some());
        state.handle(Intent::ToggleUpload);
        assert!(state.upload.is_none());
        assert!(port > 0);
    }

    #[test]
    fn upload_from_phone_offers_install() {
        let (mut state, _dir, _guard) = sandbox_state("upload-file");
        state.handle(Intent::ToggleUpload);
        let port = state.upload.as_ref().unwrap().port;
        let payload = b"!<arch>\ndebian-binary".to_vec();
        let client = std::thread::spawn(move || {
            use std::io::Write;
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut request = format!(
                "POST /upload HTTP/1.1\r\nHost: tv\r\nX-Filename: demo.ipk\r\nContent-Length: {}\r\n\r\n",
                payload.len()
            )
            .into_bytes();
            request.extend_from_slice(&payload);
            stream.write_all(&request).unwrap();
            let mut response = Vec::new();
            let _ = stream.read_to_end(&mut response);
            String::from_utf8_lossy(&response).contains("200")
        });
        assert!(client.join().unwrap(), "сервер должен ответить 200");

        let mut tries = 0;
        while matches!(state.modal, Modal::None) && tries < 300 {
            state.tick(0.02);
            tries += 1;
        }
        assert!(
            matches!(state.modal, Modal::Confirm(_)),
            "должно появиться подтверждение установки"
        );
        let body = match &state.modal {
            Modal::Confirm(confirm) => confirm.body.clone(),
            _ => String::new(),
        };
        assert!(body.contains("demo.ipk"), "{body}");
        assert!(state.upload.as_ref().unwrap().last.as_deref() == Some("demo.ipk"));
        state.handle(Intent::ToggleUpload);
    }

    #[test]
    fn usb_scan_collects_packages() {
        let (mut state, dir, _guard) = sandbox_state("usb");
        let volume = dir.path().join("media/usb1");
        std::fs::create_dir_all(&volume).unwrap();
        std::fs::write(volume.join("app.ipk"), b"!<arch>\n").unwrap();
        std::fs::write(volume.join("readme.txt"), b"not a package").unwrap();
        // Сканируем конкретный каталог песочницы вместо системных /media.
        let candidates =
            nova_core::install::scan_removable(Some(&[volume.to_string_lossy().as_ref()]));
        assert!(!candidates.is_empty(), "пакет на флешке должен найтись");
        state.scan_usb();
        // Системные пути в песочнице пусты — просто проверяем, что вызов безопасен.
        assert!(state.usb_focus == 0);
    }

    #[test]
    fn about_lists_device_facts() {
        let (mut state, _dir, _guard) = sandbox_state("about");
        state.send(Command::DetectSystem);
        let lines = state.about_lines();
        let find = |key: &str| {
            lines
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(find("Версия NovaOS"), nova_core::VERSION);
        assert!(find("Устройство").contains("UM73"), "{:?}", lines);
        assert!(!find("Архитектура").is_empty());
        assert!(!find("Совместимость").is_empty());
        assert!(find("Часовой пояс").starts_with("UTC"));
        assert!(!find("Кнопка Home").is_empty());
        assert!(find("Home LG").is_empty() || find("Home LG").contains('/'));
    }

    #[test]
    fn system_confirmation_dialogs() {
        let (mut state, _dir, _guard) = sandbox_state("confirm");
        state.handle(Intent::Reboot);
        assert!(matches!(state.modal, Modal::Confirm(_)));
        assert_eq!(state.modal.focus_count(), 2);
        state.close_modal();
        assert!(matches!(state.modal, Modal::None));

        state.handle(Intent::PowerOff);
        assert!(matches!(state.modal, Modal::Confirm(_)));
        state.modal_focus = 1;
        state.modal_activate();
        assert!(!state.quit, "отмена не должна закрывать лаунчер");

        state.handle(Intent::Quit);
        assert!(matches!(state.modal, Modal::Confirm(_)));
        state.modal_focus = 0;
        state.modal_activate();
        assert!(state.quit, "выход должен выставляться после подтверждения");
    }

    #[test]
    fn home_mode_confirmation_builds_plan() {
        let (mut state, _dir, _guard) = sandbox_state("home");
        state.send(Command::DetectSystem);
        state.handle(Intent::SetHomeMode(HomeMode::Replaced));
        let body = match &state.modal {
            Modal::Confirm(confirm) => confirm.body.clone(),
            _ => panic!("ожидался диалог настройки Home"),
        };
        assert!(!body.is_empty());
        assert!(state.modal.focus_count() >= 2);
        state.modal_focus = 0;
        state.modal_activate();
        assert_eq!(state.settings.home_mode, HomeMode::Replaced);
        // Возврат к штатному Home.
        state.handle(Intent::SetHomeMode(HomeMode::Disabled));
        state.modal_focus = 0;
        state.modal_activate();
        assert_eq!(state.settings.home_mode, HomeMode::Disabled);
    }

    #[test]
    fn theme_change_applies_palette() {
        let (mut state, _dir, _guard) = sandbox_state("theme");
        let nova = Theme::for_choice(ThemeChoice::Nova).palette.background;
        state.handle(Intent::CycleTheme);
        assert_ne!(state.settings.theme, ThemeChoice::Nova);
        assert_ne!(state.theme.palette.background, nova);
        state.handle(Intent::SetTheme(ThemeChoice::Nova));
        assert_eq!(state.settings.theme, ThemeChoice::Nova);
        assert_eq!(state.theme.palette.background, nova);
        state.handle(Intent::SetResolution(Resolution::Full));
        assert_eq!(state.settings.resolution, Resolution::Full);
    }

    #[test]
    fn settings_persist_between_states() {
        let (mut state, dir, guard) = sandbox_state("persist");
        state.handle(Intent::ToggleAnimations);
        state.handle(Intent::SetClock24h(false));
        state.goto(Screen::Store);
        state.save_settings();
        let animations = state.settings.animations;
        drop(state);

        // Те же настройки читаются новым состоянием из того же NOVA_HOME.
        std::env::set_var("NOVA_HOME", dir.path());
        let settings = Settings::load_default();
        assert_eq!(settings.animations, animations);
        assert!(!settings.clock_24h);
        assert_eq!(settings.last_screen.as_deref(), Some("store"));
        assert_eq!(
            Screen::from_id(&settings.last_screen.clone().unwrap()),
            Some(Screen::Store)
        );
        drop(guard);
    }

    #[test]
    fn install_report_summary_is_readable() {
        let report = InstallReport {
            app_id: "com.test.app".into(),
            title: "Тест".into(),
            version: "1.0.0".into(),
            kind: nova_core::app::AppKind::Web,
            format: nova_core::sniff::PackageFormat::Ipk,
            strategy: Strategy::HomebrewExec,
            sha256: "0123456789abcdef0123456789abcdef".into(),
            size_bytes: 2048,
            ipk_path: Some(PathBuf::from("/tmp/test.ipk")),
            notes: vec!["пакет для arm".into()],
        };
        let text = install_summary(&report);
        assert!(text.contains("Тест"));
        assert!(text.contains("1.0.0"));
        assert!(text.contains("Homebrew Channel"));
        assert!(text.contains("0123456789ab"));
        assert!(text.contains("/tmp/test.ipk"));
        assert!(text.contains("пакет для arm"));
    }

    #[test]
    fn real_ipk_installs_in_dry_run() {
        let sample = PathBuf::from("/home/user/.build/samples/hbchannel.ipk");
        if !sample.exists() {
            return;
        }
        let (mut state, _dir, _guard) = sandbox_state("real-ipk");
        state.start_install(InstallSource::File(sample), None);
        assert!(
            matches!(&state.modal, Modal::Message { title, .. } if title.contains("завершена")),
            "реальный IPK должен пройти конвейер: {:?}",
            state.progress.error
        );
        assert!(state.progress.error.is_none());
        assert!(
            !state.settings.favorites.is_empty(),
            "установленное приложение попадает в избранное"
        );
    }
}

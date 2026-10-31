//! Конфигурация NovaOS.
//!
//! Хранится одним JSON-файлом, пишется атомарно (временный файл + `rename`),
//! чтобы сбой питания телевизора не оставил половину настроек.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;

/// Тема оформления.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// «Nova» — тёмная с холодным акцентом (по умолчанию).
    #[default]
    Nova,
    /// «Полночь» — почти чёрный фон, тёплый акцент.
    Midnight,
    /// «Графит» — нейтральный серый.
    Graphite,
    /// «Кинотеатр» — глубокий синий, как в зале.
    Cinema,
}

impl Theme {
    /// Все темы.
    pub const ALL: [Theme; 4] = [Theme::Nova, Theme::Midnight, Theme::Graphite, Theme::Cinema];

    /// Русское название.
    pub fn label_ru(&self) -> &'static str {
        match self {
            Theme::Nova => "Nova",
            Theme::Midnight => "Полночь",
            Theme::Graphite => "Графит",
            Theme::Cinema => "Кинотеатр",
        }
    }
}

/// Разрешение интерфейса.
///
/// На слабых ARM-телевизорах 2016–2019 годов рендеринг в 720p с аппаратным
/// масштабированием до 1080p даёт заметно более плавный интерфейс.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// 720p — быстрее на старых ТВ.
    Fast,
    /// 1080p — максимальная чёткость.
    #[default]
    Full,
}

impl Resolution {
    /// Размеры буфера кадра.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Resolution::Fast => (1280, 720),
            Resolution::Full => (1920, 1080),
        }
    }

    /// Русское название.
    pub fn label_ru(&self) -> &'static str {
        match self {
            Resolution::Fast => "1280×720 (быстро)",
            Resolution::Full => "1920×1080 (чётко)",
        }
    }
}

/// Фоновое изображение.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Wallpaper {
    /// Встроенный фон.
    BuiltIn {
        /// Идентификатор встроенного фона.
        id: String,
    },
    /// Однотонный фон.
    Solid {
        /// Цвет в формате `#rrggbb`.
        color: String,
    },
    /// Файл на устройстве (например, с USB).
    File {
        /// Путь к изображению.
        path: String,
    },
    /// Изображение по URL (загружается и кэшируется локально).
    Url {
        /// Адрес изображения.
        url: String,
    },
}

impl Default for Wallpaper {
    fn default() -> Self {
        Wallpaper::BuiltIn {
            id: "nova-dunes".into(),
        }
    }
}

/// Режим интеграции с кнопкой Home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HomeMode {
    /// Не вмешиваться: NovaOS — обычное приложение.
    #[default]
    Disabled,
    /// Заменять Home при каждом старте системы (bind-mount).
    Replaced,
    /// Запускать NovaOS при старте, но не подменять Home.
    Autostart,
}

impl HomeMode {
    /// Русское название.
    pub fn label_ru(&self) -> &'static str {
        match self {
            HomeMode::Disabled => "Выключено",
            HomeMode::Replaced => "NovaOS вместо Home",
            HomeMode::Autostart => "Запускать при включении",
        }
    }
}

/// Способ установки пакетов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InstallBackend {
    /// Выбирать автоматически (по умолчанию).
    #[default]
    Auto,
    /// Только `appInstallService` (нужен режим разработчика).
    AppInstallService,
    /// Только через Homebrew Channel (нужен root).
    Homebrew,
    /// Только ручная распаковка в cryptofs (нужен root).
    Manual,
}

/// Ссылка на репозиторий пакетов.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepoRef {
    /// Название для интерфейса.
    pub title: String,
    /// Адрес `repo.json`.
    pub url: String,
    /// Включён ли репозиторий.
    pub enabled: bool,
    /// Доверенный репозиторий (не спрашивать подтверждение при установке).
    #[serde(default)]
    pub trusted: bool,
    /// Время последней синхронизации (RFC 3339).
    #[serde(default)]
    pub last_sync: Option<String>,
}

impl RepoRef {
    /// Новая ссылка на репозиторий.
    pub fn new(title: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            enabled: true,
            trusted: false,
            last_sync: None,
        }
    }
}

/// Внешний источник медиа (плейлист IPTV, DLNA, SMB).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaSource {
    /// Название.
    pub name: String,
    /// Адрес.
    pub url: String,
    /// Тип источника.
    pub kind: MediaKind,
}

/// Тип источника медиа.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// Плейлист M3U/M3U8.
    Playlist,
    /// Каталог DLNA.
    Dlna,
    /// Сетевая папка SMB.
    Smb,
    /// Локальный каталог или USB.
    Folder,
}

/// Все настройки NovaOS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Версия формата настроек.
    pub format: u32,
    /// Язык интерфейса.
    pub language: Lang,
    /// Тема.
    pub theme: Theme,
    /// Разрешение интерфейса.
    pub resolution: Resolution,
    /// Фоновое изображение.
    pub wallpaper: Wallpaper,
    /// Включены ли анимации.
    pub animations: bool,
    /// Режим кнопки Home.
    pub home_mode: HomeMode,
    /// Способ установки пакетов.
    pub install_backend: InstallBackend,
    /// Избранные приложения (порядок = порядок на главном экране).
    pub favorites: Vec<String>,
    /// Репозитории пакетов.
    pub repos: Vec<RepoRef>,
    /// Источники медиа.
    pub sources: Vec<MediaSource>,
    /// Показывать системные приложения LG.
    pub show_system_apps: bool,
    /// 24-часовой формат времени.
    pub clock_24h: bool,
    /// Заставка через N минут (0 — выключена).
    pub screensaver_minutes: u32,
    /// Проверять обновления NovaOS при запуске.
    pub auto_updates: bool,
    /// Рабочий каталог для загрузки пакетов.
    pub work_dir: Option<String>,
    /// Сбор статистики: всегда выключен, NovaOS никуда не сообщает данные.
    pub telemetry: bool,
    /// Последняя открытая вкладка главного экрана.
    pub last_screen: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format: 1,
            language: Lang::Ru,
            theme: Theme::Nova,
            resolution: Resolution::Full,
            wallpaper: Wallpaper::default(),
            animations: true,
            home_mode: HomeMode::Disabled,
            install_backend: InstallBackend::Auto,
            favorites: Vec::new(),
            repos: default_repos(),
            sources: Vec::new(),
            show_system_apps: false,
            clock_24h: true,
            screensaver_minutes: 0,
            auto_updates: true,
            work_dir: None,
            telemetry: false,
            last_screen: None,
        }
    }
}

/// Репозитории по умолчанию.
pub fn default_repos() -> Vec<RepoRef> {
    vec![
        RepoRef {
            title: "webOS Homebrew".into(),
            url: "https://repo.webosbrew.org/api/apps.json".into(),
            enabled: true,
            trusted: true,
            last_sync: None,
        },
        RepoRef {
            title: "Luma (пример стороннего репозитория)".into(),
            url: "https://github.com/ecklf/lg-luma/releases/latest/download/repo.json".into(),
            enabled: false,
            trusted: false,
            last_sync: None,
        },
    ]
}

/// Каталог конфигурации NovaOS.
///
/// Порядок поиска: `NOVA_HOME` (разработка/тесты) → `/var/lib/novaos` (root на ТВ)
/// → каталог приложения (обычная установка) → `/tmp/novaos`.
pub fn config_dir() -> PathBuf {
    if let Ok(value) = std::env::var("NOVA_HOME") {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    let system = PathBuf::from("/var/lib/novaos");
    if writable(&system) {
        return system;
    }
    if let Some(app_dir) = app_dir() {
        let candidate = app_dir.join("user");
        if writable(&candidate) {
            return candidate;
        }
    }
    PathBuf::from("/tmp/novaos")
}

/// Каталог, из которого запущен исполняемый файл (на ТВ — каталог приложения).
pub fn app_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent().map(Path::to_path_buf)
}

fn writable(path: &Path) -> bool {
    if let Ok(()) = std::fs::create_dir_all(path) {
        let probe = path.join(".nova-write-probe");
        if std::fs::write(&probe, b"ok").is_ok() {
            let _ = std::fs::remove_file(&probe);
            return true;
        }
    }
    false
}

/// Путь к файлу настроек.
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

impl Settings {
    /// Загружает настройки из файла; при отсутствии или ошибке — значения по умолчанию.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(mut settings) => {
                    settings.migrate();
                    settings
                }
                Err(_) => Self::default(),
            },
            Err(_) => Self::default(),
        }
    }

    /// Загружает настройки из стандартного пути.
    pub fn load_default() -> Self {
        Self::load(&settings_path())
    }

    /// Сохраняет настройки атомарно.
    pub fn save(&self, path: &Path) -> crate::NovaResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Сохраняет в стандартный путь.
    pub fn save_default(&self) -> crate::NovaResult<()> {
        self.save(&settings_path())
    }

    /// Приводит старые настройки к текущему формату.
    fn migrate(&mut self) {
        if self.format == 0 {
            self.format = 1;
        }
        if self.repos.is_empty() {
            self.repos = default_repos();
        }
        // Телеметрия в NovaOS невозможна по замыслу.
        self.telemetry = false;
    }

    /// Включённые репозитории.
    pub fn enabled_repos(&self) -> Vec<&RepoRef> {
        self.repos.iter().filter(|repo| repo.enabled).collect()
    }

    /// Добавляет избранный идентификатор приложения в конец списка.
    pub fn add_favorite(&mut self, id: &str) {
        if !self.favorites.iter().any(|existing| existing == id) {
            self.favorites.push(id.to_string());
        }
    }

    /// Убирает приложение из избранного.
    pub fn remove_favorite(&mut self, id: &str) {
        self.favorites.retain(|existing| existing != id);
    }

    /// Добавляет репозиторий (без дублей по URL).
    pub fn add_repo(&mut self, repo: RepoRef) -> bool {
        if self.repos.iter().any(|existing| existing.url == repo.url) {
            return false;
        }
        self.repos.push(repo);
        true
    }

    /// Рабочий каталог для загрузок (по умолчанию `/tmp/novaos`).
    pub fn work_dir(&self) -> PathBuf {
        self.work_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/novaos"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let settings = Settings::default();
        assert_eq!(settings.language, Lang::Ru);
        assert_eq!(settings.theme, Theme::Nova);
        assert_eq!(settings.resolution.size(), (1920, 1080));
        assert!(!settings.telemetry);
        assert!(!settings.repos.is_empty());
        assert!(settings.enabled_repos().iter().any(|r| r.trusted));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("nova-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        let mut settings = Settings::default();
        settings.language = Lang::En;
        settings.theme = Theme::Midnight;
        settings.resolution = Resolution::Fast;
        settings.home_mode = HomeMode::Replaced;
        settings.add_favorite("com.novaos.store");
        settings.add_favorite("com.novaos.store"); // дубль игнорируется
        settings.add_repo(RepoRef::new(
            "Мой репозиторий",
            "https://example.com/repo.json",
        ));
        settings.sources.push(MediaSource {
            name: "Домашний IPTV".into(),
            url: "http://192.168.1.10/playlist.m3u8".into(),
            kind: MediaKind::Playlist,
        });
        settings.save(&path).unwrap();

        let loaded = Settings::load(&path);
        assert_eq!(loaded, settings);
        assert_eq!(loaded.favorites.len(), 1);
        assert_eq!(loaded.repos.len(), 3);
        assert_eq!(loaded.sources[0].kind, MediaKind::Playlist);

        settings.remove_favorite("com.novaos.store");
        assert!(settings.favorites.is_empty());
        assert!(!settings.add_repo(RepoRef::new("дубль", "https://example.com/repo.json")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn broken_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("nova-settings-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ это не json").unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded, Settings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_fields_are_ignored_and_missing_filled() {
        let dir =
            std::env::temp_dir().join(format!("nova-settings-partial-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"language":"en","unknownField":42}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.language, Lang::En);
        assert_eq!(loaded.theme, Theme::Nova);
        assert!(
            !loaded.repos.is_empty(),
            "репозитории должны дополняться значениями по умолчанию"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

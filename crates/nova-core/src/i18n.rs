//! Локализация интерфейса NovaOS.
//!
//! Основной язык — русский, английский добавлен для симметрии с экосистемой webOS.
//! Переводы компилируются в двоичный файл: на телевизоре нет ни файлов локализации,
//! ни сети, поэтому строки всегда доступны.

use serde::{Deserialize, Serialize};

/// Язык интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    /// Русский.
    #[default]
    Ru,
    /// Английский.
    En,
}

impl Lang {
    /// Все поддерживаемые языки (в порядке показа в настройках).
    pub const ALL: [Lang; 2] = [Lang::Ru, Lang::En];

    /// Название языка на нём самом.
    pub fn label(&self) -> &'static str {
        match self {
            Lang::Ru => "Русский",
            Lang::En => "English",
        }
    }

    /// Код языка (`ru`, `en`).
    pub fn code(&self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }

    /// Определяет язык по переменной окружения (`LANG`, `LC_ALL`).
    pub fn from_env() -> Lang {
        for var in ["NOVA_LANG", "LC_ALL", "LANG"] {
            if let Ok(value) = std::env::var(var) {
                let lower = value.to_ascii_lowercase();
                if lower.starts_with("ru") {
                    return Lang::Ru;
                }
                if lower.starts_with("en") {
                    return Lang::En;
                }
            }
        }
        Lang::Ru
    }
}

/// Ключи строк интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    // Разделы
    Home,
    Apps,
    Store,
    Install,
    Settings,
    About,
    Sources,
    // Главный экран
    Favorites,
    Continue,
    QuickActions,
    AllApps,
    SystemApps,
    UserApps,
    LiveTv,
    Inputs,
    PowerOff,
    Reboot,
    SleepTimer,
    Welcome,
    WelcomeHint,
    // Приложения
    Launch,
    Close,
    Uninstall,
    AppDetails,
    NoApps,
    NoAppsHint,
    Refresh,
    Running,
    NotRunning,
    Vendor,
    Version,
    AppType,
    Path,
    Size,
    Installed,
    // Магазин
    Repositories,
    AddRepository,
    RepositoryUrl,
    Updates,
    InstallAction,
    UpdateAction,
    Reinstall,
    PackageSize,
    Checksum,
    Description,
    NoNetwork,
    NoNetworkHint,
    Loading,
    EmptyStore,
    // Установка своих приложений
    InstallFromUrl,
    InstallFromUsb,
    InstallFromFile,
    InstallFromQr,
    EnterUrl,
    QrHint,
    Scanning,
    DetectedFormat,
    SupportedFormats,
    ConvertToWebApp,
    Downloading,
    Verifying,
    Installing,
    Finished,
    Failed,
    Progress,
    Cancel,
    Retry,
    InstallAnother,
    UsbEmpty,
    // Настройки
    Language,
    Theme,
    Wallpaper,
    Resolution,
    Animations,
    HomeMode,
    HomeEnable,
    HomeDisable,
    HomeRestore,
    HomeEnabled,
    HomeDisabled,
    Diagnostics,
    Storage,
    Network,
    SystemInfo,
    Model,
    WebOsVersion,
    RootAccess,
    HomebrewChannel,
    NovaVersion,
    CheckUpdates,
    AutoUpdates,
    Screensaver,
    ShowSystemApps,
    Clock24h,
    Telemetry,
    TelemetryHint,
    // Диалоги и клавиатура
    Confirm,
    Yes,
    No,
    Ok,
    Back,
    CloseAction,
    UninstallConfirm,
    UninstallWarning,
    RebootWarning,
    HomeWarning,
    Keyboard,
    KeyboardHint,
    Clear,
    Space,
    Done,
    // Состояния и сообщения
    RootRequired,
    DesktopOnly,
    DemoMode,
    Error,
    Success,
    Warning,
    Empty,
    NothingFound,
    AppsCount,
    FreeSpace,
    Ready,
}

impl Key {
    /// Русский текст.
    pub fn ru(&self) -> &'static str {
        match self {
            Key::Home => "Главная",
            Key::Apps => "Приложения",
            Key::Store => "Магазин",
            Key::Install => "Установка",
            Key::Settings => "Настройки",
            Key::About => "О системе",
            Key::Sources => "Источники",
            Key::Favorites => "Избранное",
            Key::Continue => "Продолжить просмотр",
            Key::QuickActions => "Быстрые действия",
            Key::AllApps => "Все приложения",
            Key::SystemApps => "Системные приложения",
            Key::UserApps => "Мои приложения",
            Key::LiveTv => "ТВ-каналы",
            Key::Inputs => "Источники сигнала",
            Key::PowerOff => "Выключить",
            Key::Reboot => "Перезагрузка",
            Key::SleepTimer => "Таймер сна",
            Key::Welcome => "NovaOS",
            Key::WelcomeHint => "Своя операционная оболочка для LG webOS",
            Key::Launch => "Открыть",
            Key::Close => "Закрыть",
            Key::Uninstall => "Удалить",
            Key::AppDetails => "О приложении",
            Key::NoApps => "Приложений пока нет",
            Key::NoAppsHint => "Установите первое приложение в разделе «Магазин» или «Установка»",
            Key::Refresh => "Обновить",
            Key::Running => "Запущено",
            Key::NotRunning => "Остановлено",
            Key::Vendor => "Издатель",
            Key::Version => "Версия",
            Key::AppType => "Тип",
            Key::Path => "Каталог",
            Key::Size => "Размер",
            Key::Installed => "Установленные",
            Key::Repositories => "Репозитории",
            Key::AddRepository => "Добавить репозиторий",
            Key::RepositoryUrl => "Адрес репозитория (repo.json)",
            Key::Updates => "Обновления",
            Key::InstallAction => "Установить",
            Key::UpdateAction => "Обновить",
            Key::Reinstall => "Переустановить",
            Key::PackageSize => "Размер пакета",
            Key::Checksum => "Контрольная сумма SHA-256",
            Key::Description => "Описание",
            Key::NoNetwork => "Нет соединения",
            Key::NoNetworkHint => "Проверьте подключение телевизора к сети и повторите попытку",
            Key::Loading => "Загрузка…",
            Key::EmptyStore => "В репозитории пусто",
            Key::InstallFromUrl => "По ссылке (URL)",
            Key::InstallFromUsb => "С USB-накопителя",
            Key::InstallFromFile => "Из файла",
            Key::InstallFromQr => "QR-код для телефона",
            Key::EnterUrl => "Введите адрес пакета",
            Key::QrHint => "Наведите камеру телефона, чтобы отправить ссылку на телевизор",
            Key::Scanning => "Поиск устройств и файлов…",
            Key::DetectedFormat => "Определённый формат",
            Key::SupportedFormats => "Поддерживаемые форматы",
            Key::ConvertToWebApp => "Упаковать в приложение webOS",
            Key::Downloading => "Загрузка",
            Key::Verifying => "Проверка целостности",
            Key::Installing => "Установка",
            Key::Finished => "Готово",
            Key::Failed => "Ошибка",
            Key::Progress => "Выполнено",
            Key::Cancel => "Отмена",
            Key::Retry => "Повторить",
            Key::InstallAnother => "Установить ещё",
            Key::UsbEmpty => "USB-накопитель не найден",
            Key::Language => "Язык интерфейса",
            Key::Theme => "Тема оформления",
            Key::Wallpaper => "Фоновое изображение",
            Key::Resolution => "Разрешение интерфейса",
            Key::Animations => "Анимации",
            Key::HomeMode => "Режим кнопки Home",
            Key::HomeEnable => "Сделать NovaOS главным экраном",
            Key::HomeDisable => "Отключить подмену Home",
            Key::HomeRestore => "Вернуть оригинальный Home LG",
            Key::HomeEnabled => "NovaOS — главный экран системы",
            Key::HomeDisabled => "Главный экран — оригинальный LG Home",
            Key::Diagnostics => "Диагностика",
            Key::Storage => "Память",
            Key::Network => "Сеть",
            Key::SystemInfo => "Сведения об устройстве",
            Key::Model => "Модель",
            Key::WebOsVersion => "Версия webOS",
            Key::RootAccess => "Root-доступ",
            Key::HomebrewChannel => "Homebrew Channel",
            Key::NovaVersion => "Версия NovaOS",
            Key::CheckUpdates => "Проверить обновления",
            Key::AutoUpdates => "Автоматические обновления",
            Key::Screensaver => "Заставка через (мин)",
            Key::ShowSystemApps => "Показывать системные приложения",
            Key::Clock24h => "24-часовой формат времени",
            Key::Telemetry => "Сбор статистики",
            Key::TelemetryHint => "NovaOS не отправляет данные: всё работает локально",
            Key::Confirm => "Подтверждение",
            Key::Yes => "Да",
            Key::No => "Нет",
            Key::Ok => "ОК",
            Key::Back => "Назад",
            Key::CloseAction => "Закрыть",
            Key::UninstallConfirm => "Удалить приложение?",
            Key::UninstallWarning => "Данные приложения будут удалены без возможности восстановления.",
            Key::RebootWarning => "Перезагрузить телевизор сейчас?",
            Key::HomeWarning => "Изменение главного экрана влияет на загрузку системы. Оригинальный Home LG останется на диске и вернётся одной командой.",
            Key::Keyboard => "Экранная клавиатура",
            Key::KeyboardHint => "Стрелки — выбор, ОК — символ, Назад — удаление",
            Key::Clear => "Очистить",
            Key::Space => "Пробел",
            Key::Done => "Готово",
            Key::RootRequired => "Нужен root-доступ",
            Key::DesktopOnly => "Недоступно в desktop-превью",
            Key::DemoMode => "Демонстрационный режим",
            Key::Error => "Ошибка",
            Key::Success => "Выполнено",
            Key::Warning => "Внимание",
            Key::Empty => "Пусто",
            Key::NothingFound => "Ничего не найдено",
            Key::AppsCount => "Приложений",
            Key::FreeSpace => "Свободно",
            Key::Ready => "Готово к работе",
        }
    }

    /// Английский текст.
    pub fn en(&self) -> &'static str {
        match self {
            Key::Home => "Home",
            Key::Apps => "Apps",
            Key::Store => "Store",
            Key::Install => "Install",
            Key::Settings => "Settings",
            Key::About => "About",
            Key::Sources => "Sources",
            Key::Favorites => "Favorites",
            Key::Continue => "Continue watching",
            Key::QuickActions => "Quick actions",
            Key::AllApps => "All apps",
            Key::SystemApps => "System apps",
            Key::UserApps => "My apps",
            Key::LiveTv => "Live TV",
            Key::Inputs => "Inputs",
            Key::PowerOff => "Power off",
            Key::Reboot => "Reboot",
            Key::SleepTimer => "Sleep timer",
            Key::Welcome => "NovaOS",
            Key::WelcomeHint => "A custom system shell for LG webOS",
            Key::Launch => "Open",
            Key::Close => "Close",
            Key::Uninstall => "Uninstall",
            Key::AppDetails => "App details",
            Key::NoApps => "No apps yet",
            Key::NoAppsHint => "Install your first app from the Store or Install screens",
            Key::Refresh => "Refresh",
            Key::Running => "Running",
            Key::NotRunning => "Stopped",
            Key::Vendor => "Vendor",
            Key::Version => "Version",
            Key::AppType => "Type",
            Key::Path => "Folder",
            Key::Size => "Size",
            Key::Installed => "Installed",
            Key::Repositories => "Repositories",
            Key::AddRepository => "Add repository",
            Key::RepositoryUrl => "Repository URL (repo.json)",
            Key::Updates => "Updates",
            Key::InstallAction => "Install",
            Key::UpdateAction => "Update",
            Key::Reinstall => "Reinstall",
            Key::PackageSize => "Package size",
            Key::Checksum => "SHA-256 checksum",
            Key::Description => "Description",
            Key::NoNetwork => "No connection",
            Key::NoNetworkHint => "Check the TV network connection and try again",
            Key::Loading => "Loading…",
            Key::EmptyStore => "The repository is empty",
            Key::InstallFromUrl => "From URL",
            Key::InstallFromUsb => "From USB drive",
            Key::InstallFromFile => "From file",
            Key::InstallFromQr => "QR code for your phone",
            Key::EnterUrl => "Enter the package URL",
            Key::QrHint => "Point your phone camera to send the link to the TV",
            Key::Scanning => "Scanning devices and files…",
            Key::DetectedFormat => "Detected format",
            Key::SupportedFormats => "Supported formats",
            Key::ConvertToWebApp => "Package as a webOS app",
            Key::Downloading => "Downloading",
            Key::Verifying => "Verifying",
            Key::Installing => "Installing",
            Key::Finished => "Done",
            Key::Failed => "Failed",
            Key::Progress => "Progress",
            Key::Cancel => "Cancel",
            Key::Retry => "Retry",
            Key::InstallAnother => "Install another",
            Key::UsbEmpty => "No USB drive found",
            Key::Language => "Language",
            Key::Theme => "Theme",
            Key::Wallpaper => "Wallpaper",
            Key::Resolution => "Interface resolution",
            Key::Animations => "Animations",
            Key::HomeMode => "Home button mode",
            Key::HomeEnable => "Make NovaOS the Home screen",
            Key::HomeDisable => "Disable Home takeover",
            Key::HomeRestore => "Restore the original LG Home",
            Key::HomeEnabled => "NovaOS is the system Home screen",
            Key::HomeDisabled => "The original LG Home is active",
            Key::Diagnostics => "Diagnostics",
            Key::Storage => "Storage",
            Key::Network => "Network",
            Key::SystemInfo => "Device information",
            Key::Model => "Model",
            Key::WebOsVersion => "webOS version",
            Key::RootAccess => "Root access",
            Key::HomebrewChannel => "Homebrew Channel",
            Key::NovaVersion => "NovaOS version",
            Key::CheckUpdates => "Check for updates",
            Key::AutoUpdates => "Automatic updates",
            Key::Screensaver => "Screensaver after (min)",
            Key::ShowSystemApps => "Show system apps",
            Key::Clock24h => "24-hour clock",
            Key::Telemetry => "Telemetry",
            Key::TelemetryHint => "NovaOS sends nothing: everything stays local",
            Key::Confirm => "Confirm",
            Key::Yes => "Yes",
            Key::No => "No",
            Key::Ok => "OK",
            Key::Back => "Back",
            Key::CloseAction => "Close",
            Key::UninstallConfirm => "Uninstall this app?",
            Key::UninstallWarning => "App data will be removed permanently.",
            Key::RebootWarning => "Reboot the TV now?",
            Key::HomeWarning => "Changing the Home screen affects boot. The original LG Home stays on disk and can be restored with one command.",
            Key::Keyboard => "On-screen keyboard",
            Key::KeyboardHint => "Arrows select, OK types, Back deletes",
            Key::Clear => "Clear",
            Key::Space => "Space",
            Key::Done => "Done",
            Key::RootRequired => "Root access required",
            Key::DesktopOnly => "Not available in the desktop preview",
            Key::DemoMode => "Demo mode",
            Key::Error => "Error",
            Key::Success => "Done",
            Key::Warning => "Warning",
            Key::Empty => "Empty",
            Key::NothingFound => "Nothing found",
            Key::AppsCount => "Apps",
            Key::FreeSpace => "Free",
            Key::Ready => "Ready",
        }
    }
}

/// Возвращает строку для языка.
pub fn t(lang: Lang, key: Key) -> &'static str {
    match lang {
        Lang::Ru => key.ru(),
        Lang::En => key.en(),
    }
}

/// Русская плюрализация: `plural(2, "приложение", "приложения", "приложений")`.
pub fn plural(count: u64, one: &str, few: &str, many: &str) -> String {
    let mod10 = count % 10;
    let mod100 = count % 100;
    let word = if mod10 == 1 && mod100 != 11 {
        one
    } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
        few
    } else {
        many
    };
    format!("{count} {word}")
}

/// Плюрализация с учётом языка.
pub fn plural_lang(lang: Lang, count: u64, ru: (&str, &str, &str), en: (&str, &str)) -> String {
    match lang {
        Lang::Ru => plural(count, ru.0, ru.1, ru.2),
        Lang::En => {
            let word = if count == 1 { en.0 } else { en.1 };
            format!("{count} {word}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_translated() {
        // Полный перечень вариантов: если появится новый ключ без перевода,
        // тест упадёт на проверке длины совпадающих строк.
        let keys = all_keys();
        assert!(
            keys.len() > 100,
            "ожидалось больше 100 ключей, получено {}",
            keys.len()
        );
        for key in keys {
            assert!(!key.ru().is_empty(), "пустой русский текст у {:?}", key);
            assert!(!key.en().is_empty(), "пустой английский текст у {:?}", key);
        }
    }

    #[test]
    fn russian_and_english_differ_for_labels() {
        // Совпадение допустимо для имён собственных (NovaOS, Homebrew Channel).
        let allowed = ["NovaOS", "OK", "QR", "SHA-256", "Homebrew Channel"];
        let mut same = Vec::new();
        for key in all_keys() {
            if key.ru() == key.en() && !allowed.contains(&key.ru()) {
                same.push(key.ru());
            }
        }
        assert!(same.is_empty(), "не переведены: {same:?}");
    }

    #[test]
    fn lang_basics() {
        assert_eq!(Lang::default(), Lang::Ru);
        assert_eq!(Lang::Ru.code(), "ru");
        assert_eq!(Lang::En.label(), "English");
        assert_eq!(t(Lang::Ru, Key::Home), "Главная");
        assert_eq!(t(Lang::En, Key::Home), "Home");
        assert_eq!(Lang::ALL.len(), 2);
    }

    #[test]
    fn russian_plural_forms() {
        assert_eq!(
            plural(1, "приложение", "приложения", "приложений"),
            "1 приложение"
        );
        assert_eq!(
            plural(2, "приложение", "приложения", "приложений"),
            "2 приложения"
        );
        assert_eq!(
            plural(5, "приложение", "приложения", "приложений"),
            "5 приложений"
        );
        assert_eq!(
            plural(11, "приложение", "приложения", "приложений"),
            "11 приложений"
        );
        assert_eq!(
            plural(21, "приложение", "приложения", "приложений"),
            "21 приложение"
        );
        assert_eq!(
            plural_lang(Lang::En, 1, ("x", "y", "z"), ("app", "apps")),
            "1 app"
        );
        assert_eq!(
            plural_lang(Lang::En, 3, ("x", "y", "z"), ("app", "apps")),
            "3 apps"
        );
    }

    /// Перечень всех ключей (для тестов полноты перевода).
    fn all_keys() -> Vec<Key> {
        vec![
            Key::Home,
            Key::Apps,
            Key::Store,
            Key::Install,
            Key::Settings,
            Key::About,
            Key::Sources,
            Key::Favorites,
            Key::Continue,
            Key::QuickActions,
            Key::AllApps,
            Key::SystemApps,
            Key::UserApps,
            Key::LiveTv,
            Key::Inputs,
            Key::PowerOff,
            Key::Reboot,
            Key::SleepTimer,
            Key::Welcome,
            Key::WelcomeHint,
            Key::Launch,
            Key::Close,
            Key::Uninstall,
            Key::AppDetails,
            Key::NoApps,
            Key::NoAppsHint,
            Key::Refresh,
            Key::Running,
            Key::NotRunning,
            Key::Vendor,
            Key::Version,
            Key::AppType,
            Key::Path,
            Key::Size,
            Key::Installed,
            Key::Repositories,
            Key::AddRepository,
            Key::RepositoryUrl,
            Key::Updates,
            Key::InstallAction,
            Key::UpdateAction,
            Key::Reinstall,
            Key::PackageSize,
            Key::Checksum,
            Key::Description,
            Key::NoNetwork,
            Key::NoNetworkHint,
            Key::Loading,
            Key::EmptyStore,
            Key::InstallFromUrl,
            Key::InstallFromUsb,
            Key::InstallFromFile,
            Key::InstallFromQr,
            Key::EnterUrl,
            Key::QrHint,
            Key::Scanning,
            Key::DetectedFormat,
            Key::SupportedFormats,
            Key::ConvertToWebApp,
            Key::Downloading,
            Key::Verifying,
            Key::Installing,
            Key::Finished,
            Key::Failed,
            Key::Progress,
            Key::Cancel,
            Key::Retry,
            Key::InstallAnother,
            Key::UsbEmpty,
            Key::Language,
            Key::Theme,
            Key::Wallpaper,
            Key::Resolution,
            Key::Animations,
            Key::HomeMode,
            Key::HomeEnable,
            Key::HomeDisable,
            Key::HomeRestore,
            Key::HomeEnabled,
            Key::HomeDisabled,
            Key::Diagnostics,
            Key::Storage,
            Key::Network,
            Key::SystemInfo,
            Key::Model,
            Key::WebOsVersion,
            Key::RootAccess,
            Key::HomebrewChannel,
            Key::NovaVersion,
            Key::CheckUpdates,
            Key::AutoUpdates,
            Key::Screensaver,
            Key::ShowSystemApps,
            Key::Clock24h,
            Key::Telemetry,
            Key::TelemetryHint,
            Key::Confirm,
            Key::Yes,
            Key::No,
            Key::Ok,
            Key::Back,
            Key::CloseAction,
            Key::UninstallConfirm,
            Key::UninstallWarning,
            Key::RebootWarning,
            Key::HomeWarning,
            Key::Keyboard,
            Key::KeyboardHint,
            Key::Clear,
            Key::Space,
            Key::Done,
            Key::RootRequired,
            Key::DesktopOnly,
            Key::DemoMode,
            Key::Error,
            Key::Success,
            Key::Warning,
            Key::Empty,
            Key::NothingFound,
            Key::AppsCount,
            Key::FreeSpace,
            Key::Ready,
        ]
    }
}

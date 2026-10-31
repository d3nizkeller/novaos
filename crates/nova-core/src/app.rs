//! Модель приложения webOS.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Содержимое `appinfo.json` — манифеста приложения webOS.
///
/// Поля описаны по реальным пакетам (Luma, Homebrew Channel, штатные приложения LG).
/// Неизвестные поля игнорируются, отсутствующие принимают значение по умолчанию,
/// поэтому разбор не ломается на старых версиях webOS.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebOsAppInfo {
    /// Уникальный идентификатор (`com.novaos.launcher`).
    pub id: String,
    /// Версия приложения.
    pub version: Option<String>,
    /// Издатель.
    pub vendor: Option<String>,
    /// Тип: `native`, `web`, `stub`, `media`.
    #[serde(rename = "type")]
    pub app_type: Option<String>,
    /// Точка входа: исполняемый файл (`nova`) или `index.html`.
    pub main: Option<String>,
    /// Название в интерфейсе.
    pub title: Option<String>,
    /// Иконка (путь относительно каталога приложения).
    pub icon: Option<String>,
    /// Крупная иконка.
    pub large_icon: Option<String>,
    /// Описание.
    pub app_description: Option<String>,
    /// Поддерживаемое разрешение (`1920x1080`).
    pub resolution: Option<String>,
    /// Категории (`["home"]`).
    pub app_category: Vec<String>,
    /// Требуемые группы доступа Luna.
    #[serde(rename = "requiredACG")]
    pub required_acg: Vec<String>,
    /// Требуемые разрешения.
    pub required_permissions: Vec<String>,
    /// Цвет иконки/фона.
    pub icon_color: Option<String>,
    /// Цвет фона.
    pub bg_color: Option<String>,
    /// Фоновое изображение заставки.
    pub splash_background: Option<String>,
    /// Каталог приложения на устройстве (заполняется системой).
    pub folder_path: Option<String>,
    /// Показывать ли приложение в списке.
    pub visible: Option<bool>,
    /// Не гасить экран во время работы.
    pub disable_screen_timeout: Option<bool>,
    /// Обрабатывает ли приложение повторный запуск само.
    pub handles_relaunch: Option<bool>,
    /// Дополнительные параметры запуска по умолчанию.
    pub params: Option<serde_json::Value>,
    /// Всё, что не описано выше (сохраняется как есть).
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

impl WebOsAppInfo {
    /// Разбирает `appinfo.json`.
    pub fn parse(text: &str) -> crate::NovaResult<Self> {
        Ok(serde_json::from_str(text)?)
    }

    /// Название для интерфейса (с резервом на идентификатор).
    pub fn display_title(&self) -> String {
        self.title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| self.id.clone())
    }

    /// Тип приложения с значением по умолчанию.
    pub fn kind(&self) -> AppKind {
        match self.app_type.as_deref().unwrap_or("web") {
            "native" => AppKind::Native,
            "stub" => AppKind::Stub,
            "media" => AppKind::Media,
            _ => AppKind::Web,
        }
    }

    /// Является ли приложение системным (штатным для LG).
    pub fn is_system_id(&self) -> bool {
        self.id.starts_with("com.webos.")
            || self.id.starts_with("com.palm.")
            || self.id.starts_with("com.lge.")
            || self.id.starts_with("com.lg.")
    }
}

/// Тип приложения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    /// Нативное ARM-приложение (как NovaOS или Luma).
    Native,
    /// Веб-приложение (HTML/JS в WebKit).
    Web,
    /// Заглушка (ссылка на внешний источник).
    Stub,
    /// Медиа-приложение.
    Media,
}

impl AppKind {
    /// Русское название типа.
    pub fn label_ru(&self) -> &'static str {
        match self {
            AppKind::Native => "Нативное",
            AppKind::Web => "Веб",
            AppKind::Stub => "Ссылка",
            AppKind::Media => "Медиа",
        }
    }
}

/// Источник, из которого приложение попало на телевизор.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppSource {
    /// Штатная прошивка (`/usr/palm/applications`).
    System,
    /// Установлено пользователем (`/media/cryptofs/apps`).
    User,
    /// Установлено в режиме разработчика (`/media/developer/apps`).
    Developer,
    /// Из репозитория NovaOS/Homebrew.
    Repository,
}

impl AppSource {
    /// Русское название источника.
    pub fn label_ru(&self) -> &'static str {
        match self {
            AppSource::System => "Система",
            AppSource::User => "Установлено",
            AppSource::Developer => "Режим разработчика",
            AppSource::Repository => "Репозиторий",
        }
    }
}

/// Установленное приложение в каталоге NovaOS.
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledApp {
    /// Манифест приложения.
    pub info: WebOsAppInfo,
    /// Каталог приложения на устройстве.
    pub path: PathBuf,
    /// Источник установки.
    pub source: AppSource,
    /// Размер каталога в байтах (если удалось посчитать).
    pub size_bytes: Option<u64>,
    /// Путь к файлу иконки (если существует).
    pub icon_path: Option<PathBuf>,
    /// Запущено ли приложение сейчас.
    pub running: bool,
    /// Порядок в избранном на главном экране (None — не в избранном).
    pub favorite_order: Option<usize>,
}

impl InstalledApp {
    /// Идентификатор приложения.
    pub fn id(&self) -> &str {
        &self.info.id
    }

    /// Отображаемое название.
    pub fn title(&self) -> String {
        self.info.display_title()
    }

    /// Можно ли его удалить (штатные приложения LG — нельзя).
    pub fn removable(&self) -> bool {
        !matches!(self.source, AppSource::System)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_native_appinfo() {
        let text = r##"{
            "id": "com.ecklf.luma",
            "version": "2.16.0",
            "vendor": "Luma",
            "type": "native",
            "main": "luma",
            "title": "Luma",
            "icon": "app-icon.png",
            "largeIcon": "app-icon-large.png",
            "appDescription": "A remote-first native webOS launcher.",
            "resolution": "1920x1080",
            "appCategory": ["home"],
            "requiredACG": ["application.launcher", "application.query", "lunabus.query"],
            "handlesRelaunch": false,
            "iconColor": "#25272d",
            "splashBackground": "splash.png",
            "bgColor": "#17191c"
        }"##;
        let info = WebOsAppInfo::parse(text).unwrap();
        assert_eq!(info.id, "com.ecklf.luma");
        assert_eq!(info.kind(), AppKind::Native);
        assert_eq!(info.large_icon.as_deref(), Some("app-icon-large.png"));
        assert_eq!(info.app_category, vec!["home".to_string()]);
        assert_eq!(info.required_acg.len(), 3);
        assert!(!info.is_system_id());
    }

    #[test]
    fn parses_minimal_web_appinfo_and_keeps_unknown_fields() {
        let text = r#"{"id":"org.webosbrew.hbchannel","title":"Homebrew Channel","main":"index.html","type":"web","previewMetadata":{"targetEndpoint":"luna://x"}}"#;
        let info = WebOsAppInfo::parse(text).unwrap();
        assert_eq!(info.kind(), AppKind::Web);
        assert!(info.extra.contains_key("previewMetadata"));
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("previewMetadata"));
    }

    #[test]
    fn detects_system_apps() {
        let mut info = WebOsAppInfo::default();
        info.id = "com.webos.app.home".into();
        assert!(info.is_system_id());
        info.id = "com.novaos.launcher".into();
        assert!(!info.is_system_id());
        assert_eq!(info.display_title(), "com.novaos.launcher");
    }
}

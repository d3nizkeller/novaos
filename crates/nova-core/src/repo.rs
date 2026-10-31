//! Репозитории пакетов (формат Homebrew Channel).
//!
//! Реальный `repo.json` выглядит так:
//!
//! ```json
//! {
//!   "paging": { "page": 1, "count": 56, "maxPage": 1, "itemsTotal": 56 },
//!   "packages": [{
//!     "id": "com.example.app",
//!     "title": "Название",
//!     "iconUri": "https://…/icon.png",
//!     "shortDescription": "…",
//!     "fullDescriptionUrl": "…",
//!     "manifestUrl": "https://…/manifest.json",
//!     "requirements": { "webosRelease": ">=4.0" },
//!     "manifest": {
//!       "id": "com.example.app", "version": "1.0.0", "type": "web",
//!       "ipkUrl": "https://…/app_1.0.0_all.ipk",
//!       "ipkHash": { "sha256": "…" }, "ipkSize": 930444,
//!       "rootRequired": false, "sourceUrl": "https://…"
//!     }
//!   }]
//! }
//! ```
//!
//! NovaOS понимает и этот формат, и простой список `{"apps": [...]}` — так можно
//! подключить самодельный репозиторий, собранный [`build_manifest`].

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};

/// Информация о постраничной выдаче.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Paging {
    /// Текущая страница.
    pub page: u32,
    /// Количество элементов на странице.
    pub count: u32,
    /// Всего страниц.
    pub max_page: u32,
    /// Всего элементов.
    pub items_total: u32,
    /// Адрес следующей страницы.
    pub next_url: Option<String>,
}

/// Контрольные суммы пакета.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IpkHash {
    /// SHA-256 пакета.
    pub sha256: String,
}

/// Манифест пакета внутри репозитория.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PackageManifest {
    /// Идентификатор приложения.
    pub id: String,
    /// Версия.
    pub version: String,
    /// Тип приложения (`web`, `native`).
    #[serde(rename = "type")]
    pub app_type: String,
    /// Название.
    pub title: String,
    /// Описание.
    pub app_description: String,
    /// Иконка.
    pub icon_uri: String,
    /// Страница проекта.
    pub source_url: String,
    /// Требуется ли root.
    pub root_required: bool,
    /// Адрес IPK.
    pub ipk_url: String,
    /// Контрольные суммы.
    pub ipk_hash: Option<IpkHash>,
    /// Размер IPK в байтах.
    pub ipk_size: Option<u64>,
    /// Размер после установки.
    pub installed_size: Option<u64>,
}

/// Требования к устройству.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Requirements {
    /// Требуемая версия webOS, например `>=4.0`.
    pub webos_release: Option<String>,
}

/// Элемент репозитория.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PackageEntry {
    /// Идентификатор.
    pub id: String,
    /// Название.
    pub title: String,
    /// Иконка.
    pub icon_uri: String,
    /// Крупная иконка.
    pub detail_icon_uri: Option<String>,
    /// Краткое описание.
    pub short_description: Option<String>,
    /// Адрес полного описания (HTML).
    pub full_description_url: Option<String>,
    /// Адрес отдельного манифеста.
    pub manifest_url: Option<String>,
    /// Манифест (может отсутствовать — тогда его грузят отдельно).
    pub manifest: Option<PackageManifest>,
    /// Пул репозитория (`main`, `non-free`).
    pub pool: Option<String>,
    /// Требования.
    pub requirements: Option<Requirements>,
    /// Скриншоты.
    pub screenshots: Vec<String>,
    /// Рекомендуемое приложение.
    pub featured: bool,
}

/// Манифест репозитория.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoManifest {
    /// Постраничная выдача.
    pub paging: Option<Paging>,
    /// Основной список пакетов.
    pub packages: Vec<PackageEntry>,
    /// Упрощённый список (совместимость со старыми репозиториями).
    pub apps: Vec<PackageEntry>,
}

impl RepoManifest {
    /// Все пакеты независимо от формата списка.
    pub fn entries(&self) -> Vec<&PackageEntry> {
        let mut out: Vec<&PackageEntry> = self.packages.iter().collect();
        out.extend(self.apps.iter());
        out
    }
}

/// Состояние пакета относительно установленного приложения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreState {
    /// Не установлено.
    NotInstalled,
    /// Доступно обновление.
    Update,
    /// Установлена актуальная версия.
    Installed,
    /// Установленная версия новее (например, своя сборка).
    Newer,
}

impl StoreState {
    /// Русское название состояния.
    pub fn label_ru(&self) -> &'static str {
        match self {
            StoreState::NotInstalled => "Не установлено",
            StoreState::Update => "Есть обновление",
            StoreState::Installed => "Установлено",
            StoreState::Newer => "Установлена более новая версия",
        }
    }
}

/// Плоское представление пакета для интерфейса магазина.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreItem {
    /// Идентификатор приложения.
    pub id: String,
    /// Название.
    pub title: String,
    /// Версия в репозитории.
    pub version: String,
    /// Тип приложения.
    pub app_type: String,
    /// Описание.
    pub description: String,
    /// Адрес иконки.
    pub icon_url: String,
    /// Адрес IPK.
    pub ipk_url: String,
    /// SHA-256 IPK (если опубликован).
    pub sha256: Option<String>,
    /// Размер IPK.
    pub size: Option<u64>,
    /// Страница проекта.
    pub source_url: String,
    /// Требуется ли root.
    pub root_required: bool,
    /// Требование к версии webOS.
    pub webos_requirement: Option<String>,
    /// Скриншоты.
    pub screenshots: Vec<String>,
    /// Рекомендуемое.
    pub featured: bool,
    /// Репозиторий-источник.
    pub repo_title: String,
}

impl StoreItem {
    /// Состояние относительно установленной версии.
    pub fn state(&self, installed_version: Option<&str>) -> StoreState {
        match installed_version {
            None => StoreState::NotInstalled,
            Some(current) => match compare_versions(current, &self.version) {
                Ordering::Less => StoreState::Update,
                Ordering::Equal => StoreState::Installed,
                Ordering::Greater => StoreState::Newer,
            },
        }
    }

    /// Подходит ли пакет для указанной версии webOS.
    pub fn supports_webos(&self, release: Option<&str>) -> bool {
        match (&self.webos_requirement, release) {
            (Some(requirement), Some(version)) => version_satisfies(version, requirement),
            (None, _) => true,
            // Требование есть, а версия устройства неизвестна — не блокируем установку.
            (Some(_), None) => true,
        }
    }
}

/// Разбирает JSON репозитория в плоский список.
pub fn parse(value: &serde_json::Value, repo_title: &str) -> NovaResult<Vec<StoreItem>> {
    let manifest: RepoManifest = serde_json::from_value(value.clone())
        .map_err(|e| NovaError::Package(format!("репозиторий не распознан: {e}")))?;
    Ok(manifest
        .entries()
        .into_iter()
        .filter_map(|entry| to_store_item(entry, repo_title))
        .collect())
}

fn to_store_item(entry: &PackageEntry, repo_title: &str) -> Option<StoreItem> {
    let manifest = entry.manifest.clone().unwrap_or_default();
    let id = if entry.id.is_empty() {
        manifest.id.clone()
    } else {
        entry.id.clone()
    };
    if id.is_empty() {
        return None;
    }
    let ipk_url = manifest.ipk_url.clone();
    if ipk_url.is_empty() {
        return None;
    }
    let title = if entry.title.is_empty() {
        if manifest.title.is_empty() {
            id.clone()
        } else {
            manifest.title.clone()
        }
    } else {
        entry.title.clone()
    };
    let description = entry
        .short_description
        .clone()
        .filter(|d| !d.is_empty())
        .or_else(|| {
            (!manifest.app_description.is_empty()).then(|| manifest.app_description.clone())
        })
        .unwrap_or_default();
    let icon_url = if entry.icon_uri.is_empty() {
        manifest.icon_uri.clone()
    } else {
        entry.icon_uri.clone()
    };
    Some(StoreItem {
        id,
        title,
        version: manifest.version.clone(),
        app_type: if manifest.app_type.is_empty() {
            "web".into()
        } else {
            manifest.app_type.clone()
        },
        description,
        icon_url,
        ipk_url,
        sha256: manifest
            .ipk_hash
            .as_ref()
            .map(|h| h.sha256.clone())
            .filter(|s| !s.is_empty()),
        size: manifest.ipk_size,
        source_url: manifest.source_url.clone(),
        root_required: manifest.root_required,
        webos_requirement: entry
            .requirements
            .as_ref()
            .and_then(|r| r.webos_release.clone()),
        screenshots: entry.screenshots.clone(),
        featured: entry.featured,
        repo_title: repo_title.to_string(),
    })
}

/// Загружает и разбирает репозиторий по URL.
pub fn fetch(url: &str, repo_title: &str) -> NovaResult<Vec<StoreItem>> {
    let value = crate::net::fetch_json(url)?;
    parse(&value, repo_title)
}

/// Собирает собственный `repo.json` из списка пакетов.
///
/// Используется утилитой `tools/repository.py`-эквивалентом и командой
/// `nova repo publish`, чтобы опубликовать свои приложения.
pub fn build_manifest(items: &[StoreItem]) -> String {
    let packages: Vec<PackageEntry> = items
        .iter()
        .map(|item| PackageEntry {
            id: item.id.clone(),
            title: item.title.clone(),
            icon_uri: item.icon_url.clone(),
            detail_icon_uri: None,
            short_description: Some(item.description.clone()),
            full_description_url: None,
            manifest_url: None,
            manifest: Some(PackageManifest {
                id: item.id.clone(),
                version: item.version.clone(),
                app_type: item.app_type.clone(),
                title: item.title.clone(),
                app_description: item.description.clone(),
                icon_uri: item.icon_url.clone(),
                source_url: item.source_url.clone(),
                root_required: item.root_required,
                ipk_url: item.ipk_url.clone(),
                ipk_hash: item.sha256.clone().map(|sha256| IpkHash { sha256 }),
                ipk_size: item.size,
                installed_size: None,
            }),
            pool: Some("main".into()),
            requirements: item
                .webos_requirement
                .clone()
                .map(|webos_release| Requirements {
                    webos_release: Some(webos_release),
                }),
            screenshots: item.screenshots.clone(),
            featured: item.featured,
        })
        .collect();
    let manifest = RepoManifest {
        paging: Some(Paging {
            page: 1,
            count: packages.len() as u32,
            max_page: 1,
            items_total: packages.len() as u32,
            next_url: None,
        }),
        packages,
        apps: Vec::new(),
    };
    serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".into())
}

/// Сравнивает версии в стиле semver (`1.2.3` < `1.10.0`, суффиксы игнорируются).
pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let parse = |text: &str| -> Vec<u64> {
        text.trim_start_matches('v')
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .next()
            .unwrap_or("")
            .split('.')
            .filter(|p| !p.is_empty())
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let left = parse(left);
    let right = parse(right);
    let len = left.len().max(right.len());
    for index in 0..len {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        match a.cmp(&b) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// Проверяет требование вида `>=4.0`, `<7`, `=5.2`.
pub fn version_satisfies(version: &str, requirement: &str) -> bool {
    let requirement = requirement.trim();
    let (op, rest) = if let Some(rest) = requirement.strip_prefix(">=") {
        (">=", rest)
    } else if let Some(rest) = requirement.strip_prefix("<=") {
        ("<=", rest)
    } else if let Some(rest) = requirement.strip_prefix('>') {
        (">", rest)
    } else if let Some(rest) = requirement.strip_prefix('<') {
        ("<", rest)
    } else if let Some(rest) = requirement.strip_prefix('=') {
        ("=", rest)
    } else {
        ("=", requirement)
    };
    match compare_versions(version, rest.trim()) {
        Ordering::Less => matches!(op, "<" | "<="),
        Ordering::Equal => matches!(op, "=" | ">=" | "<="),
        Ordering::Greater => matches!(op, ">" | ">="),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_REPO: &str = r#"{
      "paging": { "page": 1, "count": 2, "maxPage": 1, "itemsTotal": 2 },
      "packages": [
        {
          "id": "com.gabrielecirulli.app2048.webos",
          "title": "2048 Port for WebOS",
          "iconUri": "https://repo.webosbrew.org/apps/icons/2048.png",
          "manifestUrl": "https://example.com/manifest.json",
          "pool": "main",
          "requirements": { "webosRelease": ">=4.0" },
          "shortDescription": "Classic 2048 puzzle game ported for LG webOS TVs.",
          "fullDescriptionUrl": "apps/2048/full_description.html",
          "manifest": {
            "id": "com.gabrielecirulli.app2048.webos",
            "version": "1.0.0",
            "type": "web",
            "title": "2048 Port for WebOS",
            "appDescription": "Classic 2048 puzzle game ported for LG webOS TVs.",
            "iconUri": "https://repo.webosbrew.org/apps/icons/2048.png",
            "sourceUrl": "https://github.com/Kzuismee/2048-Port-for-WebOS",
            "rootRequired": false,
            "ipkUrl": "https://github.com/Kzuismee/2048-Port-for-WebOS/releases/download/1.0.0/app_all.ipk",
            "ipkHash": { "sha256": "814f4e95dd9917e240637df7bf91396498e00d92f2fd1f7993a27c3e0e90db19" },
            "ipkSize": 930444
          }
        },
        {
          "id": "com.ecklf.luma",
          "title": "Luma",
          "manifest": {
            "id": "com.ecklf.luma", "version": "2.16.0", "type": "native", "title": "Luma",
            "rootRequired": true,
            "ipkUrl": "https://github.com/ecklf/lg-luma/releases/download/v2.16.0/com.ecklf.luma_2.16.0_arm.ipk",
            "ipkHash": { "sha256": "98c6b94a683d2a17c3ba6bb424f06f4fff12e914b69635e6441a025358729274" },
            "ipkSize": 41386094
          }
        }
      ]
    }"#;

    #[test]
    fn parses_real_repository_format() {
        let value: serde_json::Value = serde_json::from_str(REAL_REPO).unwrap();
        let items = parse(&value, "webOS Homebrew").unwrap();
        assert_eq!(items.len(), 2);
        let game = &items[0];
        assert_eq!(game.id, "com.gabrielecirulli.app2048.webos");
        assert_eq!(game.version, "1.0.0");
        assert_eq!(game.app_type, "web");
        assert_eq!(game.size, Some(930444));
        assert_eq!(game.sha256.as_deref().map(str::len), Some(64));
        assert!(!game.root_required);
        assert_eq!(game.webos_requirement.as_deref(), Some(">=4.0"));
        assert!(game.supports_webos(Some("4.9.1")));
        assert!(!game.supports_webos(Some("3.0.5")));
        assert!(game.supports_webos(None));
        assert_eq!(items[1].app_type, "native");
        assert!(items[1].root_required);
        assert_eq!(game.repo_title, "webOS Homebrew");
    }

    #[test]
    fn parses_legacy_apps_array() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"apps":[{"id":"com.legacy.app","title":"Legacy","manifest":{"ipkUrl":"https://x/y.ipk","version":"0.2"}}]}"#,
        )
        .unwrap();
        let items = parse(&value, "legacy").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].version, "0.2");
        assert_eq!(items[0].title, "Legacy");
    }

    #[test]
    fn skips_entries_without_ipk() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"packages":[{"id":"x","title":"X"}]}"#).unwrap();
        assert!(parse(&value, "repo").unwrap().is_empty());
        assert!(parse(&serde_json::json!("строка вместо объекта"), "repo").is_err());
    }

    #[test]
    fn build_manifest_roundtrip() {
        let value: serde_json::Value = serde_json::from_str(REAL_REPO).unwrap();
        let items = parse(&value, "webOS Homebrew").unwrap();
        let generated = build_manifest(&items);
        let reparsed: serde_json::Value = serde_json::from_str(&generated).unwrap();
        let again = parse(&reparsed, "webOS Homebrew").unwrap();
        assert_eq!(again.len(), items.len());
        assert_eq!(again[0].id, items[0].id);
        assert_eq!(again[0].sha256, items[0].sha256);
        assert_eq!(again[1].version, "2.16.0");
        assert_eq!(reparsed["paging"]["itemsTotal"], 2);
    }

    #[test]
    fn version_comparison() {
        assert_eq!(compare_versions("1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(compare_versions("1.2.3", "1.10.0"), Ordering::Less);
        assert_eq!(compare_versions("v2.16.0", "2.16.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.0", "0.1.0-rc1"), Ordering::Equal);
        assert_eq!(compare_versions("4.9.1-70309", "4.0"), Ordering::Greater);
    }

    #[test]
    fn requirement_syntax() {
        assert!(version_satisfies("4.9.1", ">=4.0"));
        assert!(!version_satisfies("3.9", ">=4.0"));
        assert!(version_satisfies("6.3", "<7"));
        assert!(version_satisfies("5.2", "=5.2"));
        assert!(version_satisfies("5.2", "<=5.2"));
        assert!(version_satisfies("5.3", ">5.2"));
    }

    #[test]
    fn store_states() {
        let value: serde_json::Value = serde_json::from_str(REAL_REPO).unwrap();
        let items = parse(&value, "repo").unwrap();
        assert_eq!(items[1].state(None), StoreState::NotInstalled);
        assert_eq!(items[1].state(Some("2.15.0")), StoreState::Update);
        assert_eq!(items[1].state(Some("2.16.0")), StoreState::Installed);
        assert_eq!(items[1].state(Some("3.0.0")), StoreState::Newer);
        assert!(!StoreState::Update.label_ru().is_empty());
    }
}

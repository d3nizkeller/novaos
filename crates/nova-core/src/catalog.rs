//! Каталог приложений NovaOS.
//!
//! Список приложений собирается из двух источников и объединяется:
//!
//! 1. **Файловая система** — каталоги `appinfo.json`:
//!    * `/usr/palm/applications` (штатные приложения LG, только чтение);
//!    * `/media/cryptofs/apps/usr/palm/applications` (установленные пользователем);
//!    * `/media/developer/apps/usr/palm/applications` (режим разработчика).
//! 2. **Luna** — `applicationManager/dev/listApps` (даёт актуальное состояние
//!    и `folderPath` для приложений, чей каталог смонтирован иначе).
//!
//! Файловый источник работает даже тогда, когда шина недоступна, поэтому
//! лаунчер никогда не остаётся пустым.

use std::path::{Path, PathBuf};

use crate::app::{AppSource, InstalledApp, WebOsAppInfo};

/// Каталоги, которые сканирует NovaOS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanPaths {
    /// Штатные приложения прошивки.
    pub system: Vec<PathBuf>,
    /// Приложения, установленные пользователем (cryptofs).
    pub user: Vec<PathBuf>,
    /// Приложения режима разработчика.
    pub developer: Vec<PathBuf>,
}

impl Default for ScanPaths {
    /// Типовые пути webOS TV.
    fn default() -> Self {
        Self {
            system: vec![PathBuf::from("/usr/palm/applications")],
            user: vec![PathBuf::from("/media/cryptofs/apps/usr/palm/applications")],
            developer: vec![PathBuf::from("/media/developer/apps/usr/palm/applications")],
        }
    }
}

impl ScanPaths {
    /// Пути для тестов и desktop-превью.
    pub fn for_root(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            system: vec![root.join("usr/palm/applications")],
            user: vec![root.join("media/cryptofs/apps/usr/palm/applications")],
            developer: vec![root.join("media/developer/apps/usr/palm/applications")],
        }
    }
}

/// Параметры сканирования.
#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    /// Считать размер каталогов (медленно на флеше ТВ — по умолчанию выключено).
    pub measure_size: bool,
    /// Включать штатные приложения LG в каталог.
    pub include_system: bool,
    /// Скрывать приложения без иконки/названия.
    pub hide_broken: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            measure_size: false,
            include_system: true,
            hide_broken: false,
        }
    }
}

/// Каталог установленных приложений.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    /// Приложения (отсортированы по названию).
    pub apps: Vec<InstalledApp>,
    /// Ошибки сканирования (показываются в «Настройки → Система → Диагностика»).
    pub errors: Vec<String>,
}

impl Catalog {
    /// Сканирует файловую систему.
    pub fn scan(paths: &ScanPaths, options: ScanOptions) -> Self {
        let mut catalog = Self::default();
        let groups: Vec<(AppSource, &[PathBuf])> = vec![
            (AppSource::System, &paths.system),
            (AppSource::User, &paths.user),
            (AppSource::Developer, &paths.developer),
        ];
        for (source, roots) in groups {
            if source == AppSource::System && !options.include_system {
                continue;
            }
            for root in roots.iter() {
                catalog.scan_root(root, source, &options);
            }
        }
        catalog.sort();
        catalog
    }

    fn scan_root(&mut self, root: &Path, source: AppSource, options: &ScanOptions) {
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) => {
                if root.exists() {
                    self.errors.push(format!("{}: {}", root.display(), error));
                }
                return;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest = path.join("appinfo.json");
            let text = match std::fs::read_to_string(&manifest) {
                Ok(text) => text,
                Err(_) => {
                    if options.hide_broken {
                        continue;
                    }
                    // Каталог без манифеста — возможно, недоустановленное приложение.
                    let id = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if id.is_empty() {
                        continue;
                    }
                    let mut info = WebOsAppInfo::default();
                    info.id = id;
                    self.apps.push(InstalledApp {
                        info,
                        path: path.clone(),
                        source,
                        size_bytes: None,
                        icon_path: None,
                        running: false,
                        favorite_order: None,
                    });
                    continue;
                }
            };
            let info = match WebOsAppInfo::parse(&text) {
                Ok(info) => info,
                Err(error) => {
                    self.errors
                        .push(format!("{}: {}", manifest.display(), error));
                    continue;
                }
            };
            if options.hide_broken && info.id.is_empty() {
                continue;
            }
            let icon_path = resolve_icon(&path, &info);
            let size_bytes = if options.measure_size {
                Some(dir_size(&path))
            } else {
                None
            };
            self.apps.push(InstalledApp {
                info,
                path,
                source,
                size_bytes,
                icon_path,
                running: false,
                favorite_order: None,
            });
        }
    }

    /// Добавляет сведения из Luna (`folderPath`, `visible`, …) к уже найденным приложениям.
    pub fn merge_luna(&mut self, apps: Vec<WebOsAppInfo>) {
        for remote in apps {
            if remote.id.is_empty() {
                continue;
            }
            match self.apps.iter_mut().find(|a| a.info.id == remote.id) {
                Some(existing) => {
                    // Luna знает актуальный путь и состояние видимости.
                    if let Some(folder) = &remote.folder_path {
                        let path = PathBuf::from(folder);
                        if path.is_dir() {
                            existing.path = path.clone();
                            if existing.icon_path.is_none() {
                                existing.icon_path = resolve_icon(&path, &remote);
                            }
                        }
                    }
                    if existing.info.title.is_none() {
                        existing.info.title = remote.title.clone();
                    }
                    if existing.info.version.is_none() {
                        existing.info.version = remote.version.clone();
                    }
                    if existing.info.icon.is_none() {
                        existing.info.icon = remote.icon.clone();
                    }
                }
                None => {
                    let path = remote
                        .folder_path
                        .as_ref()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| {
                            PathBuf::from("/usr/palm/applications").join(&remote.id)
                        });
                    let source = if path.starts_with("/media/developer") {
                        AppSource::Developer
                    } else if path.starts_with("/media/cryptofs") {
                        AppSource::User
                    } else {
                        AppSource::System
                    };
                    let icon_path = if path.is_dir() {
                        resolve_icon(&path, &remote)
                    } else {
                        None
                    };
                    self.apps.push(InstalledApp {
                        info: remote,
                        path,
                        source,
                        size_bytes: None,
                        icon_path,
                        running: false,
                        favorite_order: None,
                    });
                }
            }
        }
        self.sort();
    }

    /// Помечает запущенные приложения.
    pub fn mark_running(&mut self, running: &[String]) {
        for app in &mut self.apps {
            app.running = running.iter().any(|id| id == &app.info.id);
        }
    }

    /// Применяет порядок избранного из настроек.
    pub fn apply_favorites(&mut self, favorites: &[String]) {
        for app in &mut self.apps {
            app.favorite_order = favorites.iter().position(|id| id == &app.info.id);
        }
    }

    fn sort(&mut self) {
        self.apps.sort_by(|a, b| {
            let left = a.title().to_lowercase();
            let right = b.title().to_lowercase();
            left.cmp(&right).then_with(|| a.info.id.cmp(&b.info.id))
        });
    }

    /// Приложение по идентификатору.
    pub fn by_id(&self, id: &str) -> Option<&InstalledApp> {
        self.apps.iter().find(|app| app.info.id == id)
    }

    /// Только пользовательские (несистемные) приложения — для экрана «Приложения».
    pub fn user_apps(&self) -> Vec<&InstalledApp> {
        self.apps
            .iter()
            .filter(|app| !matches!(app.source, AppSource::System))
            .collect()
    }

    /// Избранные приложения в порядке, заданном пользователем.
    pub fn favorites(&self) -> Vec<&InstalledApp> {
        let mut items: Vec<&InstalledApp> = self
            .apps
            .iter()
            .filter(|app| app.favorite_order.is_some())
            .collect();
        items.sort_by_key(|app| app.favorite_order.unwrap_or(usize::MAX));
        items
    }

    /// Штатные приложения LG.
    pub fn system_apps(&self) -> Vec<&InstalledApp> {
        self.apps
            .iter()
            .filter(|app| matches!(app.source, AppSource::System))
            .collect()
    }

    /// Количество приложений в каталоге.
    pub fn len(&self) -> usize {
        self.apps.len()
    }

    /// Пуст ли каталог.
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }
}

/// Ищет файл иконки приложения.
pub fn resolve_icon(app_dir: &Path, info: &WebOsAppInfo) -> Option<PathBuf> {
    let candidates: Vec<Option<&str>> = vec![
        info.large_icon.as_deref(),
        info.icon.as_deref(),
        Some("icon.png"),
        Some("app-icon.png"),
        Some("assets/icon130.png"),
        Some("assets/icon80.png"),
    ];
    for candidate in candidates.into_iter().flatten() {
        let path = app_dir.join(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// Рекурсивный размер каталога (без учёта симлинков).
pub fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if child.is_symlink() {
            continue;
        }
        if child.is_dir() {
            total += dir_size(&child);
        } else if let Ok(meta) = child.metadata() {
            total += meta.len();
        }
    }
    total
}

/// Демонстрационный каталог для desktop-превью и снимков экрана.
///
/// Используется только по явному флагу `--demo`; на телевизоре NovaOS всегда
/// показывает настоящие данные (подменять их демонстрационными нельзя).
pub fn demo_catalog() -> Catalog {
    let items: Vec<(&str, &str, &str, AppSource)> = vec![
        (
            "org.webosbrew.hbchannel",
            "Homebrew Channel",
            "web",
            AppSource::Developer,
        ),
        ("com.ecklf.luma", "Luma", "native", AppSource::User),
        ("com.novaos.store", "Nova Магазин", "web", AppSource::User),
        ("ru.yandex.kinopoisk", "Кинопоиск", "web", AppSource::System),
        ("com.netflix.ninja", "Netflix", "web", AppSource::System),
        ("youtube.leanback.v4", "YouTube", "web", AppSource::System),
        ("com.webos.app.browser", "Браузер", "web", AppSource::System),
        ("com.webos.app.hdmi1", "HDMI 1", "stub", AppSource::System),
        (
            "com.webos.app.livetv",
            "ТВ-каналы",
            "web",
            AppSource::System,
        ),
        ("com.novaos.iptv", "IPTV Плейлист", "web", AppSource::User),
        ("com.plex.htpc", "Plex", "web", AppSource::User),
        ("com.jellyfin.app", "Jellyfin", "web", AppSource::User),
    ];
    let mut catalog = Catalog::default();
    for (id, title, kind, source) in items {
        let mut info = WebOsAppInfo::default();
        info.id = id.to_string();
        info.title = Some(title.to_string());
        info.app_type = Some(kind.to_string());
        info.version = Some(if source == AppSource::System {
            "1.0".into()
        } else {
            "0.4.2".into()
        });
        info.vendor = Some(match source {
            AppSource::System => "LG Electronics".into(),
            _ => "NovaOS".into(),
        });
        info.app_description = Some(format!("{title} — установлено на этом телевизоре"));
        catalog.apps.push(InstalledApp {
            info,
            path: PathBuf::from("/media/cryptofs/apps/usr/palm/applications").join(id),
            source,
            size_bytes: Some(18 * 1024 * 1024),
            icon_path: None,
            running: id == "youtube.leanback.v4",
            favorite_order: None,
        });
    }
    let favorites: Vec<String> = vec![
        "com.novaos.store".into(),
        "youtube.leanback.v4".into(),
        "ru.yandex.kinopoisk".into(),
        "com.novaos.iptv".into(),
        "org.webosbrew.hbchannel".into(),
    ];
    catalog.apply_favorites(&favorites);
    catalog.sort();
    catalog
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_app(root: &Path, id: &str, title: &str, kind: &str) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("appinfo.json"),
            format!(r#"{{"id":"{id}","title":"{title}","type":"{kind}","version":"1.0","main":"index.html"}}"#),
        )
        .unwrap();
        std::fs::write(dir.join("icon.png"), b"\x89PNG\r\n\x1a\nfake").unwrap();
    }

    #[test]
    fn scans_fake_webos_tree() {
        let tmp = std::env::temp_dir().join(format!("nova-catalog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let system = tmp.join("usr/palm/applications");
        let user = tmp.join("media/cryptofs/apps/usr/palm/applications");
        let dev = tmp.join("media/developer/apps/usr/palm/applications");
        for dir in [&system, &user, &dev] {
            std::fs::create_dir_all(dir).unwrap();
        }
        write_app(&system, "com.webos.app.home", "LG Home", "native");
        write_app(&user, "com.novaos.launcher", "NovaOS", "native");
        write_app(&dev, "org.webosbrew.hbchannel", "Homebrew Channel", "web");
        // Битый каталог без манифеста.
        std::fs::create_dir_all(user.join("com.broken.app")).unwrap();

        let paths = ScanPaths::for_root(&tmp);
        let catalog = Catalog::scan(
            &paths,
            ScanOptions {
                measure_size: true,
                include_system: true,
                hide_broken: false,
            },
        );
        assert!(
            catalog.len() >= 4,
            "ожидалось ≥4 приложений, получено {}",
            catalog.len()
        );
        let nova = catalog
            .by_id("com.novaos.launcher")
            .expect("NovaOS найдена");
        assert_eq!(nova.title(), "NovaOS");
        assert_eq!(nova.source, AppSource::User);
        assert!(nova.icon_path.is_some(), "иконка должна находиться");
        assert!(nova.size_bytes.unwrap() > 0);
        assert!(catalog.by_id("com.webos.app.home").is_some());
        assert_eq!(
            catalog.user_apps().len(),
            3,
            "системное приложение не входит в пользовательские"
        );

        let without_system = Catalog::scan(
            &paths,
            ScanOptions {
                include_system: false,
                ..Default::default()
            },
        );
        assert!(without_system.by_id("com.webos.app.home").is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_luna_adds_missing_and_updates_paths() {
        let mut catalog = Catalog::default();
        let mut existing = WebOsAppInfo::default();
        existing.id = "com.novaos.launcher".into();
        catalog.apps.push(InstalledApp {
            info: existing,
            path: PathBuf::from("/nowhere"),
            source: AppSource::User,
            size_bytes: None,
            icon_path: None,
            running: false,
            favorite_order: None,
        });

        let mut remote = WebOsAppInfo::default();
        remote.id = "com.novaos.launcher".into();
        remote.title = Some("NovaOS".into());
        remote.version = Some("0.1.0".into());
        let mut fresh = WebOsAppInfo::default();
        fresh.id = "com.example.new".into();
        fresh.title = Some("Новое".into());
        fresh.folder_path =
            Some("/media/developer/apps/usr/palm/applications/com.example.new".into());
        catalog.merge_luna(vec![remote, fresh]);

        assert_eq!(
            catalog.by_id("com.novaos.launcher").unwrap().title(),
            "NovaOS"
        );
        let new = catalog.by_id("com.example.new").unwrap();
        assert_eq!(new.source, AppSource::Developer);
    }

    #[test]
    fn favorites_keep_order() {
        let mut catalog = demo_catalog();
        let favorites: Vec<&InstalledApp> = catalog.favorites();
        assert!(!favorites.is_empty());
        assert_eq!(favorites[0].info.id, "com.novaos.store");
        assert_eq!(favorites.len(), 5);
        catalog.mark_running(&["com.novaos.store".into()]);
        assert!(catalog.by_id("com.novaos.store").unwrap().running);
    }
}

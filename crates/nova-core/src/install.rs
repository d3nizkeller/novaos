//! Установка «своих приложений всех форматов».
//!
//! Конвейер установки:
//!
//! ```text
//! источник (URL / файл / USB / память)
//!   → определение формата           (crate::sniff)
//!   → загрузка и SHA-256            (crate::net)
//!   → преобразование в IPK, если нужно (crate::webapp)
//!   → запись в рабочий каталог
//!   → регистрация в системе:
//!       1. com.webos.appInstallService/dev/install   (режим разработчика)
//!       2. Homebrew Channel → root-exec того же сервиса
//!       3. Homebrew Channel → install по URL
//!       4. ручная распаковка в /media/cryptofs (свой двоичный файл)
//! ```
//!
//! Каждый следующий способ пробуется, только если предыдущий недоступен.
//! Выбранный способ попадает в отчёт — пользователь видит, как именно
//! приложение оказалось на телевизоре.

use std::path::{Path, PathBuf};

use crate::app::AppKind;
use crate::error::{NovaError, NovaResult};
use crate::ipk::{self, IpkInfo};
use crate::luna::LunaClient;
use crate::net::{self, DownloadProgress};
use crate::settings::InstallBackend;
use crate::sniff::{self, PackageFormat};
use crate::webapp::{self, WebAppSpec};

/// Источник пакета.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallSource {
    /// HTTP(S)-ссылка.
    Url(String),
    /// Файл на устройстве (USB, /tmp, каталог приложения).
    File(PathBuf),
    /// Байты в памяти (например, из локального HTTP-приёмника).
    Memory {
        /// Имя файла (влияет на определение формата и идентификатор).
        name: String,
        /// Содержимое.
        bytes: Vec<u8>,
    },
}

impl InstallSource {
    /// Человекочитаемое имя источника.
    pub fn display_name(&self) -> String {
        match self {
            InstallSource::Url(url) => url.clone(),
            InstallSource::File(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string()),
            InstallSource::Memory { name, .. } => name.clone(),
        }
    }

    /// Это сетевой источник.
    pub fn is_remote(&self) -> bool {
        matches!(self, InstallSource::Url(_))
    }
}

/// Способ установки, которым пакет реально попал в систему.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Напрямую через `appInstallService` (нужен активный режим разработчика).
    AppInstallService,
    /// Через root-exec Homebrew Channel.
    HomebrewExec,
    /// Через сервис установки Homebrew Channel (по URL).
    HomebrewUrl,
    /// Ручная распаковка в cryptofs двоичным файлом NovaOS.
    ManualExtract,
    /// Только проверка: пакет разобран, но не установлен (desktop-превью).
    DryRun,
}

impl Strategy {
    /// Русское название способа.
    pub fn label_ru(&self) -> &'static str {
        match self {
            Strategy::AppInstallService => "Системный установщик webOS",
            Strategy::HomebrewExec => "Homebrew Channel (root)",
            Strategy::HomebrewUrl => "Homebrew Channel (по ссылке)",
            Strategy::ManualExtract => "Ручная установка в cryptofs",
            Strategy::DryRun => "Проверка без установки",
        }
    }
}

/// Стадия установки — то, что видит пользователь в диалоге прогресса.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallStage {
    /// Определяем формат.
    Detecting,
    /// Загружаем пакет.
    Downloading {
        /// Доля выполнения 0…1.
        progress: f32,
        /// Получено байт.
        received: u64,
        /// Всего байт.
        total: Option<u64>,
    },
    /// Проверяем контрольную сумму.
    Verifying,
    /// Преобразуем в приложение webOS.
    Converting {
        /// Пояснение для интерфейса.
        note: String,
    },
    /// Разбираем пакет.
    Inspecting,
    /// Записываем в рабочий каталог.
    Writing {
        /// Путь к IPK.
        path: String,
    },
    /// Регистрируем в системе.
    Registering {
        /// Способ установки.
        strategy: Strategy,
        /// Текст статуса из системного сервиса.
        status: String,
    },
    /// Готово.
    Finished,
}

/// Отчёт об установке.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallReport {
    /// Идентификатор установленного приложения.
    pub app_id: String,
    /// Название.
    pub title: String,
    /// Версия.
    pub version: String,
    /// Тип приложения.
    pub kind: AppKind,
    /// Исходный формат.
    pub format: PackageFormat,
    /// Способ установки.
    pub strategy: Strategy,
    /// SHA-256 установленного IPK.
    pub sha256: String,
    /// Размер IPK.
    pub size_bytes: u64,
    /// Путь к IPK на устройстве.
    pub ipk_path: Option<PathBuf>,
    /// Предупреждения (например, «приложение для ARM, но устройство aarch64»).
    pub notes: Vec<String>,
}

/// Подготовленный к установке пакет.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// Байты готового IPK.
    pub ipk: Vec<u8>,
    /// Разобранные сведения о пакете.
    pub info: IpkInfo,
    /// Формат исходного файла.
    pub format: PackageFormat,
    /// Путь, куда записан IPK (если записан).
    pub path: Option<PathBuf>,
    /// Предупреждения.
    pub notes: Vec<String>,
}

/// Установщик NovaOS.
#[derive(Debug)]
pub struct Installer {
    luna: LunaClient,
    work_dir: PathBuf,
    backend: InstallBackend,
    /// Путь к двоичному файлу NovaOS на устройстве (для ручной распаковки от root).
    self_binary: Option<PathBuf>,
    /// Не выполнять системных вызовов (desktop-превью и тесты).
    dry_run: bool,
}

impl Installer {
    /// Создаёт установщик.
    pub fn new(luna: LunaClient, work_dir: PathBuf, backend: InstallBackend) -> Self {
        let dry_run = !luna.available();
        Self {
            luna,
            work_dir,
            backend,
            self_binary: std::env::current_exe().ok(),
            dry_run,
        }
    }

    /// Установщик для тестов: только подготовка пакета, без системных вызовов.
    pub fn dry_run(work_dir: PathBuf) -> Self {
        Self {
            luna: LunaClient::offline(),
            work_dir,
            backend: InstallBackend::Auto,
            self_binary: None,
            dry_run: true,
        }
    }

    /// Рабочий каталог.
    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    /// Загружает, определяет формат и при необходимости конвертирует источник в IPK.
    pub fn prepare<F>(
        &self,
        source: &InstallSource,
        expected_sha256: Option<&str>,
        mut on_stage: F,
    ) -> NovaResult<Prepared>
    where
        F: FnMut(InstallStage),
    {
        on_stage(InstallStage::Detecting);
        let (name, bytes) = self.load(source, &mut on_stage)?;

        let sha256 = net::sha256_hex(&bytes);
        if let Some(expected) = expected_sha256 {
            on_stage(InstallStage::Verifying);
            net::verify_checksum(expected, &sha256)?;
        }

        let format = sniff::detect(&bytes, Some(&name));
        let mut notes = Vec::new();
        let ipk = match format {
            PackageFormat::Ipk => bytes,
            PackageFormat::LegacyIpk => {
                notes.push("Пакет в старом формате opkg: NovaOS перечитала его как IPK.".into());
                bytes
            }
            PackageFormat::HtmlApp => {
                on_stage(InstallStage::Converting {
                    note: "HTML-файл упаковывается в приложение webOS".into(),
                });
                let spec = WebAppSpec::from_name(&name);
                webapp::from_html(&bytes, &spec)?
            }
            PackageFormat::ZipWebApp => {
                on_stage(InstallStage::Converting {
                    note: "ZIP-архив распаковывается и упаковывается в IPK".into(),
                });
                let spec = WebAppSpec::from_name(&name);
                webapp::from_zip(&bytes, &spec)?
            }
            PackageFormat::Playlist => {
                on_stage(InstallStage::Converting {
                    note: "Создаётся IPTV-приложение NovaOS с этим плейлистом".into(),
                });
                let text = String::from_utf8_lossy(&bytes).to_string();
                let spec = WebAppSpec::from_name(&name).with_title(cleanup_name(&name));
                webapp::from_playlist(&text, &spec)?
            }
            PackageFormat::Apk => {
                let info = crate::apk::parse(&bytes)?;
                return Err(NovaError::Unsupported(format!(
                    "APK «{}» ({}) нельзя установить на webOS: это пакет для Android. {}",
                    info.display_title(),
                    info.package,
                    if info.is_tv_app() {
                        "Пакет рассчитан на Android TV — установите его на Android-устройство."
                    } else {
                        "Это приложение для телефона, на телевизоре оно не запустится."
                    }
                )));
            }
            PackageFormat::Deb => {
                return Err(NovaError::Unsupported(
                    "Это Debian-пакет (.deb): webOS использует формат IPK. \
                     Распакуйте данные и соберите IPK командой `nova package`."
                        .into(),
                ));
            }
            PackageFormat::Media => {
                return Err(NovaError::Unsupported(
                    "Это медиафайл. Добавьте его в «Источники» — он появится в медиатеке NovaOS."
                        .into(),
                ));
            }
            PackageFormat::RepoManifest => {
                return Err(NovaError::Unsupported(
                    "Это манифест репозитория. Добавьте его в «Настройки → Репозитории».".into(),
                ));
            }
            PackageFormat::WebOsAppDir => {
                return Err(NovaError::Invalid(
                    "Каталоги приложений устанавливаются из файловой системы: используйте `nova package <каталог>`.".into(),
                ));
            }
            PackageFormat::Unknown => {
                return Err(NovaError::Package(format!(
                    "неизвестный формат файла «{name}»: NovaOS поддерживает IPK, ZIP/HTML (веб-приложения), M3U, APK (только анализ)"
                )));
            }
        };

        on_stage(InstallStage::Inspecting);
        let info = ipk::parse(&ipk)?;
        if info.control.architecture == "arm" && !cfg!(target_arch = "arm") {
            notes.push("Пакет собран для ARM: на desktop-превью он не запустится.".into());
        }
        if info.has_service {
            notes.push(
                "Пакет содержит сервис Luna — после установки может потребоваться перезагрузка."
                    .into(),
            );
        }
        Ok(Prepared {
            ipk,
            info,
            format,
            path: None,
            notes,
        })
    }

    fn load<F>(&self, source: &InstallSource, on_stage: &mut F) -> NovaResult<(String, Vec<u8>)>
    where
        F: FnMut(InstallStage),
    {
        match source {
            InstallSource::Url(url) => {
                let name = url
                    .split(['/', '?'])
                    .filter(|part| !part.is_empty() && part.contains('.'))
                    .last()
                    .unwrap_or("package.ipk")
                    .to_string();
                let downloaded = net::download(url, |progress: DownloadProgress| {
                    on_stage(InstallStage::Downloading {
                        progress: progress.fraction(),
                        received: progress.received,
                        total: progress.total,
                    });
                })?;
                Ok((name, downloaded.bytes))
            }
            InstallSource::File(path) => {
                let bytes = std::fs::read(path)?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "package.ipk".into());
                Ok((name, bytes))
            }
            InstallSource::Memory { name, bytes } => Ok((name.clone(), bytes.clone())),
        }
    }

    /// Записывает IPK в рабочий каталог и возвращает путь.
    pub fn write_work_file(&self, app_id: &str, version: &str, ipk: &[u8]) -> NovaResult<PathBuf> {
        std::fs::create_dir_all(&self.work_dir)?;
        let safe_id = app_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let path = self.work_dir.join(format!("{safe_id}_{version}.ipk"));
        std::fs::write(&path, ipk)?;
        Ok(path)
    }

    /// Устанавливает пакет выбранным доступным способом.
    pub fn install<F>(
        &self,
        source: &InstallSource,
        expected_sha256: Option<&str>,
        mut on_stage: F,
    ) -> NovaResult<InstallReport>
    where
        F: FnMut(InstallStage),
    {
        let mut prepared = self.prepare(source, expected_sha256, &mut on_stage)?;
        let info = &prepared.info;
        let app_id = info.app_id().to_string();
        let version = info.version();
        let sha256 = net::sha256_hex(&prepared.ipk);

        let path = self.write_work_file(&app_id, &version, &prepared.ipk)?;
        prepared.path = Some(path.clone());
        on_stage(InstallStage::Writing {
            path: path.display().to_string(),
        });

        if self.dry_run {
            on_stage(InstallStage::Registering {
                strategy: Strategy::DryRun,
                status: "Пакет проверен и подготовлен".into(),
            });
            prepared
                .notes
                .push("Установка не выполнялась: шина Luna недоступна (desktop-превью).".into());
            on_stage(InstallStage::Finished);
            let mut report = self.report(&prepared, Strategy::DryRun, sha256);
            report.notes = prepared.notes.clone();
            return Ok(report);
        }

        let mut attempts: Vec<String> = Vec::new();
        for strategy in self.strategy_order(source) {
            on_stage(InstallStage::Registering {
                strategy,
                status: format!("Пробуем: {}", strategy.label_ru()),
            });
            match self.try_strategy(strategy, source, &path, &mut on_stage) {
                Ok(()) => {
                    on_stage(InstallStage::Finished);
                    let mut report = self.report(&prepared, strategy, sha256);
                    report.notes.extend(prepared.notes.drain(..));
                    report.notes.extend(attempts);
                    return Ok(report);
                }
                Err(error) => {
                    attempts.push(format!("{}: {error}", strategy.label_ru()));
                }
            }
        }
        Err(NovaError::Permission(format!(
            "не удалось установить пакет ни одним способом. {}",
            attempts.join(" | ")
        )))
    }

    /// Порядок способов установки с учётом настроек и доступности.
    fn strategy_order(&self, source: &InstallSource) -> Vec<Strategy> {
        let auto = vec![
            Strategy::AppInstallService,
            Strategy::HomebrewExec,
            Strategy::HomebrewUrl,
            Strategy::ManualExtract,
        ];
        let filtered = match self.backend {
            InstallBackend::Auto => auto,
            InstallBackend::AppInstallService => vec![Strategy::AppInstallService],
            InstallBackend::Homebrew => vec![Strategy::HomebrewExec, Strategy::HomebrewUrl],
            InstallBackend::Manual => vec![Strategy::ManualExtract],
        };
        filtered
            .into_iter()
            .filter(|strategy| *strategy != Strategy::HomebrewUrl || source.is_remote())
            .collect()
    }

    fn try_strategy<F>(
        &self,
        strategy: Strategy,
        source: &InstallSource,
        path: &Path,
        on_stage: &mut F,
    ) -> NovaResult<()>
    where
        F: FnMut(InstallStage),
    {
        let path_text = path.display().to_string();
        match strategy {
            Strategy::AppInstallService => {
                self.luna.install_ipk(&path_text, |status, code| {
                    on_stage(InstallStage::Registering {
                        strategy,
                        status: format!(
                            "Установка ({}) — {status}",
                            code.map(|c| c.to_string()).unwrap_or_else(|| "…".into())
                        ),
                    });
                })?;
                Ok(())
            }
            Strategy::HomebrewExec => {
                let command = format!(
                    "luna-send-pub -n 40 luna://com.webos.appInstallService/dev/install '{{\"id\":\"novaos\",\"ipkUrl\":\"{}\",\"subscribe\":true}}'",
                    path_text
                );
                let output = self.luna.exec_root(&command)?;
                if output.contains("\"statusValue\":30") || output.contains("packageId") {
                    Ok(())
                } else {
                    Err(NovaError::Luna(truncate(output.trim(), 180).to_string()))
                }
            }
            Strategy::HomebrewUrl => {
                let InstallSource::Url(url) = source else {
                    return Err(NovaError::Invalid("способ доступен только для URL".into()));
                };
                let sha256 = net::sha256_file(path)?;
                let payload = serde_json::json!({
                    "ipkUrl": url,
                    "ipkHash": sha256,
                    "subscribe": true,
                });
                let response = self.luna.subscribe(
                    "org.webosbrew.hbchannel.service/install",
                    &payload,
                    |value| {
                        let text = value
                            .get("statusText")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        on_stage(InstallStage::Registering {
                            strategy,
                            status: text,
                        });
                        value.get("finished").and_then(|v| v.as_bool()) != Some(true)
                    },
                )?;
                match response.error_text() {
                    Some(error) => Err(NovaError::Luna(error)),
                    None => Ok(()),
                }
            }
            Strategy::ManualExtract => {
                let Some(binary) = &self.self_binary else {
                    return Err(NovaError::Unsupported(
                        "не найден двоичный файл NovaOS".into(),
                    ));
                };
                let app_id = ipk::parse(&std::fs::read(path)?)?.app_id().to_string();
                let command = manual_install_command(binary, path, &app_id);
                let output = self.luna.exec_root(&command)?;
                if output.contains("NOVA_MANUAL_OK") {
                    Ok(())
                } else {
                    Err(NovaError::Luna(truncate(output.trim(), 180).to_string()))
                }
            }
            Strategy::DryRun => Ok(()),
        }
    }

    fn report(&self, prepared: &Prepared, strategy: Strategy, sha256: String) -> InstallReport {
        let info = &prepared.info;
        InstallReport {
            app_id: info.app_id().to_string(),
            title: info.title(),
            version: info.version(),
            kind: match info.app_type().as_str() {
                "native" => AppKind::Native,
                "stub" => AppKind::Stub,
                "media" => AppKind::Media,
                _ => AppKind::Web,
            },
            format: prepared.format,
            strategy,
            sha256,
            size_bytes: info.archive_size,
            ipk_path: prepared.path.clone(),
            notes: Vec::new(),
        }
    }

    /// Удаляет приложение.
    pub fn uninstall(&self, app_id: &str) -> NovaResult<Strategy> {
        if self.dry_run {
            return Ok(Strategy::DryRun);
        }
        // Сначала штатный способ.
        if self.luna.remove_app(app_id).is_ok() {
            return Ok(Strategy::AppInstallService);
        }
        // Затем root-удаление каталога.
        let command = format!(
            "luna-send-pub -n 20 luna://com.webos.appInstallService/dev/remove '{{\"id\":\"{app_id}\",\"subscribe\":true}}'"
        );
        if let Ok(output) = self.luna.exec_root(&command) {
            if !output.contains("returnValue\":false") {
                return Ok(Strategy::HomebrewExec);
            }
        }
        let cleanup = format!(
            "rm -rf /media/cryptofs/apps/usr/palm/applications/{app_id} /media/cryptofs/apps/usr/palm/packages/{app_id} && sync && echo NOVA_MANUAL_OK"
        );
        if let Ok(output) = self.luna.exec_root(&cleanup) {
            if output.contains("NOVA_MANUAL_OK") {
                return Ok(Strategy::ManualExtract);
            }
        }
        Err(NovaError::Permission(format!(
            "не удалось удалить {app_id}: нужен root или активный режим разработчика"
        )))
    }
}

/// Команда root-установки: распаковка IPK собственным двоичным файлом NovaOS.
///
/// На webOS нет утилиты `ar`, поэтому извлечение `data.tar.gz` выполняет сам
/// лаунчер (`nova --extract-ipk`), запущенный от root через Homebrew Channel.
pub fn manual_install_command(binary: &Path, ipk: &Path, app_id: &str) -> String {
    format!(
        "sh -c '{} --extract-ipk {} --app-id {} --dest /media/cryptofs/apps && sync && echo NOVA_MANUAL_OK'",
        binary.display(),
        ipk.display(),
        app_id
    )
}

fn truncate(text: &str, max: usize) -> &str {
    if text.chars().count() <= max {
        text
    } else {
        let end = text
            .char_indices()
            .nth(max)
            .map(|(i, _)| i)
            .unwrap_or(text.len());
        &text[..end]
    }
}

fn cleanup_name(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string())
}

/// Кандидат на установку, найденный на съёмном носителе.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageCandidate {
    /// Путь к файлу.
    pub path: PathBuf,
    /// Имя файла.
    pub name: String,
    /// Размер.
    pub size: u64,
    /// Определённый формат.
    pub format: PackageFormat,
    /// Съёмный носитель (например, `/media/usb1`).
    pub volume: PathBuf,
}

/// Каталоги, где webOS монтирует USB-накопители.
pub const REMOVABLE_ROOTS: [&str; 5] = [
    "/media/usb",
    "/media/usb1",
    "/media/usb2",
    "/mnt/usb",
    "/media/sda1",
];

/// Ищет пакеты на подключённых USB-накопителях.
///
/// Просматриваются каталоги верхнего уровня и подкаталоги глубиной до двух,
/// чтобы пользователь мог просто положить файл в корень флешки.
pub fn scan_removable(roots: Option<&[&str]>) -> Vec<PackageCandidate> {
    let roots: Vec<PathBuf> = match roots {
        Some(list) => list.iter().map(PathBuf::from).collect(),
        None => REMOVABLE_ROOTS.iter().map(PathBuf::from).collect(),
    };
    let mut found = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        collect_candidates(&root, &root, 0, &mut found);
    }
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    found
}

fn collect_candidates(volume: &Path, dir: &Path, depth: usize, out: &mut Vec<PackageCandidate>) {
    if depth > 2 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            // Пропускаем служебные каталоги файловых систем.
            if name.starts_with('.') || name == "System Volume Information" || name == "LGSMARTAD" {
                continue;
            }
            collect_candidates(volume, &path, depth + 1, out);
            continue;
        }
        let Ok(meta) = path.metadata() else { continue };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let extension = Path::new(&name)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(
            extension.as_str(),
            "ipk" | "apk" | "zip" | "html" | "htm" | "m3u" | "m3u8" | "deb"
        ) {
            continue;
        }
        let format = sniff::detect_file(&path).unwrap_or(PackageFormat::Unknown);
        out.push(PackageCandidate {
            path: path.clone(),
            name,
            size: meta.len(),
            format,
            volume: volume.to_path_buf(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipk::{ControlFile, PackageFile};

    fn sample_ipk(id: &str) -> Vec<u8> {
        ipk::build(
            ControlFile::new(id, "1.0.0", "all"),
            vec![
                PackageFile::new(
                    format!("usr/palm/applications/{id}/appinfo.json"),
                    format!(r#"{{"id":"{id}","title":"Тест","type":"web","main":"index.html","version":"1.0.0"}}"#).into_bytes(),
                ),
                PackageFile::new(
                    format!("usr/palm/applications/{id}/index.html"),
                    b"<html>test</html>".to_vec(),
                ),
            ],
        )
        .unwrap()
    }

    #[test]
    fn prepares_ipk_without_installing() {
        let dir = std::env::temp_dir().join(format!("nova-install-{}", std::process::id()));
        let installer = Installer::dry_run(dir.clone());
        let ipk = sample_ipk("com.novaos.test.prepare");
        let mut stages = Vec::new();
        let prepared = installer
            .prepare(
                &InstallSource::Memory {
                    name: "com.novaos.test.prepare_1.0.0_all.ipk".into(),
                    bytes: ipk,
                },
                None,
                |stage| stages.push(stage),
            )
            .unwrap();
        assert_eq!(prepared.format, PackageFormat::Ipk);
        assert_eq!(prepared.info.app_id(), "com.novaos.test.prepare");
        assert!(stages.iter().any(|s| matches!(s, InstallStage::Detecting)));
        assert!(stages.iter().any(|s| matches!(s, InstallStage::Inspecting)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn converts_html_and_playlist_to_ipk() {
        let dir = std::env::temp_dir().join(format!("nova-install-conv-{}", std::process::id()));
        let installer = Installer::dry_run(dir.clone());

        let html = installer
            .prepare(
                &InstallSource::Memory {
                    name: "Мой плеер.html".into(),
                    bytes: b"<!DOCTYPE html><html><body>1</body></html>".to_vec(),
                },
                None,
                |_| {},
            )
            .unwrap();
        assert_eq!(html.format, PackageFormat::HtmlApp);
        assert!(html.info.app_id().starts_with("com.novaos.side."));
        assert_eq!(html.info.app_type(), "web");

        let playlist = installer
            .prepare(
                &InstallSource::Memory {
                    name: "iptv.m3u".into(),
                    bytes: b"#EXTM3U\n#EXTINF:-1,Test\nhttp://x/1.m3u8\n".to_vec(),
                },
                None,
                |_| {},
            )
            .unwrap();
        assert_eq!(playlist.format, PackageFormat::Playlist);
        assert!(
            playlist.info.file_count >= 3,
            "иконки и манифест должны быть добавлены"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checksum_mismatch_is_rejected() {
        let dir = std::env::temp_dir().join(format!("nova-install-sum-{}", std::process::id()));
        let installer = Installer::dry_run(dir.clone());
        let error = installer
            .prepare(
                &InstallSource::Memory {
                    name: "app.ipk".into(),
                    bytes: sample_ipk("com.novaos.checksum"),
                },
                Some("0000000000000000000000000000000000000000000000000000000000000000"),
                |_| {},
            )
            .expect_err("ожидалась ошибка контрольной суммы");
        assert!(matches!(error, NovaError::Checksum { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apk_is_reported_not_installed() {
        let dir = std::env::temp_dir().join(format!("nova-install-apk-{}", std::process::id()));
        let installer = Installer::dry_run(dir.clone());
        // Минимальный «APK»: zip с AndroidManifest.xml непригодного содержания —
        // проверка формата срабатывает раньше разбора, поэтому используем имя файла.
        let error = installer
            .prepare(
                &InstallSource::Memory {
                    name: "app.apk".into(),
                    bytes: b"PK\x03\x04 fake apk".to_vec(),
                },
                None,
                |_| {},
            )
            .expect_err("APK не должен устанавливаться");
        assert!(matches!(
            error,
            NovaError::Unsupported(_) | NovaError::Package(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_install_reports_strategy() {
        let dir = std::env::temp_dir().join(format!("nova-install-dry-{}", std::process::id()));
        let installer = Installer::dry_run(dir.clone());
        let report = installer
            .install(
                &InstallSource::Memory {
                    name: "app.ipk".into(),
                    bytes: sample_ipk("com.novaos.dry"),
                },
                None,
                |_| {},
            )
            .unwrap();
        assert_eq!(report.strategy, Strategy::DryRun);
        assert_eq!(report.app_id, "com.novaos.dry");
        assert_eq!(report.title, "Тест");
        assert_eq!(report.kind, AppKind::Web);
        assert!(report.ipk_path.unwrap().exists());
        assert!(report.notes.iter().any(|n| n.contains("Luna")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strategy_order_respects_backend_and_source() {
        let dir = std::env::temp_dir().join("nova-install-order");
        let mut installer = Installer::dry_run(dir.clone());
        installer.backend = InstallBackend::Homebrew;
        let order = installer.strategy_order(&InstallSource::Url("https://x/y.ipk".into()));
        assert_eq!(order, vec![Strategy::HomebrewExec, Strategy::HomebrewUrl]);
        let order_local =
            installer.strategy_order(&InstallSource::File(PathBuf::from("/tmp/y.ipk")));
        assert_eq!(order_local, vec![Strategy::HomebrewExec]);

        installer.backend = InstallBackend::Auto;
        assert_eq!(
            installer
                .strategy_order(&InstallSource::File(PathBuf::from("/tmp/y.ipk")))
                .len(),
            3
        );
        installer.backend = InstallBackend::Manual;
        assert_eq!(
            installer.strategy_order(&InstallSource::File(PathBuf::from("/tmp/y.ipk"))),
            vec![Strategy::ManualExtract]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removable_scan_on_fake_volume() {
        let root = std::env::temp_dir().join(format!("nova-usb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("films")).unwrap();
        std::fs::write(root.join("app.ipk"), sample_ipk("com.novaos.usb")).unwrap();
        std::fs::write(root.join("films/video.mp4"), b"not a package").unwrap();
        std::fs::write(root.join("site.zip"), b"PK\x03\x04 zip").unwrap();
        let found = scan_removable(Some(&[root.to_str().unwrap()]));
        let names: Vec<&str> = found.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"app.ipk"), "{names:?}");
        assert!(names.contains(&"site.zip"));
        assert!(
            !names.contains(&"video.mp4"),
            "видео не должно предлагаться как пакет"
        );
        let ipk = found.iter().find(|c| c.name == "app.ipk").unwrap();
        assert_eq!(ipk.format, PackageFormat::Ipk);
        assert_eq!(ipk.volume, root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn manual_command_shape() {
        let command = manual_install_command(
            Path::new("/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher/nova"),
            Path::new("/tmp/x.ipk"),
            "com.example.app",
        );
        assert!(command.contains("--extract-ipk"));
        assert!(command.contains("com.example.app"));
        assert!(command.contains("NOVA_MANUAL_OK"));
    }
}

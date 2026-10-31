//! Пакеты webOS: `.ipk`.
//!
//! Формат проверен на реальных пакетах (`com.ecklf.luma_2.16.0_arm.ipk`,
//! `org.webosbrew.hbchannel_0.7.3_all.ipk`):
//!
//! ```text
//! app.ipk = ar {
//!   debian-binary  : "2.0\n"
//!   control.tar.gz : { control }
//!   data.tar.gz    : { usr/palm/applications/<id>/..., usr/palm/packages/<id>/packageinfo.json }
//! }
//! ```
//!
//! Здесь же — сборщик IPK, которым NovaOS упаковывает «свои» приложения
//! (HTML5, ZIP, M3U) в формат, понятный `com.webos.appInstallService`.

use std::collections::BTreeMap;
use std::io::Read;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

use crate::app::WebOsAppInfo;
use crate::ar::{self, ArMember};
use crate::error::{NovaError, NovaResult};

/// Поля управляющего файла `control`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlFile {
    /// Имя пакета = идентификатор приложения webOS.
    pub package: String,
    /// Версия пакета.
    pub version: String,
    /// Раздел (`misc`).
    #[serde(default = "default_section")]
    pub section: String,
    /// Приоритет (`optional`).
    #[serde(default = "default_priority")]
    pub priority: String,
    /// Архитектура: `arm` для нативных, `all` для веб-приложений.
    #[serde(default = "default_arch")]
    pub architecture: String,
    /// Распакованный размер в байтах.
    #[serde(default)]
    pub installed_size: u64,
    /// Сопровождающий.
    #[serde(default = "default_maintainer")]
    pub maintainer: String,
    /// Описание (одна строка).
    #[serde(default = "default_description")]
    pub description: String,
    /// Дополнительные поля (`webOS-Package-Format-Version` и т.п.).
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, String>,
}

fn default_section() -> String {
    "misc".into()
}
fn default_priority() -> String {
    "optional".into()
}
fn default_arch() -> String {
    "all".into()
}
fn default_maintainer() -> String {
    "NovaOS <novaos@localhost>".into()
}
fn default_description() -> String {
    "This is a webOS application.".into()
}

impl ControlFile {
    /// Минимальный корректный `control` для пакета NovaOS.
    pub fn new(
        package: impl Into<String>,
        version: impl Into<String>,
        architecture: impl Into<String>,
    ) -> Self {
        Self {
            package: package.into(),
            version: version.into(),
            architecture: architecture.into(),
            ..Self::default()
        }
    }

    /// Разбирает текстовый файл `control`.
    pub fn parse(text: &str) -> NovaResult<Self> {
        let mut control = Self::default();
        let mut continuation = false;
        for line in text.lines() {
            if line.trim().is_empty() {
                continuation = false;
                continue;
            }
            if (line.starts_with(' ') || line.starts_with('\t')) && continuation {
                control.description.push(' ');
                control.description.push_str(line.trim());
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().to_string();
            continuation = true;
            match key.to_ascii_lowercase().as_str() {
                "package" => control.package = value,
                "version" => control.version = value,
                "section" => control.section = value,
                "priority" => control.priority = value,
                "architecture" => control.architecture = value,
                "installed-size" => control.installed_size = value.parse().unwrap_or(0),
                "maintainer" => control.maintainer = value,
                "description" => control.description = value,
                _ => {
                    // Дополнительные поля сохраняются с исходным регистром
                    // (важно для webOS-Package-Format-Version).
                    control.extra.insert(key.to_string(), value);
                }
            }
        }
        if control.package.is_empty() {
            return Err(NovaError::Package(
                "в control отсутствует поле Package".into(),
            ));
        }
        Ok(control)
    }

    /// Сериализует в канонический текст `control`.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Package: {}\n", self.package));
        out.push_str(&format!("Version: {}\n", self.version));
        out.push_str(&format!("Section: {}\n", self.section));
        out.push_str(&format!("Priority: {}\n", self.priority));
        out.push_str(&format!("Architecture: {}\n", self.architecture));
        out.push_str(&format!("Installed-Size: {}\n", self.installed_size));
        out.push_str(&format!("Maintainer: {}\n", self.maintainer));
        out.push_str(&format!("Description: {}\n", self.description));
        for (key, value) in &self.extra {
            out.push_str(&format!("{key}: {value}\n"));
        }
        out
    }

    /// Типовой набор дополнительных полей webOS.
    pub fn with_webos_fields(mut self) -> Self {
        self.extra
            .insert("webOS-Package-Format-Version".into(), "2".into());
        self.extra
            .insert("webOS-Packager-Version".into(), "novaos".into());
        self
    }
}

/// Краткое содержимое элемента `data.tar.gz`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataEntry {
    /// Путь внутри архива (`usr/palm/applications/...`).
    pub path: String,
    /// Размер в байтах (0 для каталогов).
    pub size: u64,
    /// Это каталог.
    pub is_dir: bool,
    /// Права доступа.
    pub mode: u32,
}

/// Результат разбора IPK — всё, что нужно интерфейсу NovaOS.
#[derive(Debug, Clone)]
pub struct IpkInfo {
    /// Данные из `control`.
    pub control: ControlFile,
    /// `appinfo.json` приложения (если есть).
    pub appinfo: Option<WebOsAppInfo>,
    /// `packageinfo.json` из `usr/palm/packages/<id>/`.
    pub packageinfo: Option<serde_json::Value>,
    /// Содержит ли пакет сервис Luna (`usr/palm/services/...`).
    pub has_service: bool,
    /// Размер сжатого архива (в байтах).
    pub archive_size: u64,
    /// Суммарный распакованный размер.
    pub installed_size: u64,
    /// Количество файлов.
    pub file_count: usize,
    /// Список элементов архива данных.
    pub entries: Vec<DataEntry>,
    /// SHA-256 всего файла пакета (шестнадцатеричная строка).
    pub sha256: String,
}

impl IpkInfo {
    /// Идентификатор приложения.
    pub fn app_id(&self) -> &str {
        self.appinfo
            .as_ref()
            .map(|a| a.id.as_str())
            .filter(|id| !id.is_empty())
            .unwrap_or(&self.control.package)
    }

    /// Человекочитаемое название.
    pub fn title(&self) -> String {
        self.appinfo
            .as_ref()
            .and_then(|a| a.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| self.control.package.clone())
    }

    /// Версия приложения (из appinfo, иначе из control).
    pub fn version(&self) -> String {
        self.appinfo
            .as_ref()
            .and_then(|a| a.version.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| self.control.version.clone())
    }

    /// Тип приложения: `native`, `web`, `stub` и т. д.
    pub fn app_type(&self) -> String {
        self.appinfo
            .as_ref()
            .and_then(|a| a.app_type.clone())
            .unwrap_or_else(|| "web".into())
    }

    /// Является ли пакет нативным приложением для ARM.
    pub fn is_native(&self) -> bool {
        self.app_type() == "native" || self.control.architecture == "arm"
    }
}

fn normalize(path: &str) -> String {
    let trimmed = path.trim_start_matches("./").trim_start_matches('/');
    trimmed.to_string()
}

/// Разбирает IPK из памяти.
pub fn parse(bytes: &[u8]) -> NovaResult<IpkInfo> {
    let sha256 = crate::net::sha256_hex(bytes);
    let members = ar::read(bytes)?;
    let control_member = members
        .iter()
        .find(|m| m.name == "control.tar.gz" || m.name.starts_with("control.tar"))
        .ok_or_else(|| NovaError::Package("в IPK нет control.tar.gz".into()))?;
    let data_member = members
        .iter()
        .find(|m| m.name == "data.tar.gz" || m.name.starts_with("data.tar"))
        .ok_or_else(|| NovaError::Package("в IPK нет data.tar.gz".into()))?;

    // --- control ---
    let control_text = decompress_member(&control_member.data, &control_member.name)?;
    let mut control = None;
    {
        let mut archive = tar::Archive::new(&control_text[..]);
        for entry in archive
            .entries()
            .map_err(|e| NovaError::Package(e.to_string()))?
        {
            let mut entry = entry.map_err(|e| NovaError::Package(e.to_string()))?;
            let path = entry.path()?.to_string_lossy().to_string();
            if normalize(&path) == "control" {
                let mut text = String::new();
                entry
                    .read_to_string(&mut text)
                    .map_err(|e| NovaError::Package(e.to_string()))?;
                control = Some(ControlFile::parse(&text)?);
            }
        }
    }
    let control =
        control.ok_or_else(|| NovaError::Package("в control.tar.gz нет файла control".into()))?;

    // --- data ---
    let data = decompress_member(&data_member.data, &data_member.name)?;
    let mut archive = tar::Archive::new(&data[..]);
    let mut entries = Vec::new();
    let mut appinfo = None;
    let mut packageinfo = None;
    let mut has_service = false;
    let mut installed_size: u64 = 0;
    for entry in archive
        .entries()
        .map_err(|e| NovaError::Package(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| NovaError::Package(e.to_string()))?;
        let header = entry.header();
        let raw_path = entry.path()?.to_string_lossy().to_string();
        let path = normalize(&raw_path);
        if path.is_empty() {
            continue;
        }
        let is_dir = header.entry_type().is_dir();
        let size = header.size().unwrap_or(0);
        let mode = header.mode().unwrap_or(0o644);
        installed_size = installed_size.saturating_add(size);

        if path.contains("usr/palm/services/") {
            has_service = true;
        }
        let parts: Vec<&str> = path.split('/').collect();
        // usr/palm/applications/<id>/appinfo.json
        if parts.len() == 5
            && parts[0] == "usr"
            && parts[2] == "applications"
            && parts[4] == "appinfo.json"
        {
            let mut text = String::new();
            if entry.read_to_string(&mut text).is_ok() {
                appinfo = serde_json::from_str::<WebOsAppInfo>(&text).ok();
            }
        }
        // usr/palm/packages/<id>/packageinfo.json
        if parts.len() == 5
            && parts[0] == "usr"
            && parts[2] == "packages"
            && parts[4] == "packageinfo.json"
        {
            let mut text = String::new();
            if entry.read_to_string(&mut text).is_ok() {
                packageinfo = serde_json::from_str(&text).ok();
            }
        }
        entries.push(DataEntry {
            path,
            size,
            is_dir,
            mode,
        });
    }

    let file_count = entries.iter().filter(|e| !e.is_dir).count();
    Ok(IpkInfo {
        control,
        appinfo,
        packageinfo,
        has_service,
        archive_size: bytes.len() as u64,
        installed_size,
        file_count,
        entries,
        sha256,
    })
}

fn decompress_member(data: &[u8], name: &str) -> NovaResult<Vec<u8>> {
    let mut out = Vec::new();
    if name.ends_with(".gz") || data.starts_with(&[0x1f, 0x8b]) {
        GzDecoder::new(data)
            .read_to_end(&mut out)
            .map_err(|e| NovaError::Package(format!("не удалось распаковать {name}: {e}")))?;
    } else {
        out.extend_from_slice(data);
    }
    Ok(out)
}

/// Файл, добавляемый в `data.tar.gz`.
#[derive(Debug, Clone)]
pub struct PackageFile {
    /// Путь внутри пакета, например `usr/palm/applications/com.novaos.launcher/nova`.
    pub path: String,
    /// Содержимое.
    pub data: Vec<u8>,
    /// Права (например, `0o755` для исполняемого файла).
    pub mode: u32,
}

impl PackageFile {
    /// Обычный файл (0644).
    pub fn new(path: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            data,
            mode: 0o644,
        }
    }

    /// Исполняемый файл (0755).
    pub fn executable(path: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            data,
            mode: 0o755,
        }
    }
}

/// Собирает IPK из списка файлов.
///
/// Каталоги создаются автоматически, порядок элементов детерминирован —
/// одинаковые входные данные дают побайтово одинаковый пакет (важно для
/// проверяемых сборок и кэширования).
pub fn build(control: ControlFile, mut files: Vec<PackageFile>) -> NovaResult<Vec<u8>> {
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let mut installed_size: u64 = 0;
    let mut dirs: Vec<String> = Vec::new();
    for file in &files {
        installed_size = installed_size.saturating_add(file.data.len() as u64);
        let parts: Vec<&str> = file.path.split('/').collect();
        for i in 1..parts.len() {
            let dir = format!("{}/", parts[..i].join("/"));
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs.sort();

    let mut control = control;
    if control.installed_size == 0 {
        control.installed_size = installed_size;
    }
    let control = control.with_webos_fields();

    // data.tar.gz
    let mut data_encoder = GzEncoder::new(Vec::new(), Compression::new(9));
    {
        let mut builder = tar::Builder::new(&mut data_encoder);
        builder.mode(tar::HeaderMode::Deterministic);
        for dir in &dirs {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header
                .set_path(dir)
                .map_err(|e| NovaError::Package(e.to_string()))?;
            header.set_size(0);
            header.set_mode(0o777);
            header.set_uid(1001);
            header.set_gid(1001);
            header.set_mtime(0);
            header.set_cksum();
            builder
                .append(&header, &[][..])
                .map_err(|e| NovaError::Package(e.to_string()))?;
        }
        for file in &files {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header
                .set_path(&file.path)
                .map_err(|e| NovaError::Package(e.to_string()))?;
            header.set_size(file.data.len() as u64);
            header.set_mode(file.mode);
            header.set_uid(1001);
            header.set_gid(1001);
            header.set_mtime(0);
            header.set_cksum();
            builder
                .append(&header, &file.data[..])
                .map_err(|e| NovaError::Package(e.to_string()))?;
        }
        builder
            .finish()
            .map_err(|e| NovaError::Package(e.to_string()))?;
    }
    let data = data_encoder
        .finish()
        .map_err(|e| NovaError::Package(e.to_string()))?;

    // control.tar.gz
    let control_bytes = control.to_text().into_bytes();
    let mut control_encoder = GzEncoder::new(Vec::new(), Compression::new(9));
    {
        let mut builder = tar::Builder::new(&mut control_encoder);
        builder.mode(tar::HeaderMode::Deterministic);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header
            .set_path("control")
            .map_err(|e| NovaError::Package(e.to_string()))?;
        header.set_size(control_bytes.len() as u64);
        header.set_mode(0o666);
        header.set_uid(1001);
        header.set_gid(1001);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append(&header, &control_bytes[..])
            .map_err(|e| NovaError::Package(e.to_string()))?;
        builder
            .finish()
            .map_err(|e| NovaError::Package(e.to_string()))?;
    }
    let control_tar = control_encoder
        .finish()
        .map_err(|e| NovaError::Package(e.to_string()))?;

    let members = vec![
        ArMember::new("debian-binary", b"2.0\n".to_vec()),
        ArMember {
            name: "control.tar.gz".into(),
            mode: 0o100666,
            uid: 1001,
            gid: 1001,
            mtime: 0,
            data: control_tar,
        },
        ArMember {
            name: "data.tar.gz".into(),
            mode: 0o100666,
            uid: 1001,
            gid: 1001,
            mtime: 0,
            data,
        },
    ];
    Ok(ar::write(&members))
}

/// Распаковывает `data.tar.gz` пакета в указанный каталог (используется
/// резервным установщиком и командой `nova inspect --extract`).
pub fn extract_data(bytes: &[u8], destination: &std::path::Path) -> NovaResult<usize> {
    let members = ar::read(bytes)?;
    let data_member = members
        .iter()
        .find(|m| m.name.starts_with("data.tar"))
        .ok_or_else(|| NovaError::Package("в IPK нет data.tar.gz".into()))?;
    let data = decompress_member(&data_member.data, &data_member.name)?;
    let mut archive = tar::Archive::new(&data[..]);
    std::fs::create_dir_all(destination)?;
    let mut count = 0;
    for entry in archive
        .entries()
        .map_err(|e| NovaError::Package(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| NovaError::Package(e.to_string()))?;
        entry
            .unpack_in(destination)
            .map_err(|e| NovaError::Package(e.to_string()))?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_appinfo() -> WebOsAppInfo {
        serde_json::from_str(
            r##"{
                "id": "com.novaos.demo",
                "version": "1.0.0",
                "vendor": "NovaOS",
                "type": "native",
                "main": "demo",
                "title": "Демо NovaOS",
                "icon": "icon.png",
                "largeIcon": "icon-large.png",
                "appDescription": "Демонстрационное приложение",
                "resolution": "1920x1080",
                "iconColor": "#0b0d12"
            }"##,
        )
        .unwrap()
    }

    #[test]
    fn build_then_parse_roundtrip() {
        let appinfo = sample_appinfo();
        let files = vec![
            PackageFile::new(
                "usr/palm/applications/com.novaos.demo/appinfo.json",
                serde_json::to_vec_pretty(&appinfo).unwrap(),
            ),
            PackageFile::executable(
                "usr/palm/applications/com.novaos.demo/demo",
                vec![0x7f, b'E', b'L', b'F'],
            ),
            PackageFile::new(
                "usr/palm/packages/com.novaos.demo/packageinfo.json",
                br#"{"id":"com.novaos.demo","version":"1.0.0","app":"com.novaos.demo"}"#.to_vec(),
            ),
        ];
        let control = ControlFile::new("com.novaos.demo", "1.0.0", "arm");
        let bytes = build(control, files).unwrap();

        let info = parse(&bytes).unwrap();
        assert_eq!(info.control.package, "com.novaos.demo");
        assert_eq!(info.control.architecture, "arm");
        assert_eq!(
            info.control
                .extra
                .get("webOS-Package-Format-Version")
                .map(String::as_str),
            Some("2")
        );
        assert_eq!(info.app_id(), "com.novaos.demo");
        assert_eq!(info.title(), "Демо NovaOS");
        assert!(info.is_native());
        assert_eq!(info.file_count, 3);
        assert!(info.installed_size > 0);
        assert_eq!(info.packageinfo.unwrap()["version"], "1.0.0");
        assert_eq!(info.sha256.len(), 64);
        // Детерминированность сборки.
        let again = build(
            ControlFile::new("com.novaos.demo", "1.0.0", "arm"),
            vec![
                PackageFile::new(
                    "usr/palm/applications/com.novaos.demo/appinfo.json",
                    serde_json::to_vec_pretty(&appinfo).unwrap(),
                ),
                PackageFile::executable(
                    "usr/palm/applications/com.novaos.demo/demo",
                    vec![0x7f, b'E', b'L', b'F'],
                ),
                PackageFile::new(
                    "usr/palm/packages/com.novaos.demo/packageinfo.json",
                    br#"{"id":"com.novaos.demo","version":"1.0.0","app":"com.novaos.demo"}"#
                        .to_vec(),
                ),
            ],
        )
        .unwrap();
        assert_eq!(bytes, again, "сборка IPK должна быть детерминированной");
    }

    #[test]
    fn control_parse_continuation_lines() {
        let text = "Package: com.example.app\nVersion: 2.3.4\nArchitecture: all\nDescription: первая строка\n продолжение\nwebOS-Package-Format-Version: 2\n";
        let control = ControlFile::parse(text).unwrap();
        assert_eq!(control.package, "com.example.app");
        assert_eq!(control.description, "первая строка продолжение");
        assert_eq!(control.architecture, "all");
        assert_eq!(control.extra["webOS-Package-Format-Version"], "2");
    }
}

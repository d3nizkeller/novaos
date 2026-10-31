//! Разбор Android-пакетов (`.apk`).
//!
//! NovaOS ставит IPK на webOS, но «установка приложений всех форматов» означает,
//! что пользователь может принести и APK. В этом случае NovaOS:
//!
//! * показывает честные сведения о пакете (имя, версия, SDK, разрешения, ABI);
//! * объясняет, что APK устанавливается только на Android TV;
//! * предлагает отправить пакет на подключённое Android-устройство (ADB) или
//!   сохранить его в медиатеку.
//!
//! Разбор не требует ни Android SDK, ни внешних утилит — только `zip`,
//! парсер бинарного XML ([`crate::axml`]) и таблиц ресурсов ([`crate::arsc`]).

use std::io::Read;

use crate::arsc::ResourceStrings;
use crate::axml;
use crate::error::{NovaError, NovaResult};

/// Сведения о APK-пакете.
#[derive(Debug, Clone, PartialEq)]
pub struct ApkInfo {
    /// Имя пакета (`com.example.app`).
    pub package: String,
    /// Код версии.
    pub version_code: Option<i64>,
    /// Строка версии.
    pub version_name: Option<String>,
    /// Название приложения (из манифеста или `resources.arsc`).
    pub label: Option<String>,
    /// Минимальный поддерживаемый Android API.
    pub min_sdk: Option<i64>,
    /// Целевой Android API.
    pub target_sdk: Option<i64>,
    /// Запрошенные разрешения.
    pub permissions: Vec<String>,
    /// Архитектуры из `lib/<abi>/`.
    pub abis: Vec<String>,
    /// Есть ли leanback-launcher (приложение для Android TV).
    pub leanback: bool,
    /// Есть ли обычный launcher-активити (приложение для телефона).
    pub phone_launcher: bool,
    /// Требуется ли Android TV-фича `android.software.leanback`.
    pub requires_leanback_feature: bool,
    /// Размер архива в байтах.
    pub size: u64,
    /// SHA-256 архива.
    pub sha256: String,
    /// Количество элементов в архиве.
    pub entry_count: usize,
    /// Распакованный размер.
    pub uncompressed_size: u64,
}

impl ApkInfo {
    /// Отображаемое название (с резервом на имя пакета).
    pub fn display_title(&self) -> String {
        self.label
            .clone()
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| self.package.clone())
    }

    /// Является ли пакет приложением для Android TV.
    pub fn is_tv_app(&self) -> bool {
        self.leanback || self.requires_leanback_feature
    }

    /// Человекочитаемое описание для интерфейса NovaOS.
    pub fn summary_ru(&self) -> String {
        let mut parts = Vec::new();
        if let Some(version) = &self.version_name {
            parts.push(format!("версия {version}"));
        }
        if let Some(min) = self.min_sdk {
            parts.push(format!("мин. API {min}"));
        }
        if !self.abis.is_empty() {
            parts.push(self.abis.join(", "));
        }
        if parts.is_empty() {
            "APK-пакет Android".into()
        } else {
            parts.join(" · ")
        }
    }
}

const MANIFEST: &str = "AndroidManifest.xml";
const RESOURCES: &str = "resources.arsc";
const ANDROID_NS: &str = "http://schemas.android.com/apk/res/android";

/// Разбирает APK из памяти.
pub fn parse(bytes: &[u8]) -> NovaResult<ApkInfo> {
    let sha256 = crate::net::sha256_hex(bytes);
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| NovaError::Package(format!("не удалось открыть ZIP: {e}")))?;

    let mut manifest_bytes = Vec::new();
    let mut resources_bytes: Option<Vec<u8>> = None;
    let mut abis: Vec<String> = Vec::new();
    let mut entry_count = 0usize;
    let mut uncompressed_size = 0u64;

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|e| NovaError::Package(format!("ZIP: {e}")))?;
        entry_count += 1;
        uncompressed_size = uncompressed_size.saturating_add(file.size());
        let name = file.name().to_string();
        match name.as_str() {
            MANIFEST => {
                file.read_to_end(&mut manifest_bytes)
                    .map_err(|e| NovaError::Package(e.to_string()))?;
            }
            RESOURCES => {
                // Таблица ресурсов может быть большой — читаем, только если она разумного размера.
                if file.size() <= 32 * 1024 * 1024 {
                    let mut buffer = Vec::with_capacity(file.size() as usize);
                    if file.read_to_end(&mut buffer).is_ok() {
                        resources_bytes = Some(buffer);
                    }
                }
            }
            other => {
                if let Some(abi) = other.strip_prefix("lib/") {
                    if let Some(abi) = abi.split('/').next() {
                        if !abi.is_empty() && !abis.iter().any(|a| a == abi) {
                            abis.push(abi.to_string());
                        }
                    }
                }
            }
        }
    }

    if manifest_bytes.is_empty() {
        return Err(NovaError::Package(
            "в APK отсутствует AndroidManifest.xml".into(),
        ));
    }

    let manifest = axml::parse(&manifest_bytes)?;
    let resources = resources_bytes
        .as_deref()
        .map(crate::arsc::parse)
        .unwrap_or_default();

    let package = manifest.attr_str("package").unwrap_or("").to_string();
    if package.is_empty() {
        return Err(NovaError::Package(
            "в манифесте нет атрибута package".into(),
        ));
    }

    let mut permissions: Vec<String> = Vec::new();
    for node in manifest.find_all("uses-permission") {
        if let Some(name) = node.attr("name") {
            permissions.push(value_to_string(&name.value, &resources));
        }
    }
    for node in manifest.find_all("uses-permission-sdk-23") {
        if let Some(name) = node.attr("name") {
            permissions.push(value_to_string(&name.value, &resources));
        }
    }

    let mut requires_leanback_feature = false;
    for node in manifest.find_all("uses-feature") {
        if node.attr_str("name") == Some("android.software.leanback") {
            requires_leanback_feature = true;
        }
    }

    let mut leanback = false;
    let mut phone_launcher = false;
    for activity in manifest.find_all("activity") {
        for intent in activity.find_all("intent-filter") {
            let has_main = intent
                .find_all("action")
                .iter()
                .any(|a| a.attr_str("name") == Some("android.intent.action.MAIN"));
            if !has_main {
                continue;
            }
            for category in intent.find_all("category") {
                match category.attr_str("name") {
                    Some("android.intent.category.LEANBACK_LAUNCHER") => leanback = true,
                    Some("android.intent.category.LAUNCHER") => phone_launcher = true,
                    _ => {}
                }
            }
        }
    }

    let application = manifest.find("application");
    let label = application
        .and_then(|app| app.attr("label"))
        .map(|attr| value_to_string(&attr.value, &resources))
        .filter(|l| !l.is_empty() && !l.starts_with("@0x"));

    let min_sdk = manifest
        .find("uses-sdk")
        .and_then(|node| node.attr("minSdkVersion"))
        .and_then(|attr| attr.value.as_int());
    let target_sdk = manifest
        .find("uses-sdk")
        .and_then(|node| node.attr("targetSdkVersion"))
        .and_then(|attr| attr.value.as_int());

    Ok(ApkInfo {
        package,
        version_code: manifest.attr_int("versionCode"),
        version_name: manifest.attr_str("versionName").map(str::to_string),
        label,
        min_sdk,
        target_sdk,
        permissions,
        abis,
        leanback,
        phone_launcher,
        requires_leanback_feature,
        size: bytes.len() as u64,
        sha256,
        entry_count,
        uncompressed_size,
    })
}

/// Значение атрибута как строка (с разрешением ссылок на ресурсы).
fn value_to_string(value: &axml::AttrValue, resources: &ResourceStrings) -> String {
    match value {
        axml::AttrValue::Str(s) => s.clone(),
        axml::AttrValue::Ref(id) => resources
            .get(*id)
            .map(str::to_string)
            .unwrap_or_else(|| format!("@0x{id:08x}")),
        other => other.as_text(),
    }
}

/// Проверяет, похож ли файл на APK (по именам элементов архива).
pub fn looks_like_apk(bytes: &[u8]) -> bool {
    if let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
        for index in 0..archive.len().min(64) {
            if let Ok(file) = archive.by_index(index) {
                if file.name() == MANIFEST || file.name() == "classes.dex" {
                    return true;
                }
            }
        }
    }
    false
}

/// Читает и разбирает APK с диска.
pub fn parse_file(path: &std::path::Path) -> NovaResult<ApkInfo> {
    let bytes = std::fs::read(path)?;
    parse(&bytes)
}

/// Ошибка, если Android-namespace не найден (для диагностики).
pub fn android_namespace() -> &'static str {
    ANDROID_NS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Собирает тестовый APK: манифест (бинарный XML) + classes.dex + lib/armeabi-v7a/x.so.
    fn build_test_apk() -> Vec<u8> {
        let manifest = build_manifest();
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file(MANIFEST, options).unwrap();
            zip.write_all(&manifest).unwrap();
            zip.start_file("classes.dex", options).unwrap();
            zip.write_all(b"dex\n035\0").unwrap();
            zip.start_file("lib/armeabi-v7a/libfoo.so", options)
                .unwrap();
            zip.write_all(b"\x7fELF").unwrap();
            zip.start_file("res/layout/main.xml", options).unwrap();
            zip.write_all(b"<x/>").unwrap();
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    /// Минимальный AXML-манифест с UTF-8 пулом.
    fn build_manifest() -> Vec<u8> {
        let strings = [
            "",
            "manifest",
            "package",
            "com.novaos.demo.android",
            "versionCode",
            "versionName",
            "1.2.3",
            "application",
            "label",
            "Демо Nova",
            "uses-sdk",
            "minSdkVersion",
            "uses-permission",
            "name",
            "android.permission.INTERNET",
            "uses-feature",
            "android.software.leanback",
            "activity",
            "intent-filter",
            "action",
            "android.intent.action.MAIN",
            "category",
            "android.intent.category.LEANBACK_LAUNCHER",
            ANDROID_NS,
        ];
        let mut pool_data: Vec<u8> = Vec::new();
        let mut offsets: Vec<u32> = Vec::new();
        for s in strings {
            offsets.push(pool_data.len() as u32);
            let bytes = s.as_bytes();
            pool_data.push(bytes.len() as u8); // символов
            pool_data.push(bytes.len() as u8); // байт
            pool_data.extend_from_slice(bytes);
            pool_data.push(0);
        }
        let mut pool = Vec::new();
        pool.extend_from_slice(&0x0001u16.to_le_bytes());
        pool.extend_from_slice(&28u16.to_le_bytes());
        pool.extend_from_slice(&0u32.to_le_bytes());
        pool.extend_from_slice(&(strings.len() as u32).to_le_bytes());
        pool.extend_from_slice(&0u32.to_le_bytes());
        pool.extend_from_slice(&0x100u32.to_le_bytes()); // UTF-8
        pool.extend_from_slice(&((28 + strings.len() * 4) as u32).to_le_bytes());
        pool.extend_from_slice(&0u32.to_le_bytes());
        for o in offsets {
            pool.extend_from_slice(&o.to_le_bytes());
        }
        pool.extend_from_slice(&pool_data);
        let pool_size = pool.len() as u32;
        pool[4..8].copy_from_slice(&pool_size.to_le_bytes());

        let mut body = Vec::new();
        body.extend_from_slice(&pool);

        let ns = 23u32; // ANDROID_NS
        let start = |name: u32, attrs: Vec<(u32, u32, u8, u32)>| -> Vec<u8> {
            let mut chunk = Vec::new();
            chunk.extend_from_slice(&0x0102u16.to_le_bytes());
            chunk.extend_from_slice(&16u16.to_le_bytes());
            chunk.extend_from_slice(&0u32.to_le_bytes());
            chunk.extend_from_slice(&1u32.to_le_bytes());
            chunk.extend_from_slice(&u32::MAX.to_le_bytes());
            chunk.extend_from_slice(&0u32.to_le_bytes());
            chunk.extend_from_slice(&name.to_le_bytes());
            chunk.extend_from_slice(&20u16.to_le_bytes());
            chunk.extend_from_slice(&20u16.to_le_bytes());
            chunk.extend_from_slice(&(attrs.len() as u16).to_le_bytes());
            chunk.extend_from_slice(&0u16.to_le_bytes());
            chunk.extend_from_slice(&0u16.to_le_bytes());
            chunk.extend_from_slice(&0u16.to_le_bytes());
            for (a_ns, a_name, ty, data) in attrs {
                chunk.extend_from_slice(&a_ns.to_le_bytes());
                chunk.extend_from_slice(&a_name.to_le_bytes());
                chunk.extend_from_slice(&u32::MAX.to_le_bytes());
                chunk.extend_from_slice(&8u16.to_le_bytes());
                chunk.push(0);
                chunk.push(ty);
                chunk.extend_from_slice(&data.to_le_bytes());
            }
            let size = chunk.len() as u32;
            chunk[4..8].copy_from_slice(&size.to_le_bytes());
            chunk
        };
        let end = |name: u32| -> Vec<u8> {
            let mut chunk = Vec::new();
            chunk.extend_from_slice(&0x0103u16.to_le_bytes());
            chunk.extend_from_slice(&16u16.to_le_bytes());
            chunk.extend_from_slice(&24u32.to_le_bytes());
            chunk.extend_from_slice(&1u32.to_le_bytes());
            chunk.extend_from_slice(&u32::MAX.to_le_bytes());
            chunk.extend_from_slice(&0u32.to_le_bytes());
            chunk.extend_from_slice(&name.to_le_bytes());
            chunk
        };
        const TYPE_STRING: u8 = 0x03;
        const TYPE_INT_DEC: u8 = 0x10;

        body.extend_from_slice(&start(
            1,
            vec![
                (0, 2, TYPE_STRING, 3),
                (ns, 4, TYPE_INT_DEC, 12),
                (ns, 5, TYPE_STRING, 6),
            ],
        ));
        body.extend_from_slice(&start(10, vec![(ns, 11, TYPE_INT_DEC, 21)]));
        body.extend_from_slice(&end(10));
        body.extend_from_slice(&start(12, vec![(ns, 13, TYPE_STRING, 14)]));
        body.extend_from_slice(&end(12));
        body.extend_from_slice(&start(15, vec![(ns, 13, TYPE_STRING, 16)]));
        body.extend_from_slice(&end(15));
        body.extend_from_slice(&start(7, vec![(ns, 8, TYPE_STRING, 9)]));
        body.extend_from_slice(&start(17, Vec::new()));
        body.extend_from_slice(&start(18, Vec::new()));
        body.extend_from_slice(&start(19, vec![(ns, 13, TYPE_STRING, 20)]));
        body.extend_from_slice(&end(19));
        body.extend_from_slice(&start(21, vec![(ns, 13, TYPE_STRING, 22)]));
        body.extend_from_slice(&end(21));
        body.extend_from_slice(&end(18));
        body.extend_from_slice(&end(17));
        body.extend_from_slice(&end(7));
        body.extend_from_slice(&end(1));

        let mut file = Vec::new();
        file.extend_from_slice(&0x0003u16.to_le_bytes());
        file.extend_from_slice(&8u16.to_le_bytes());
        file.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    #[test]
    fn parses_test_apk() {
        let apk = build_test_apk();
        let info = parse(&apk).expect("разбор APK");
        assert_eq!(info.package, "com.novaos.demo.android");
        assert_eq!(info.version_name.as_deref(), Some("1.2.3"));
        assert_eq!(info.version_code, Some(12));
        assert_eq!(info.min_sdk, Some(21));
        assert_eq!(info.label.as_deref(), Some("Демо Nova"));
        assert_eq!(info.display_title(), "Демо Nova");
        assert_eq!(
            info.permissions,
            vec!["android.permission.INTERNET".to_string()]
        );
        assert_eq!(info.abis, vec!["armeabi-v7a".to_string()]);
        assert!(info.leanback, "LEANBACK_LAUNCHER должен быть найден");
        assert!(info.is_tv_app());
        assert!(info.requires_leanback_feature);
        assert!(!info.phone_launcher);
        assert_eq!(info.entry_count, 4);
        assert!(info.uncompressed_size > 0);
        assert_eq!(info.sha256.len(), 64);
        assert!(info.summary_ru().contains("версия 1.2.3"));
        assert!(looks_like_apk(&apk));
    }

    #[test]
    fn rejects_non_apk() {
        assert!(parse(b"definitely not a zip archive").is_err());
        assert!(!looks_like_apk(b"nope"));
    }
}

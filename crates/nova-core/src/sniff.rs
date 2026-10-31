//! Определение формата пакета по содержимому («установка приложений всех форматов»).

use crate::error::NovaResult;

/// Формат, который NovaOS умеет обрабатывать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageFormat {
    /// Пакет webOS (`ar`-архив с `control.tar.gz` и `data.tar.gz`).
    Ipk,
    /// «Классический» opkg-пакет: gzip(tar{control.tar.gz,data.tar.gz}).
    LegacyIpk,
    /// Debian-пакет (не webOS).
    Deb,
    /// Android-пакет.
    Apk,
    /// ZIP-архив с веб-приложением (будет упакован в IPK).
    ZipWebApp,
    /// Одиночный HTML-файл (будет упакован в IPK).
    HtmlApp,
    /// Каталог/архив с `appinfo.json` — уже готовое приложение webOS.
    WebOsAppDir,
    /// Плейлист IPTV (M3U/M3U8) — создаёт источник в NovaOS.
    Playlist,
    /// Манифест репозитория `repo.json`.
    RepoManifest,
    /// Медиафайл (видео/аудио/изображение).
    Media,
    /// Неизвестный формат.
    Unknown,
}

impl PackageFormat {
    /// Русское название формата для интерфейса.
    pub fn label_ru(&self) -> &'static str {
        match self {
            PackageFormat::Ipk => "Пакет webOS (IPK)",
            PackageFormat::LegacyIpk => "Пакет webOS (opkg)",
            PackageFormat::Deb => "Пакет Debian (DEB)",
            PackageFormat::Apk => "Пакет Android (APK)",
            PackageFormat::ZipWebApp => "ZIP с веб-приложением",
            PackageFormat::HtmlApp => "Веб-приложение (HTML)",
            PackageFormat::WebOsAppDir => "Каталог приложения webOS",
            PackageFormat::Playlist => "Плейлист IPTV (M3U)",
            PackageFormat::RepoManifest => "Репозиторий (repo.json)",
            PackageFormat::Media => "Медиафайл",
            PackageFormat::Unknown => "Неизвестный формат",
        }
    }

    /// Что NovaOS сделает с таким файлом.
    pub fn action_ru(&self) -> &'static str {
        match self {
            PackageFormat::Ipk | PackageFormat::LegacyIpk => "Установить на телевизор",
            PackageFormat::WebOsAppDir => "Упаковать в IPK и установить",
            PackageFormat::ZipWebApp | PackageFormat::HtmlApp => {
                "Преобразовать в приложение webOS и установить"
            }
            PackageFormat::Playlist => "Добавить как источник каналов",
            PackageFormat::RepoManifest => "Подключить как репозиторий",
            PackageFormat::Apk => "Показать сведения (APK ставится только на Android TV)",
            PackageFormat::Deb => "Извлечь данные (DEB не поддерживается webOS)",
            PackageFormat::Media => "Добавить в медиатеку",
            PackageFormat::Unknown => "Открыть как есть не получится",
        }
    }

    /// Можно ли это установить на webOS.
    pub fn installable_on_webos(&self) -> bool {
        matches!(
            self,
            PackageFormat::Ipk
                | PackageFormat::LegacyIpk
                | PackageFormat::ZipWebApp
                | PackageFormat::HtmlApp
                | PackageFormat::WebOsAppDir
                | PackageFormat::Playlist
        )
    }
}

/// Определяет формат по первым байтам и (опционально) имени файла.
pub fn detect(bytes: &[u8], filename: Option<&str>) -> PackageFormat {
    let name = filename.unwrap_or("").to_ascii_lowercase();
    let starts_with =
        |prefix: &[u8]| bytes.len() >= prefix.len() && &bytes[..prefix.len()] == prefix;

    if starts_with(b"!<arch>\n") {
        // ar-контейнер: IPK и DEB различаются содержимым control-архива.
        return if looks_like_webos_ipk(bytes) {
            PackageFormat::Ipk
        } else {
            PackageFormat::Deb
        };
    }
    if starts_with(&[0x50, 0x4b, 0x03, 0x04]) || starts_with(&[0x50, 0x4b, 0x05, 0x06]) {
        if name.ends_with(".apk") || contains_apk_marker(bytes) {
            return PackageFormat::Apk;
        }
        return PackageFormat::ZipWebApp;
    }
    if starts_with(&[0x1f, 0x8b]) {
        if name.ends_with(".ipk") {
            return PackageFormat::LegacyIpk;
        }
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            return PackageFormat::WebOsAppDir;
        }
        return PackageFormat::Unknown;
    }
    if starts_with(b"PK") && name.ends_with(".apk") {
        return PackageFormat::Apk;
    }

    // Текстовые форматы.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let head_trim = head.trim_start();
    if head_trim.starts_with("#EXTM3U") {
        return PackageFormat::Playlist;
    }
    if head_trim.starts_with("<!DOCTYPE html")
        || head_trim.starts_with("<html")
        || head_trim.starts_with("<!doctype html")
    {
        return PackageFormat::HtmlApp;
    }
    if head_trim.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(head_trim) {
            if value.get("packages").is_some() || value.get("apps").is_some() {
                return PackageFormat::RepoManifest;
            }
            if value.get("id").is_some() && value.get("main").is_some() {
                return PackageFormat::WebOsAppDir;
            }
        }
    }
    if is_media(&name) || is_media_magic(bytes) {
        return PackageFormat::Media;
    }
    if name.ends_with(".ipk") {
        return PackageFormat::LegacyIpk;
    }
    if name.ends_with(".apk") {
        return PackageFormat::Apk;
    }
    PackageFormat::Unknown
}

fn is_media(name: &str) -> bool {
    [
        ".mp4", ".mkv", ".avi", ".mov", ".ts", ".m2ts", ".webm", ".mp3", ".flac", ".wav", ".aac",
        ".m4a", ".ogg", ".jpg", ".jpeg", ".png", ".webp",
    ]
    .iter()
    .any(|ext| name.ends_with(ext))
}

fn is_media_magic(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }
    // ftyp-контейнер (mp4/mov), Matroska EBML, RIFF, FLAC, PNG, JPEG.
    &bytes[4..8] == b"ftyp"
        || bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        || bytes.starts_with(b"RIFF")
        || bytes.starts_with(b"fLaC")
        || bytes.starts_with(&[0x89, b'P', b'N', b'G'])
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
}

/// Проверяет, есть ли в ZIP-контейнере `AndroidManifest.xml`.
fn contains_apk_marker(bytes: &[u8]) -> bool {
    windows_contain(bytes, b"AndroidManifest.xml")
}

fn looks_like_webos_ipk(bytes: &[u8]) -> bool {
    use std::io::Read;
    let Ok(members) = crate::ar::read(bytes) else {
        return false;
    };
    let has_control = members.iter().any(|m| m.name.starts_with("control.tar"));
    let Some(data) = members.iter().find(|m| m.name.starts_with("data.tar")) else {
        return has_control;
    };
    if !has_control {
        return false;
    }
    // data.tar.gz сжат, поэтому ищем маркеры webOS в распакованном начале архива:
    // имена путей лежат в tar-заголовках и видны уже в первых килобайтах.
    let mut head = Vec::new();
    let mut decoder = flate2::read::GzDecoder::new(&data.data[..]);
    let mut limited = (&mut decoder).take(64 * 1024);
    if limited.read_to_end(&mut head).is_err() && head.is_empty() {
        // Не gzip — возможно, data.tar без сжатия.
        head = data.data.iter().take(64 * 1024).copied().collect();
    }
    windows_contain(&head, b"usr/palm/") || windows_contain(&head, b"palm/applications")
}

/// Ищет подстроку в сжатых данных (gzip-поток не сжат по именам в tar-заголовках,
/// поэтому для быстрой эвристики достаточно поиска по исходным байтам).
fn windows_contain(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Определяет формат файла на диске (читает не больше 64 КБ).
pub fn detect_file(path: &std::path::Path) -> NovaResult<PackageFormat> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut head = vec![0u8; 64 * 1024];
    let read = file.read(&mut head)?;
    head.truncate(read);
    let name = path.file_name().map(|n| n.to_string_lossy().to_string());
    Ok(detect(&head, name.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_magic_bytes() {
        assert_eq!(
            detect(b"PK\x03\x04rest", Some("app.zip")),
            PackageFormat::ZipWebApp
        );
        assert_eq!(
            detect(b"PK\x03\x04rest", Some("app.apk")),
            PackageFormat::Apk
        );
        assert_eq!(
            detect(b"\x1f\x8b\x08\x00rest", Some("app.ipk")),
            PackageFormat::LegacyIpk
        );
        assert_eq!(
            detect(b"#EXTM3U\n#EXTINF:-1,Channel", None),
            PackageFormat::Playlist
        );
        assert_eq!(
            detect(b"<!DOCTYPE html><html></html>", None),
            PackageFormat::HtmlApp
        );
        assert_eq!(
            detect(br#"{"paging":{},"packages":[]}"#, Some("repo.json")),
            PackageFormat::RepoManifest
        );
        assert_eq!(
            detect(b"\x00\x00\x00\x18ftypmp42", Some("video.mp4")),
            PackageFormat::Media
        );
        assert_eq!(
            detect(b"random garbage", Some("file.bin")),
            PackageFormat::Unknown
        );
    }

    #[test]
    fn detects_real_ipk_and_deb() {
        let ipk = crate::ipk::build(
            crate::ipk::ControlFile::new("com.novaos.test", "1.0", "all"),
            vec![crate::ipk::PackageFile::new(
                "usr/palm/applications/com.novaos.test/appinfo.json",
                r#"{"id":"com.novaos.test","title":"Test","main":"index.html","type":"web"}"#
                    .as_bytes()
                    .to_vec(),
            )],
        )
        .unwrap();
        assert_eq!(
            detect(&ipk, Some("com.novaos.test_1.0_all.ipk")),
            PackageFormat::Ipk
        );

        // Тот же контейнер, но без usr/palm — это уже Debian-пакет.
        let deb = crate::ar::write(&[
            crate::ar::ArMember::new("debian-binary", b"2.0\n".to_vec()),
            crate::ar::ArMember::new("control.tar.gz", b"\x1f\x8b\x08\x00".to_vec()),
            crate::ar::ArMember::new("data.tar.gz", b"\x1f\x8b\x08\x00usr/bin/x".to_vec()),
        ]);
        assert_eq!(detect(&deb, Some("pkg.deb")), PackageFormat::Deb);
    }

    #[test]
    fn labels_and_installability() {
        assert!(PackageFormat::Ipk.installable_on_webos());
        assert!(PackageFormat::HtmlApp.installable_on_webos());
        assert!(!PackageFormat::Apk.installable_on_webos());
        assert!(!PackageFormat::Apk.label_ru().is_empty());
        assert!(!PackageFormat::Apk.action_ru().is_empty());
    }
}

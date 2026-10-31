//! Сеть и контрольные суммы.
//!
//! Хэши доступны всегда, загрузка по HTTP — только с фичей `net`
//! (на телевизоре она включена; в unit-тестах отключается, чтобы тесты
//! не зависели от сети).

use crate::error::{NovaError, NovaResult};

/// SHA-256 байтового среза в виде шестнадцатеричной строки.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    to_hex(&hasher.finalize())
}

/// SHA-256 файла (читает по кускам, чтобы не грузить большие IPK в память).
pub fn sha256_file(path: &std::path::Path) -> NovaResult<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(to_hex(&hasher.finalize()))
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Сверяет хэш без учёта регистра и возможных пробелов.
pub fn verify_checksum(expected: &str, actual: &str) -> NovaResult<()> {
    let expected = expected.trim().to_ascii_lowercase();
    let actual = actual.trim().to_ascii_lowercase();
    if expected.is_empty() {
        return Ok(());
    }
    if expected == actual {
        Ok(())
    } else {
        Err(NovaError::Checksum { expected, actual })
    }
}

/// Прогресс загрузки.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DownloadProgress {
    /// Скачано байт.
    pub received: u64,
    /// Общий размер (если сервер его сообщил).
    pub total: Option<u64>,
}

impl DownloadProgress {
    /// Доля выполнения 0.0…1.0 (0.0 если размер неизвестен).
    pub fn fraction(&self) -> f32 {
        match self.total {
            Some(total) if total > 0 => (self.received as f32 / total as f32).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

/// Результат загрузки.
#[derive(Debug, Clone)]
pub struct Downloaded {
    /// Байты файла.
    pub bytes: Vec<u8>,
    /// SHA-256 загруженного файла.
    pub sha256: String,
    /// Финальный URL (после перенаправлений).
    pub url: String,
}

/// Загрузка по HTTP(S) с прогрессом.
#[cfg(feature = "net")]
pub fn download<F>(url: &str, mut on_progress: F) -> NovaResult<Downloaded>
where
    F: FnMut(DownloadProgress),
{
    use std::time::Duration;

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(NovaError::Network(format!(
            "поддерживаются только http(s) URL: {url}"
        )));
    }
    let response = ureq::get(url)
        .timeout(Duration::from_secs(300))
        .call()
        .map_err(|e| NovaError::Network(format!("{url}: {e}")))?;
    let total = response
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut last_report = std::time::Instant::now();
    loop {
        use std::io::Read;
        let read = reader
            .read(&mut chunk)
            .map_err(|e| NovaError::Network(format!("чтение потока: {e}")))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if last_report.elapsed() >= Duration::from_millis(120) {
            last_report = std::time::Instant::now();
            on_progress(DownloadProgress {
                received: bytes.len() as u64,
                total,
            });
        }
    }
    on_progress(DownloadProgress {
        received: bytes.len() as u64,
        total: total.or(Some(bytes.len() as u64)),
    });
    let sha256 = sha256_hex(&bytes);
    Ok(Downloaded {
        bytes,
        sha256,
        url: url.to_string(),
    })
}

/// Загрузка JSON по HTTP.
#[cfg(feature = "net")]
pub fn fetch_json(url: &str) -> NovaResult<serde_json::Value> {
    let downloaded = download(url, |_| {})?;
    Ok(serde_json::from_slice(&downloaded.bytes)?)
}

/// Загрузка по HTTP без фичи `net` недоступна.
#[cfg(not(feature = "net"))]
pub fn download<F>(url: &str, _on_progress: F) -> NovaResult<Downloaded>
where
    F: FnMut(DownloadProgress),
{
    Err(NovaError::Unsupported(format!(
        "загрузка {url} недоступна: соберите с фичей `net`"
    )))
}

/// Загрузка JSON без фичи `net` недоступна.
#[cfg(not(feature = "net"))]
pub fn fetch_json(url: &str) -> NovaResult<serde_json::Value> {
    download(url, |_| {}).map(|_| serde_json::Value::Null)
}

/// Человекочитаемый размер («12,4 МБ»).
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{:.0} {}", value, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit]).replace('.', ",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn checksum_compare_is_case_insensitive() {
        assert!(verify_checksum("ABC123", "abc123").is_ok());
        assert!(verify_checksum("", "whatever").is_ok());
        assert!(matches!(
            verify_checksum("abc", "def"),
            Err(NovaError::Checksum { .. })
        ));
    }

    #[test]
    fn human_sizes() {
        assert_eq!(format_size(512), "512 Б");
        assert_eq!(format_size(2048), "2,0 КБ");
        assert_eq!(format_size(41_386_094), "39,5 МБ");
    }

    #[test]
    fn progress_fraction() {
        let p = DownloadProgress {
            received: 50,
            total: Some(200),
        };
        assert!((p.fraction() - 0.25).abs() < 1e-6);
        let unknown = DownloadProgress {
            received: 10,
            total: None,
        };
        assert_eq!(unknown.fraction(), 0.0);
    }
}

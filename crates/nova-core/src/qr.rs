//! QR-коды для установки приложений с телефона.
//!
//! Сценарий: на экране «Установка» NovaOS рисует QR-код с адресом локального
//! HTTP-приёмника (`http://<IP-телевизора>:7788/upload`). Пользователь наводит
//! камеру телефона, выбирает файл — и пакет уходит на телевизор.

use crate::error::{NovaError, NovaResult};

use qrcodegen::{QrCode, QrCodeEcc};

/// Матрица QR-кода.
#[derive(Debug, Clone, PartialEq)]
pub struct QrMatrix {
    /// Размер стороны в модулях (без полей).
    pub size: usize,
    /// Модули построчно: `true` — тёмный.
    pub modules: Vec<bool>,
    /// Рекомендуемое светлое поле (quiet zone) в модулях.
    pub quiet_zone: usize,
}

impl QrMatrix {
    /// Кодирует текст в QR-код.
    pub fn encode(text: &str) -> NovaResult<Self> {
        Self::encode_with(text, QrCodeEcc::Medium)
    }

    /// Кодирует текст с указанным уровнем коррекции ошибок.
    pub fn encode_with(text: &str, ecc: QrCodeEcc) -> NovaResult<Self> {
        if text.is_empty() {
            return Err(NovaError::Invalid("пустой текст для QR-кода".into()));
        }
        let code = QrCode::encode_text(text, ecc)
            .map_err(|_| NovaError::Invalid("текст не помещается в QR-код".into()))?;
        let size = code.size() as usize;
        let mut modules = Vec::with_capacity(size * size);
        for y in 0..code.size() {
            for x in 0..code.size() {
                modules.push(code.get_module(x, y));
            }
        }
        Ok(Self {
            size,
            modules,
            quiet_zone: 4,
        })
    }

    /// Значение модуля.
    pub fn get(&self, x: usize, y: usize) -> bool {
        if x >= self.size || y >= self.size {
            return false;
        }
        self.modules[y * self.size + x]
    }

    /// Полный размер с учётом quiet-поля.
    pub fn total_size(&self) -> usize {
        self.size + self.quiet_zone * 2
    }

    /// Значение модуля с учётом quiet-поля (вне кода — «светло»).
    pub fn get_padded(&self, x: usize, y: usize) -> bool {
        let (x, y) = (
            x as isize - self.quiet_zone as isize,
            y as isize - self.quiet_zone as isize,
        );
        if x < 0 || y < 0 {
            return false;
        }
        self.get(x as usize, y as usize)
    }

    /// Текстовое представление (для отладки в терминале).
    pub fn to_ascii(&self) -> String {
        let mut out = String::new();
        for y in 0..self.total_size() {
            for x in 0..self.total_size() {
                out.push(if self.get_padded(x, y) { '█' } else { ' ' });
            }
            out.push('\n');
        }
        out
    }
}

/// Формирует адрес локального приёмника файлов NovaOS.
pub fn upload_url(ip: Option<&str>, port: u16) -> String {
    match ip {
        Some(ip) if !ip.is_empty() => format!("http://{ip}:{port}/upload"),
        _ => format!("http://<IP-телевизора>:{port}/upload"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_url() {
        let matrix = QrMatrix::encode("http://192.168.1.42:7788/upload").unwrap();
        assert!(
            matrix.size >= 21,
            "QR должен быть не меньше 21×21, получено {}",
            matrix.size
        );
        assert_eq!(matrix.modules.len(), matrix.size * matrix.size);
        // Три поисковых маркера: левый верхний угол всегда тёмный.
        assert!(matrix.get(0, 0));
        assert!(!matrix.get_padded(0, 0), "quiet-поле должно быть светлым");
        assert_eq!(matrix.total_size(), matrix.size + 8);
        assert!(matrix.to_ascii().lines().count() == matrix.total_size());
    }

    #[test]
    fn rejects_empty_and_oversized() {
        assert!(QrMatrix::encode("").is_err());
        let long = "x".repeat(6000);
        assert!(QrMatrix::encode(&long).is_err());
    }

    #[test]
    fn upload_url_formatting() {
        assert_eq!(
            upload_url(Some("10.0.0.5"), 7788),
            "http://10.0.0.5:7788/upload"
        );
        assert_eq!(upload_url(None, 7788), "http://<IP-телевизора>:7788/upload");
    }
}

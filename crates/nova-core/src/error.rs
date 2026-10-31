//! Единый тип ошибки ядра.

use std::fmt;

/// Результат операций NovaOS.
pub type NovaResult<T> = Result<T, NovaError>;

/// Ошибки ядра NovaOS.
#[derive(Debug)]
pub enum NovaError {
    /// Ошибка ввода-вывода.
    Io(std::io::Error),
    /// Повреждённые или неподдерживаемые данные пакета.
    Package(String),
    /// Ошибка разбора JSON.
    Json(String),
    /// Ошибка сети или загрузки.
    Network(String),
    /// Ошибка проверки целостности (SHA-256 не совпал).
    Checksum { expected: String, actual: String },
    /// Ошибка шины Luna / системного сервиса.
    Luna(String),
    /// Недостаточно прав (нужен root или Homebrew Channel).
    Permission(String),
    /// Некорректные аргументы или состояние.
    Invalid(String),
    /// Операция не поддерживается на этой платформе/версии webOS.
    Unsupported(String),
    /// Операция отменена пользователем.
    Cancelled,
}

impl NovaError {
    /// Короткое человекочитаемое описание (используется в UI на русском).
    pub fn short(&self) -> &'static str {
        match self {
            NovaError::Io(_) => "Ошибка ввода-вывода",
            NovaError::Package(_) => "Ошибка пакета",
            NovaError::Json(_) => "Ошибка данных JSON",
            NovaError::Network(_) => "Ошибка сети",
            NovaError::Checksum { .. } => "Не совпала контрольная сумма",
            NovaError::Luna(_) => "Ошибка системного сервиса",
            NovaError::Permission(_) => "Недостаточно прав",
            NovaError::Invalid(_) => "Некорректные данные",
            NovaError::Unsupported(_) => "Не поддерживается",
            NovaError::Cancelled => "Отменено",
        }
    }
}

impl fmt::Display for NovaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NovaError::Io(e) => write!(f, "ввод-вывод: {e}"),
            NovaError::Package(m) => write!(f, "пакет: {m}"),
            NovaError::Json(m) => write!(f, "json: {m}"),
            NovaError::Network(m) => write!(f, "сеть: {m}"),
            NovaError::Checksum { expected, actual } => {
                write!(
                    f,
                    "контрольная сумма: ожидалось {expected}, получено {actual}"
                )
            }
            NovaError::Luna(m) => write!(f, "luna: {m}"),
            NovaError::Permission(m) => write!(f, "права: {m}"),
            NovaError::Invalid(m) => write!(f, "данные: {m}"),
            NovaError::Unsupported(m) => write!(f, "не поддерживается: {m}"),
            NovaError::Cancelled => write!(f, "отменено"),
        }
    }
}

impl std::error::Error for NovaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NovaError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for NovaError {
    fn from(e: std::io::Error) -> Self {
        NovaError::Io(e)
    }
}

impl From<serde_json::Error> for NovaError {
    fn from(e: serde_json::Error) -> Self {
        NovaError::Json(e.to_string())
    }
}

impl From<String> for NovaError {
    fn from(e: String) -> Self {
        NovaError::Invalid(e)
    }
}

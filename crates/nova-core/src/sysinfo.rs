//! Сведения об устройстве и совместимость.
//!
//! NovaOS должна честно говорить пользователю, подойдёт ли она его телевизору.
//! Поэтому здесь — определение модели/версии webOS, наличия root и Homebrew Channel,
//! а также отчёт о совместимости с пояснениями на русском.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::luna::LunaClient;

/// Платформа, на которой запущен лаунчер.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// Телевизор LG webOS.
    WebOsTv,
    /// Desktop (превью, разработка, CI).
    Desktop,
    /// Неизвестно.
    Unknown,
}

impl Target {
    /// Русское название.
    pub fn label_ru(&self) -> &'static str {
        match self {
            Target::WebOsTv => "LG webOS TV",
            Target::Desktop => "Компьютер (превью)",
            Target::Unknown => "Неизвестное устройство",
        }
    }
}

/// Сведения об устройстве.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SysInfo {
    /// Платформа.
    pub target: Target,
    /// Версия webOS из `/etc/starfish-release` (например, `4.9.1`).
    pub webos_release: Option<String>,
    /// Основная версия webOS (3, 4, 5, 6, 22, 23, 24…).
    pub webos_major: Option<u32>,
    /// Кодовое имя прошивки (`kisscurl`, `mango`…).
    pub codename: Option<String>,
    /// Модель телевизора (`UM7300PLB`).
    pub model: Option<String>,
    /// Год модели (2019 для UM73…).
    pub model_year: Option<u16>,
    /// Чипсет/SoC, если удалось определить.
    pub chipset: Option<String>,
    /// Архитектура двоичных файлов NovaOS.
    pub arch: &'static str,
    /// Имя хоста.
    pub hostname: Option<String>,
    /// IP-адрес в локальной сети (для установки по QR и SSH).
    pub ip: Option<String>,
    /// Есть ли root-доступ.
    pub rooted: bool,
    /// Установлен ли Homebrew Channel.
    pub homebrew_channel: bool,
    /// Активен ли режим разработчика.
    pub devmode: bool,
    /// Включён ли NovaOS как системный Home.
    pub nova_home_enabled: bool,
    /// Общий объём раздела приложений.
    pub storage_total_bytes: Option<u64>,
    /// Свободное место раздела приложений.
    pub storage_free_bytes: Option<u64>,
    /// Версия ядра Linux.
    pub kernel: Option<String>,
}

impl Default for SysInfo {
    fn default() -> Self {
        Self {
            target: Target::Desktop,
            webos_release: None,
            webos_major: None,
            codename: None,
            model: None,
            model_year: None,
            chipset: None,
            arch: current_arch(),
            hostname: None,
            ip: None,
            rooted: false,
            homebrew_channel: false,
            devmode: false,
            nova_home_enabled: false,
            storage_total_bytes: None,
            storage_free_bytes: None,
            kernel: None,
        }
    }
}

/// Архитектура текущей сборки.
pub fn current_arch() -> &'static str {
    #[cfg(all(target_arch = "arm", target_os = "linux"))]
    {
        "armv7 (soft-float ABI)"
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        "aarch64"
    }
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        "x86_64 (desktop)"
    }
    #[cfg(not(any(
        all(target_arch = "arm", target_os = "linux"),
        all(target_arch = "aarch64", target_os = "linux"),
        all(target_arch = "x86_64", target_os = "linux")
    )))]
    {
        "неизвестно"
    }
}

impl SysInfo {
    /// Определяет устройство: файлы webOS → Luna → эвристики.
    pub fn detect(luna: &LunaClient) -> Self {
        let mut info = Self::default();

        if let Some(text) = read_file("/etc/starfish-release") {
            let (release, codename) = parse_starfish_release(&text);
            info.webos_major = release.as_deref().and_then(major_version);
            info.webos_release = release;
            info.codename = codename;
            info.target = Target::WebOsTv;
        }
        if Path::new("/usr/palm/applications").is_dir() {
            info.target = Target::WebOsTv;
        }

        info.hostname = read_file("/etc/hostname")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        info.kernel = read_file("/proc/version")
            .and_then(|line| line.split_whitespace().nth(2).map(str::to_string));
        info.ip = detect_ip();

        // Luna отдаёт самые точные данные о модели.
        if let Some(value) = luna.system_info() {
            if let Some(model) = value.get("modelName").and_then(|v| v.as_str()) {
                info.model = Some(model.to_string());
            }
            if info.webos_release.is_none() {
                if let Some(firmware) = value.get("firmwareVersion").and_then(|v| v.as_str()) {
                    info.webos_release = Some(firmware.to_string());
                    info.webos_major = major_version(firmware);
                }
            }
            if let Some(sdk) = value.get("sdkVersion").and_then(|v| v.as_str()) {
                if info.webos_major.is_none() {
                    info.webos_major = major_version(sdk);
                }
            }
            if let Some(chipset) = value.get("chipType").and_then(|v| v.as_str()) {
                info.chipset = Some(chipset.to_string());
            }
        }
        if info.model.is_none() {
            info.model = read_file("/var/run/nyx/device_info.json")
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|value| {
                    value
                        .get("device_id")
                        .or_else(|| value.get("model_name"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                });
        }
        if let Some(model) = &info.model {
            info.model_year = model_year(model);
        }

        info.homebrew_channel = luna.homebrew_channel_present();
        info.rooted =
            Path::new("/var/lib/webosbrew").is_dir() || info.homebrew_channel || is_root();
        info.devmode =
            Path::new("/media/cryptofs/apps/usr/palm/services/com.palmdts.devmode.service")
                .is_dir();
        info.nova_home_enabled = Path::new("/var/lib/novaos-home").is_dir()
            || Path::new("/var/lib/webosbrew/init.d/50-novaos-home").exists();

        let (total, free) = storage_for("/media/cryptofs/apps");
        info.storage_total_bytes = total;
        info.storage_free_bytes = free;
        info
    }

    /// Информация о «настенном» устройстве для desktop-превью.
    ///
    /// Совпадает с реальным LG UM7300PLB (2019, webOS 4.x) — целевой моделью проекта.
    pub fn mock_um7300() -> Self {
        Self {
            target: Target::WebOsTv,
            webos_release: Some("4.9.1-70309".into()),
            webos_major: Some(4),
            codename: Some("kisscurl".into()),
            model: Some("UM7300PLB".into()),
            model_year: Some(2019),
            chipset: Some("M16P (Cortex-A9, armv7)".into()),
            arch: "armv7 (soft-float ABI)",
            hostname: Some("nova-tv".into()),
            ip: Some("192.168.1.42".into()),
            rooted: true,
            homebrew_channel: true,
            devmode: true,
            nova_home_enabled: true,
            storage_total_bytes: Some(3_221_225_472),
            storage_free_bytes: Some(1_932_735_283),
            kernel: Some("4.9.84".into()),
        }
    }

    /// Отчёт о совместимости NovaOS с этим устройством.
    pub fn compatibility(&self) -> Compatibility {
        let mut report = Compatibility::default();
        match self.target {
            Target::WebOsTv => {}
            Target::Desktop => {
                report.supported = true;
                report
                    .notes
                    .push("Запущено desktop-превью: системные функции недоступны.".into());
                return report;
            }
            Target::Unknown => {
                report
                    .warnings
                    .push("Не удалось определить платформу.".into());
                return report;
            }
        }
        match self.webos_major {
            Some(major) if (3..=6).contains(&major) => {
                report.supported = true;
                report.notes.push(format!(
                    "webOS {major}.x — поддерживается (ARMv7, soft-float)."
                ));
            }
            Some(major) if major >= 22 => {
                report.supported = true;
                report.warnings.push(format!(
                    "webOS {major} использует 64-битные процессоры: соберите NovaOS для aarch64."
                ));
            }
            Some(major) => {
                report
                    .warnings
                    .push(format!("webOS {major}.x не проверена на совместимость."));
            }
            None => report.warnings.push("Версия webOS не определена.".into()),
        }
        if !self.rooted {
            report.supported = false;
            report.warnings.push(
                "Нет root-доступа: установка приложений и замена Home невозможны. Сначала выполните root (RootMyTV / Homebrew Channel)."
                    .into(),
            );
        }
        if !self.homebrew_channel {
            report.warnings.push(
                "Homebrew Channel не найден: NovaOS будет ставить пакеты напрямую через appInstallService (нужен режим разработчика)."
                    .into(),
            );
        }
        if let (Some(total), Some(free)) = (self.storage_total_bytes, self.storage_free_bytes) {
            if free < 200 * 1024 * 1024 {
                report.warnings.push(format!(
                    "Мало свободного места: {} из {}. Освободите раздел приложений.",
                    crate::net::format_size(free),
                    crate::net::format_size(total)
                ));
            }
        }
        if report.supported && report.warnings.is_empty() {
            report
                .notes
                .push("Устройство полностью совместимо с NovaOS.".into());
        }
        report
    }

    /// Краткая строка для «О системе» в интерфейсе.
    pub fn summary(&self) -> String {
        let model = self.model.clone().unwrap_or_else(|| "LG webOS TV".into());
        match &self.webos_release {
            Some(release) => format!("{model} · webOS {release}"),
            None => model,
        }
    }
}

/// Отчёт о совместимости.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compatibility {
    /// Поддерживается ли устройство.
    pub supported: bool,
    /// Информационные сообщения.
    pub notes: Vec<String>,
    /// Предупреждения и блокирующие проблемы.
    pub warnings: Vec<String>,
}

/// Разбирает `/etc/starfish-release`.
///
/// Пример содержимого:
/// ```text
/// build_name=4.9.1-70309
/// codename=kisscurl
/// distro=k2l
/// media=flash
/// release=4.9.1
/// ```
pub fn parse_starfish_release(text: &str) -> (Option<String>, Option<String>) {
    let mut release = None;
    let mut codename = None;
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "release" => release = Some(value.trim().to_string()),
                "codename" => codename = Some(value.trim().to_string()),
                _ => {}
            }
        }
    }
    (release, codename)
}

/// Основная версия webOS из строки вида `4.9.1-70309` или `webOS TV 6.3.1`.
pub fn major_version(text: &str) -> Option<u32> {
    let digits: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Определяет год выпуска модели LG.
///
/// Правила именования LG:
/// * LCD: `[класс][год][серия]`, например `UM7300PLB` → класс `U` (UHD),
///   год — **вторая** буква (`M` = 2019, `N` = 2020, `P` = 2021, `Q` = 2022,
///   `R` = 2023, `T` = 2024);
/// * OLED: `OLED[размер][серия][год]`, например `OLED55C9PLA` → год задаёт
///   символ после буквы серии (`9` = 2019, `X` = 2020, `1` = 2021, `2` = 2022,
///   `3` = 2023, `4` = 2024).
pub fn model_year(model: &str) -> Option<u16> {
    let upper = model.to_ascii_uppercase();
    if let Some(after) = upper.split("OLED").nth(1) {
        let start = after.find(|c: char| c.is_ascii_alphabetic())?;
        let tail = &after[start..];
        let year_char = tail.chars().nth(1)?;
        return match year_char {
            '9' => Some(2019),
            'X' => Some(2020),
            '1' => Some(2021),
            '2' => Some(2022),
            '3' => Some(2023),
            '4' => Some(2024),
            '5' => Some(2025),
            _ => None,
        };
    }
    let letters: Vec<char> = upper.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    let year_letter = if letters.len() >= 2 {
        letters[1]
    } else {
        *letters.first()?
    };
    match year_letter {
        'J' => Some(2015),
        'H' => Some(2016),
        'G' | 'K' => Some(2016),
        'L' => Some(2018),
        'M' | 'C' | 'S' => Some(2019),
        'N' | 'X' => Some(2020),
        'P' => Some(2021),
        'Q' => Some(2022),
        'R' => Some(2023),
        'T' => Some(2024),
        _ => None,
    }
}

/// Версия webOS, соответствующая году выпуска модели LG.
pub fn webos_for_year(year: u16) -> &'static str {
    match year {
        ..=2015 => "webOS 2.x",
        2016 => "webOS 3.x",
        2017 => "webOS 3.5",
        2018 => "webOS 4.0",
        2019 => "webOS 4.5 / 4.9",
        2020 => "webOS 5.x",
        2021 => "webOS 6.x",
        2022 => "webOS 22",
        2023 => "webOS 23",
        _ => "webOS 24+",
    }
}

fn read_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn is_root() -> bool {
    if let Some(text) = read_file("/proc/self/status") {
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("Uid:") {
                return value.split_whitespace().next() == Some("0");
            }
        }
    }
    false
}

fn detect_ip() -> Option<String> {
    let text = read_file("/proc/net/fib_trie").unwrap_or_default();
    let mut found: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("|-- ") {
            let parts: Vec<&str> = rest.split('.').collect();
            if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
                let address = rest.to_string();
                if !address.starts_with("127.")
                    && !address.starts_with("0.")
                    && !found.contains(&address)
                {
                    found.push(address);
                }
            }
        }
    }
    found.into_iter().next()
}

fn storage_for(path: &str) -> (Option<u64>, Option<u64>) {
    // `statvfs` без зависимости от libc недоступен, поэтому используем штатный `df`.
    if let Ok(output) = std::process::Command::new("df").args(["-k", path]).output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = text.lines().nth(1) {
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() >= 4 {
                    let total = fields[1].parse::<u64>().ok().map(|v| v * 1024);
                    let free = fields[3].parse::<u64>().ok().map(|v| v * 1024);
                    return (total, free);
                }
            }
        }
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_starfish_release() {
        let (release, codename) = parse_starfish_release(
            "build_name=4.9.1-70309\ncodename=kisscurl\ndistro=k2l\nmedia=flash\nrelease=4.9.1\n",
        );
        assert_eq!(release.as_deref(), Some("4.9.1"));
        assert_eq!(codename.as_deref(), Some("kisscurl"));
        assert_eq!(major_version("4.9.1-70309"), Some(4));
        assert_eq!(major_version("webOS TV 6.3.1"), Some(6));
        assert_eq!(major_version("нет цифр"), None);
    }

    #[test]
    fn detects_um7300_year() {
        assert_eq!(model_year("UM7300PLB"), Some(2019));
        assert_eq!(model_year("UN71006LB"), Some(2020));
        assert_eq!(model_year("55UP77006LA"), Some(2021));
        assert_eq!(model_year("OLED55C9PLA"), Some(2019));
        assert_eq!(model_year("OLED65C24LA"), Some(2022));
        assert_eq!(model_year("12345"), None);
        assert_eq!(webos_for_year(2019), "webOS 4.5 / 4.9");
    }

    #[test]
    fn um7300_is_supported() {
        let info = SysInfo::mock_um7300();
        let report = info.compatibility();
        assert!(report.supported, "{:?}", report);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(report.notes.iter().any(|n| n.contains("webOS 4")));
        assert!(info.summary().contains("UM7300PLB"));
    }

    #[test]
    fn unrooted_tv_is_not_supported() {
        let mut info = SysInfo::mock_um7300();
        info.rooted = false;
        let report = info.compatibility();
        assert!(!report.supported);
        assert!(report.warnings.iter().any(|w| w.contains("root")));
    }

    #[test]
    fn desktop_target_reports_preview() {
        let info = SysInfo::default();
        let report = info.compatibility();
        assert!(report.supported);
        assert!(report.notes[0].contains("desktop"));
    }
}

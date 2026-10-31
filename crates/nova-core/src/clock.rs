//! Часы и календарь NovaOS.
//!
//! На экране телевизора время видно всегда, поэтому оно обязано быть местным.
//! В `std` нет часовых поясов, а тянуть `chrono` в прошивку armv7 не хочется —
//! здесь ровно тот минимум, который нужен: перевод Unix-времени в гражданскую
//! дату (алгоритм Хоуарда Хиннанта), разбор системного `TZif` (`/etc/localtime`)
//! и русское форматирование.

use std::path::Path;

/// Минут в часе.
const MINUTES_PER_HOUR: i32 = 60;
/// Секунд в минуте.
const SECONDS_PER_MINUTE: i64 = 60;
/// Секунд в часе.
const SECONDS_PER_HOUR: i64 = 3600;
/// Секунд в сутках.
const SECONDS_PER_DAY: i64 = 86_400;

/// Момент времени в местной зоне.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeParts {
    /// Год.
    pub year: i64,
    /// Месяц 1…12.
    pub month: u32,
    /// День 1…31.
    pub day: u32,
    /// Час 0…23.
    pub hour: u32,
    /// Минута 0…59.
    pub minute: u32,
    /// Секунда 0…59.
    pub second: u32,
    /// День недели: 0 — воскресенье, 1 — понедельник … 6 — суббота.
    pub weekday: u32,
}

impl TimeParts {
    /// Часы в 12-часовом формате (1…12).
    pub fn hour12(&self) -> u32 {
        match self.hour % 12 {
            0 => 12,
            h => h,
        }
    }

    /// До полудня ли (для «утра/вечера»).
    pub fn is_pm(&self) -> bool {
        self.hour >= 12
    }

    /// Русское название части суток.
    pub fn daypart_ru(&self) -> &'static str {
        match self.hour {
            0..=4 => "ночи",
            5..=11 => "утра",
            12..=16 => "дня",
            17..=22 => "вечера",
            _ => "ночи",
        }
    }
}

/// Текущее Unix-время в секундах.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Разбирает Unix-вмя в гражданскую дату с учётом смещения зоны в минутах.
pub fn parts_from_unix(unix: i64, offset_minutes: i32) -> TimeParts {
    let local = unix + i64::from(offset_minutes) * SECONDS_PER_MINUTE;
    let days = local.div_euclid(SECONDS_PER_DAY);
    let seconds = local.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    TimeParts {
        year,
        month,
        day,
        hour: (seconds / SECONDS_PER_HOUR) as u32,
        minute: ((seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE) as u32,
        second: (seconds % SECONDS_PER_MINUTE) as u32,
        weekday: (days + 4).rem_euclid(7) as u32,
    }
}

/// Гражданская дата из числа суток с 1970-01-01 (алгоритм Хоуарда Хиннанта).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (y + i64::from(month <= 2), month, day)
}

/// Число суток с 1970-01-01 для гражданской даты (обратная операция).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = u64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + u64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// Русское название месяца (именительный падеж).
pub fn month_name_ru(month: u32) -> &'static str {
    match month {
        1 => "январь",
        2 => "февраль",
        3 => "март",
        4 => "апрель",
        5 => "май",
        6 => "июнь",
        7 => "июль",
        8 => "август",
        9 => "сентябрь",
        10 => "октябрь",
        11 => "ноябрь",
        12 => "декабрь",
        _ => "",
    }
}

/// Русское название месяца в родительном падеже («31 октября»).
pub fn month_name_genitive_ru(month: u32) -> &'static str {
    match month {
        1 => "января",
        2 => "февраля",
        3 => "марта",
        4 => "апреля",
        5 => "мая",
        6 => "июня",
        7 => "июля",
        8 => "августа",
        9 => "сентября",
        10 => "октября",
        11 => "ноября",
        12 => "декабря",
        _ => "",
    }
}

/// Русское название дня недели.
pub fn weekday_name_ru(weekday: u32) -> &'static str {
    match weekday {
        0 => "воскресенье",
        1 => "понедельник",
        2 => "вторник",
        3 => "среда",
        4 => "четверг",
        5 => "пятница",
        6 => "суббота",
        _ => "",
    }
}

/// Время для верхней панели: «21:40» или «9:40 вечера».
pub fn format_clock(parts: &TimeParts, use_24h: bool) -> String {
    if use_24h {
        format!("{:02}:{:02}", parts.hour, parts.minute)
    } else {
        format!(
            "{}:{:02} {}",
            parts.hour12(),
            parts.minute,
            parts.daypart_ru()
        )
    }
}

/// Дата: «суббота, 31 октября».
pub fn format_date(parts: &TimeParts) -> String {
    format!(
        "{}, {} {}",
        weekday_name_ru(parts.weekday),
        parts.day,
        month_name_genitive_ru(parts.month)
    )
}

/// Короткая дата: «31.10.2026».
pub fn format_short_date(parts: &TimeParts) -> String {
    format!("{:02}.{:02}.{:04}", parts.day, parts.month, parts.year)
}

// ============================ Часовой пояс ============================

/// Часовой пояс, разобранный из `TZif`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    /// Моменты переходов (Unix-секунды).
    pub transitions: Vec<i64>,
    /// Смещения в секундах, соответствующие переходам.
    pub offsets: Vec<i32>,
    /// Смещение до первого перехода.
    pub default_offset: i32,
}

impl Zone {
    /// Разбирает файл `TZif` (версии 1, 2, 3 и 4).
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 44 || &data[0..4] != b"TZif" {
            return None;
        }
        let version = data[4];
        let v1 = read_block(data, 20, false)?;
        if version == b'1' || version == 0 {
            return Some(v1);
        }
        // Версии 2+: после блока v1 идёт блок с 64-битными временами — он точнее.
        let v1_size = block_size(data, 20, false)?;
        let v2_header = 20 + v1_size;
        if data.len() <= v2_header + 44 || &data[v2_header..v2_header + 4] != b"TZif" {
            return Some(v1);
        }
        read_block(data, v2_header + 20, true).or(Some(v1))
    }

    /// Читает `/etc/localtime`.
    pub fn from_localtime() -> Option<Self> {
        Self::from_file(Path::new("/etc/localtime"))
    }

    /// Читает файл зоны.
    pub fn from_file(path: &Path) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        Self::parse(&data)
    }

    /// Смещение в секундах для момента `unix`.
    pub fn offset_seconds(&self, unix: i64) -> i32 {
        let mut result = self.default_offset;
        for (index, transition) in self.transitions.iter().enumerate() {
            if unix < *transition {
                break;
            }
            if let Some(offset) = self.offsets.get(index) {
                result = *offset;
            }
        }
        result
    }

    /// Смещение в минутах.
    pub fn offset_minutes(&self, unix: i64) -> i32 {
        self.offset_seconds(unix) / 60
    }
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Полный размер блока данных TZif вместе с его 24-байтовым заголовком.
fn block_size(data: &[u8], header_at: usize, wide: bool) -> Option<usize> {
    let header = counts(data, header_at)?;
    let time_size = if wide { 8 } else { 4 };
    let leap_size = if wide { 12 } else { 8 };
    // 24 байта — сам заголовок блока (шесть счётчиков по 4 байта).
    Some(
        24 + header.timecnt * time_size
            + header.timecnt
            + header.typecnt * 6
            + header.charcnt
            + header.leapcnt * leap_size
            + header.isstdcnt
            + header.isutcnt,
    )
}

/// Поля заголовка блока данных TZif.
#[derive(Debug, Clone, Copy)]
struct TzCounts {
    isutcnt: usize,
    isstdcnt: usize,
    leapcnt: usize,
    timecnt: usize,
    typecnt: usize,
    charcnt: usize,
}

fn counts(data: &[u8], at: usize) -> Option<TzCounts> {
    let get = |offset: usize| be_u32(data, at + offset).map(|v| v as usize);
    Some(TzCounts {
        isutcnt: get(0)?,
        isstdcnt: get(4)?,
        leapcnt: get(8)?,
        timecnt: get(12)?,
        typecnt: get(16)?,
        charcnt: get(20)?,
    })
}

fn read_block(data: &[u8], at: usize, wide: bool) -> Option<Zone> {
    let header = counts(data, at)?;
    if header.timecnt > 4096 || header.typecnt > 64 || header.charcnt > 4096 || header.leapcnt > 64
    {
        return None;
    }
    let time_size = if wide { 8 } else { 4 };
    let mut cursor = at + 24;

    let mut transitions = Vec::with_capacity(header.timecnt);
    for _ in 0..header.timecnt {
        if wide {
            let bytes = data.get(cursor..cursor + 8)?;
            let mut buf = [0u8; 8];
            buf.copy_from_slice(bytes);
            transitions.push(i64::from_be_bytes(buf));
        } else {
            transitions.push(i64::from(be_u32(data, cursor)? as i32));
        }
        cursor += time_size;
    }

    let mut type_index = Vec::with_capacity(header.timecnt);
    for _ in 0..header.timecnt {
        type_index.push(*data.get(cursor)?);
        cursor += 1;
    }

    let mut gmtoffs = Vec::with_capacity(header.typecnt);
    let mut is_dst = Vec::with_capacity(header.typecnt);
    for _ in 0..header.typecnt {
        let bytes = data.get(cursor..cursor + 4)?;
        gmtoffs.push(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
        is_dst.push(*data.get(cursor + 4)? != 0);
        cursor += 6;
    }

    let offsets: Vec<i32> = type_index
        .iter()
        .map(|i| gmtoffs.get(usize::from(*i)).copied().unwrap_or(0))
        .collect();
    // Смещение по умолчанию: первое «зимнее», иначе первое попавшееся.
    let default_offset = is_dst
        .iter()
        .position(|dst| !dst)
        .and_then(|i| gmtoffs.get(i).copied())
        .or_else(|| gmtoffs.first().copied())
        .unwrap_or(0);

    Some(Zone {
        transitions,
        offsets,
        default_offset,
    })
}

/// Разбирает POSIX-строку `TZ`: `MSK-3`, `<+03>-3`, `EST5EDT,M3.2.0,M11.1.0`.
///
/// Знак в POSIX противоположен привычному: `MSK-3` означает «местное время =
/// UTC + 3», поэтому результат инвертируется. Летнее время не учитывается —
/// берётся стандартное смещение.
pub fn parse_posix_tz(value: &str) -> Option<i32> {
    let text = value.trim();
    if text.is_empty() || text == "UTC" || text == "GMT" {
        return Some(0);
    }
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0;
    if bytes.first() == Some(&'<') {
        index = bytes.iter().position(|c| *c == '>')? + 1;
    } else {
        while index < bytes.len() && bytes[index].is_alphabetic() {
            index += 1;
        }
    }
    if index == 0 || index >= bytes.len() {
        return None;
    }
    let rest: String = bytes[index..].iter().collect();
    let rest = rest.trim_start();
    if rest.is_empty() {
        return None;
    }
    let (sign, rest) = match rest.chars().next() {
        Some('-') => (-1, &rest[1..]),
        Some('+') => (1, &rest[1..]),
        _ => (1, rest),
    };
    // Числовая часть заканчивается там, где начинается имя летней зоны или правило.
    let numeric: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ':')
        .collect();
    let hours: i32 = numeric
        .split(':')
        .next()
        .unwrap_or("")
        .parse()
        .ok()
        .filter(|h: &i32| h.abs() <= 24)?;
    let minutes: i32 = numeric
        .split(':')
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let posix = sign * (hours * MINUTES_PER_HOUR + minutes);
    Some(-posix)
}

/// Смещение системного часового пояса в минутах.
///
/// Порядок: переменная `NOVA_TZ_OFFSET` (для разработки) → POSIX-строка `TZ` →
/// `/etc/localtime`. Если ничего не найдено — UTC.
pub fn system_offset_minutes() -> i32 {
    system_offset_minutes_at(now_unix())
}

/// То же, но для указанного момента (удобно тестировать).
pub fn system_offset_minutes_at(unix: i64) -> i32 {
    if let Ok(value) = std::env::var("NOVA_TZ_OFFSET") {
        if let Ok(minutes) = value.trim().parse::<i32>() {
            return minutes;
        }
    }
    if let Ok(value) = std::env::var("TZ") {
        if let Some(minutes) = parse_posix_tz(&value) {
            return minutes;
        }
        // Имя зоны («Europe/Moscow») — пробуем найти её файл.
        for root in ["/usr/share/zoneinfo", "/etc/zoneinfo"] {
            let candidate = Path::new(root).join(value.trim_start_matches('/'));
            if let Some(zone) = Zone::from_file(&candidate) {
                return zone.offset_minutes(unix);
            }
        }
    }
    Zone::from_localtime()
        .map(|zone| zone.offset_minutes(unix))
        .unwrap_or(0)
}

/// Местное время «здесь и сейчас».
pub fn local_now(use_24h: bool) -> (TimeParts, String, String) {
    let offset = system_offset_minutes();
    let parts = parts_from_unix(now_unix(), offset);
    let clock = format_clock(&parts, use_24h);
    let date = format_date(&parts);
    (parts, clock, date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_is_1970_thursday() {
        let parts = parts_from_unix(0, 0);
        assert_eq!((parts.year, parts.month, parts.day), (1970, 1, 1));
        assert_eq!((parts.hour, parts.minute, parts.second), (0, 0, 0));
        assert_eq!(parts.weekday, 4, "1 января 1970 — четверг");
    }

    #[test]
    fn known_timestamps() {
        // 2023-11-14 22:13:20 UTC.
        let utc = parts_from_unix(1_700_000_000, 0);
        assert_eq!((utc.year, utc.month, utc.day), (2023, 11, 14));
        assert_eq!((utc.hour, utc.minute, utc.second), (22, 13, 20));
        // Москва (+3): уже 15 ноября, 01:13:20.
        let msk = parts_from_unix(1_700_000_000, 180);
        assert_eq!((msk.year, msk.month, msk.day), (2023, 11, 15));
        assert_eq!((msk.hour, msk.minute), (1, 13));
        // Отрицательное смещение (Нью-Йорк, -5).
        let ny = parts_from_unix(1_700_000_000, -300);
        assert_eq!((ny.day, ny.hour), (14, 17));
    }

    #[test]
    fn civil_roundtrip() {
        for (year, month, day) in [
            (1970, 1, 1),
            (1969, 12, 31),
            (2000, 2, 29),
            (2019, 5, 6),
            (2026, 10, 31),
            (2100, 3, 1),
        ] {
            let days = days_from_civil(year, month, day);
            let (y, m, d) = civil_from_days(days);
            assert_eq!(
                (y, m, d),
                (year, month, day),
                "сбой для {year}-{month}-{day}"
            );
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
    }

    #[test]
    fn formatting_is_russian() {
        let parts = TimeParts {
            year: 2026,
            month: 10,
            day: 31,
            hour: 21,
            minute: 40,
            second: 0,
            weekday: 6,
        };
        assert_eq!(format_clock(&parts, true), "21:40");
        assert_eq!(format_clock(&parts, false), "9:40 вечера");
        assert_eq!(format_date(&parts), "суббота, 31 октября");
        assert_eq!(format_short_date(&parts), "31.10.2026");
        let morning = TimeParts {
            hour: 7,
            minute: 5,
            ..parts
        };
        assert_eq!(format_clock(&morning, false), "7:05 утра");
        assert_eq!(morning.hour12(), 7);
        let noon = TimeParts {
            hour: 12,
            minute: 0,
            ..parts
        };
        assert_eq!(noon.hour12(), 12);
        assert!(noon.is_pm());
        let midnight = TimeParts {
            hour: 0,
            minute: 0,
            ..parts
        };
        assert_eq!(midnight.hour12(), 12);
        assert_eq!(midnight.daypart_ru(), "ночи");
    }

    #[test]
    fn parses_posix_tz() {
        assert_eq!(parse_posix_tz("MSK-3"), Some(180));
        assert_eq!(parse_posix_tz("<+03>-3"), Some(180));
        assert_eq!(parse_posix_tz("EST5EDT,M3.2.0,M11.1.0"), Some(-300));
        assert_eq!(parse_posix_tz("UTC"), Some(0));
        assert_eq!(parse_posix_tz("IST-5:30"), Some(330));
        assert_eq!(parse_posix_tz("Europe/Moscow"), None);
        assert_eq!(parse_posix_tz(""), Some(0));
    }

    /// Собирает TZif2 для зоны со смещением `gmtoff` и одним переходом.
    fn synthetic_tzif(gmtoff: i32, abbr: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let chars: Vec<u8> = abbr.bytes().chain(std::iter::once(0)).collect();
        let write_block = |out: &mut Vec<u8>, wide: bool| {
            // Заголовок блока: ttisutcnt, ttisstdcnt, leapcnt, timecnt, typecnt, charcnt.
            for value in [0u32, 0u32, 0u32, 1u32, 1u32, chars.len() as u32] {
                out.extend_from_slice(&value.to_be_bytes());
            }
            if wide {
                out.extend_from_slice(&0i64.to_be_bytes());
            } else {
                out.extend_from_slice(&0i32.to_be_bytes());
            }
            out.push(0); // индекс типа для перехода
            out.extend_from_slice(&gmtoff.to_be_bytes());
            out.push(0); // isdst
            out.push(0); // abbrind
            out.extend_from_slice(&chars);
        };
        out.extend_from_slice(b"TZif");
        out.push(b'2');
        out.extend_from_slice(&[0u8; 15]);
        write_block(&mut out, false);
        out.extend_from_slice(b"TZif");
        out.push(b'2');
        out.extend_from_slice(&[0u8; 15]);
        write_block(&mut out, true);
        out.extend_from_slice(b"\nMSK0\n");
        out
    }

    #[test]
    fn parses_tzif2() {
        let data = synthetic_tzif(10_800, "MSK");
        let zone = Zone::parse(&data).expect("разбор TZif2");
        assert_eq!(zone.default_offset, 10_800);
        assert_eq!(zone.transitions, vec![0]);
        assert_eq!(zone.offset_seconds(1_700_000_000), 10_800);
        assert_eq!(zone.offset_minutes(1_700_000_000), 180);
        // До перехода действует смещение по умолчанию.
        assert_eq!(zone.offset_seconds(-1), 10_800);
    }

    #[test]
    fn parses_tzif1_and_rejects_junk() {
        // Версия 1: файл состоит из одного блока с 32-битными временами.
        let mut v1 = synthetic_tzif(-18_000, "EST");
        v1[4] = b'1';
        let cut = 20 + block_size(&v1, 20, false).unwrap();
        v1.truncate(cut);
        let zone = Zone::parse(&v1).expect("разбор TZif1");
        assert_eq!(zone.default_offset, -18_000);
        assert_eq!(zone.offset_minutes(1_700_000_000), -300);

        assert!(Zone::parse(b"not a timezone file at all").is_none());
        assert!(Zone::parse(&[]).is_none());
        // Обрезанный TZif2 не должен ронять разбор.
        let mut broken = synthetic_tzif(3600, "CET");
        let tail = broken.len();
        broken.truncate(tail - 10);
        let _ = Zone::parse(&broken);
    }

    #[test]
    fn system_offset_is_sane() {
        let offset = system_offset_minutes();
        assert!((-1_000..=1_000).contains(&offset), "смещение {offset}");
        if let Some(zone) = Zone::from_localtime() {
            let seconds = zone.offset_seconds(now_unix());
            assert!(seconds.abs() <= 14 * 3600, "нереальное смещение {seconds}");
        }
        let (parts, clock, date) = local_now(true);
        assert!(parts.year >= 2024);
        assert_eq!(clock.len(), 5);
        assert!(date.contains(',') || date.len() > 5);
    }
}

//! Разбор таблиц ресурсов Android (`resources.arsc`).
//!
//! Нужен ровно для одного: получить человекочитаемое название приложения,
//! когда `android:label` в манифесте — ссылка на ресурс (`@string/app_name`).
//!
//! Формат: таблица (0x0002) → глобальный пул строк значений (0x0001) →
//! пакеты (0x0200) → типы (0x0201) → записи с `Res_value`.
//! Идентификатор ресурса: `packageId << 24 | typeId << 16 | entryIndex`.

use std::collections::BTreeMap;

use crate::error::{NovaError, NovaResult};

const CHUNK_STRING_POOL: u16 = 0x0001;
const CHUNK_TABLE: u16 = 0x0002;
const CHUNK_PACKAGE: u16 = 0x0200;
const CHUNK_TYPE: u16 = 0x0201;

const FLAG_UTF8: u32 = 1 << 8;
const FLAG_COMPLEX: u16 = 0x0001;
const TYPE_STRING: u8 = 0x03;

const NO_ENTRY: u32 = 0xFFFF_FFFF;

/// Карта «идентификатор ресурса → строка».
#[derive(Debug, Clone, Default)]
pub struct ResourceStrings {
    map: BTreeMap<u32, String>,
}

impl ResourceStrings {
    /// Пустая таблица.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Строка по идентификатору ресурса.
    pub fn get(&self, id: u32) -> Option<&str> {
        self.map.get(&id).map(String::as_str)
    }

    /// Количество найденных строк.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Пуста ли таблица.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn insert(&mut self, id: u32, value: String) {
        self.map.entry(id).or_insert(value);
    }
}

fn u16_at(data: &[u8], pos: usize) -> Option<u16> {
    let b = data.get(pos..pos + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(data: &[u8], pos: usize) -> Option<u32> {
    let b = data.get(pos..pos + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Разбирает `resources.arsc`.
///
/// Функция не падает на повреждённых данных: название приложения — не критичная
/// информация, поэтому при любой ошибке возвращается пустая таблица.
pub fn parse(data: &[u8]) -> ResourceStrings {
    parse_inner(data).unwrap_or_default()
}

fn parse_inner(data: &[u8]) -> NovaResult<ResourceStrings> {
    let mut map = ResourceStrings::default();
    let table_type =
        u16_at(data, 0).ok_or_else(|| NovaError::Package("ARSC: короткая таблица".into()))?;
    let table_header = u16_at(data, 2).unwrap_or(12) as usize;
    if table_type != CHUNK_TABLE {
        return Err(NovaError::Package("ARSC: неверный тип таблицы".into()));
    }
    let mut values: Vec<String> = Vec::new();
    let mut pos = table_header;
    while pos + 8 <= data.len() {
        let chunk_type = u16_at(data, pos).unwrap_or(0);
        let chunk_size = u32_at(data, pos + 4).unwrap_or(0) as usize;
        if chunk_size < 8 || pos + chunk_size > data.len() {
            break;
        }
        match chunk_type {
            CHUNK_STRING_POOL if values.is_empty() => {
                values = read_string_pool(data, pos)?;
            }
            CHUNK_PACKAGE => {
                parse_package(data, pos, chunk_size, &values, &mut map)?;
            }
            _ => {}
        }
        pos += chunk_size;
    }
    Ok(map)
}

fn parse_package(
    data: &[u8],
    start: usize,
    size: usize,
    values: &[String],
    map: &mut ResourceStrings,
) -> NovaResult<()> {
    let header_size = u16_at(data, start + 2).unwrap_or(0x0120) as usize;
    let package_id = u32_at(data, start + 8).unwrap_or(0x7f);
    let end = start + size;
    let mut sub = start + header_size.max(288);
    while sub + 8 <= end {
        let chunk_type = u16_at(data, sub).unwrap_or(0);
        let chunk_size = u32_at(data, sub + 4).unwrap_or(0) as usize;
        if chunk_size < 8 || sub + chunk_size > end {
            break;
        }
        if chunk_type == CHUNK_TYPE {
            parse_type(data, sub, end, package_id, values, map);
        }
        sub += chunk_size;
    }
    Ok(())
}

fn parse_type(
    data: &[u8],
    start: usize,
    package_end: usize,
    package_id: u32,
    values: &[String],
    map: &mut ResourceStrings,
) {
    let Some(header_size) = u16_at(data, start + 2).map(usize::from) else {
        return;
    };
    let Some(type_id) = data.get(start + 8).copied() else {
        return;
    };
    let Some(entry_count) = u32_at(data, start + 12) else {
        return;
    };
    let Some(entries_start) = u32_at(data, start + 16) else {
        return;
    };
    if entry_count > 100_000 {
        return;
    }
    let offsets_base = start + header_size;
    let data_base = start + entries_start as usize;
    for index in 0..entry_count {
        let Some(offset) = u32_at(data, offsets_base + index as usize * 4) else {
            break;
        };
        if offset == NO_ENTRY {
            continue;
        }
        let entry = data_base + offset as usize;
        let Some(entry_size) = u16_at(data, entry).map(usize::from) else {
            continue;
        };
        let Some(flags) = u16_at(data, entry + 2) else {
            continue;
        };
        if flags & FLAG_COMPLEX != 0 {
            continue;
        }
        let value = entry + entry_size.max(8);
        if value + 8 > package_end {
            continue;
        }
        let Some(data_type) = data.get(value + 3).copied() else {
            continue;
        };
        let Some(payload) = u32_at(data, value + 4) else {
            continue;
        };
        if data_type == TYPE_STRING {
            if let Some(text) = values.get(payload as usize) {
                if !text.is_empty() {
                    let id = (package_id << 24) | ((type_id as u32) << 16) | index;
                    map.insert(id, text.clone());
                }
            }
        }
    }
}

/// Читает `ResStringPool` (общий формат для AXML и ARSC).
pub(crate) fn read_string_pool(data: &[u8], chunk_start: usize) -> NovaResult<Vec<String>> {
    let header_size = u16_at(data, chunk_start + 2).unwrap_or(28) as usize;
    let string_count = u32_at(data, chunk_start + 8).unwrap_or(0) as usize;
    let flags = u32_at(data, chunk_start + 16).unwrap_or(0);
    let strings_start = u32_at(data, chunk_start + 20).unwrap_or(0) as usize;
    if string_count > 200_000 || header_size < 28 {
        return Err(NovaError::Package(
            "ARSC: некорректный строковый пул".into(),
        ));
    }
    let utf8 = flags & FLAG_UTF8 != 0;
    let base = chunk_start + strings_start;
    let mut out = Vec::with_capacity(string_count);
    for i in 0..string_count {
        let offset = u32_at(data, chunk_start + header_size + i * 4).unwrap_or(0) as usize;
        out.push(decode_string(data, base + offset, utf8));
    }
    Ok(out)
}

fn decode_string(data: &[u8], start: usize, utf8: bool) -> String {
    if start >= data.len() {
        return String::new();
    }
    let mut pos = start;
    if utf8 {
        // Первый счётчик — количество символов, второй — количество байт.
        let first = data[pos] as usize;
        pos += 1;
        if first & 0x80 != 0 {
            pos += 1;
        }
        let second = data.get(pos).copied().unwrap_or(0) as usize;
        pos += 1;
        let len = if second & 0x80 != 0 {
            let hi = (second & 0x7f) << 8;
            let lo = data.get(pos).copied().unwrap_or(0) as usize;
            pos += 1;
            hi | lo
        } else {
            second
        };
        let end = (pos + len).min(data.len());
        String::from_utf8_lossy(&data[pos..end]).to_string()
    } else {
        let low = u16_at(data, pos).unwrap_or(0) as usize;
        pos += 2;
        let len = if low & 0x8000 != 0 {
            let hi = (low & 0x7fff) << 16;
            let lo = u16_at(data, pos).unwrap_or(0) as usize;
            pos += 2;
            hi | lo
        } else {
            low
        };
        let mut out = String::with_capacity(len);
        for i in 0..len {
            let Some(unit) = u16_at(data, pos + i * 2) else {
                break;
            };
            if unit == 0 {
                break;
            }
            out.push(char::from_u32(unit as u32).unwrap_or('\u{fffd}'));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Собирает минимальный `resources.arsc`: 0x7f010000 -> "Демо Nova".
    fn sample_arsc() -> Vec<u8> {
        // Пул строк значений (UTF-8): одна строка "Демо Nova".
        let text = "Демо Nova".as_bytes();
        let mut pool_body: Vec<u8> = Vec::new();
        pool_body.push(1); // количество символов
        pool_body.push(text.len() as u8); // количество байт
        pool_body.extend_from_slice(text);
        pool_body.push(0);
        let pool_header = 28usize;
        let strings_start = pool_header + 4; // одно смещение
        let mut pool = Vec::new();
        pool.extend_from_slice(&CHUNK_STRING_POOL.to_le_bytes());
        pool.extend_from_slice(&(pool_header as u16).to_le_bytes());
        pool.extend_from_slice(&0u32.to_le_bytes()); // размер (заполним ниже)
        pool.extend_from_slice(&1u32.to_le_bytes()); // stringCount
        pool.extend_from_slice(&0u32.to_le_bytes()); // styleCount
        pool.extend_from_slice(&(FLAG_UTF8).to_le_bytes()); // flags
        pool.extend_from_slice(&(strings_start as u32).to_le_bytes());
        pool.extend_from_slice(&0u32.to_le_bytes()); // stylesStart
        pool.extend_from_slice(&0u32.to_le_bytes()); // offset[0]
        pool.extend_from_slice(&pool_body);
        let pool_size = pool.len() as u32;
        pool[4..8].copy_from_slice(&pool_size.to_le_bytes());

        // Чанк типа (0x0201): одна запись со строковым значением.
        let type_header = 84usize; // минимальный ResTable_type header
        let mut type_chunk = Vec::new();
        type_chunk.extend_from_slice(&CHUNK_TYPE.to_le_bytes());
        type_chunk.extend_from_slice(&(type_header as u16).to_le_bytes());
        type_chunk.extend_from_slice(&0u32.to_le_bytes()); // size
        type_chunk.push(1); // id
        type_chunk.push(0); // res0
        type_chunk.extend_from_slice(&0u16.to_le_bytes()); // res1
        type_chunk.extend_from_slice(&1u32.to_le_bytes()); // entryCount
        type_chunk.extend_from_slice(&((type_header + 4) as u32).to_le_bytes()); // entriesStart
        type_chunk.extend_from_slice(&vec![0u8; 64]); // config (не используется)
        debug_assert_eq!(type_chunk.len(), type_header);
        type_chunk.extend_from_slice(&0u32.to_le_bytes()); // offsets[0]
                                                           // Запись: size=8, flags=0, key=0, затем Res_value{size=8,res0=0,dataType=3,data=0}
        type_chunk.extend_from_slice(&8u16.to_le_bytes());
        type_chunk.extend_from_slice(&0u16.to_le_bytes());
        type_chunk.extend_from_slice(&0u32.to_le_bytes());
        type_chunk.extend_from_slice(&8u16.to_le_bytes());
        type_chunk.push(0);
        type_chunk.push(TYPE_STRING);
        type_chunk.extend_from_slice(&0u32.to_le_bytes()); // индекс строки
        let type_size = type_chunk.len() as u32;
        type_chunk[4..8].copy_from_slice(&type_size.to_le_bytes());

        // Пакет (0x0200): заголовок 288 байт + чанк типа.
        let package_header = 288usize;
        let mut package = vec![0u8; package_header];
        package[0..2].copy_from_slice(&CHUNK_PACKAGE.to_le_bytes());
        package[2..4].copy_from_slice(&(package_header as u16).to_le_bytes());
        let package_size = (package_header + type_chunk.len()) as u32;
        package[4..8].copy_from_slice(&package_size.to_le_bytes());
        package[8..12].copy_from_slice(&0x7fu32.to_le_bytes()); // id пакета
        package.extend_from_slice(&type_chunk);

        // Таблица: заголовок + пул строк значений + пакет.
        let mut table = Vec::new();
        table.extend_from_slice(&CHUNK_TABLE.to_le_bytes());
        table.extend_from_slice(&12u16.to_le_bytes());
        let table_size = (12 + pool.len() + package.len()) as u32;
        table.extend_from_slice(&table_size.to_le_bytes());
        table.extend_from_slice(&1u32.to_le_bytes()); // packageCount
        table.extend_from_slice(&pool);
        table.extend_from_slice(&package);
        table
    }

    #[test]
    fn resolves_label_resource() {
        let data = sample_arsc();
        let parsed = parse(&data);
        assert_eq!(parsed.get(0x7f010000), Some("Демо Nova"));
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn garbage_yields_empty_map() {
        let parsed = parse(b"not an arsc table at all");
        assert!(parsed.is_empty());
        assert_eq!(parsed.get(0x7f010000), None);
        assert!(ResourceStrings::empty().is_empty());
    }
}

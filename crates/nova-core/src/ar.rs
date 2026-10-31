//! Чтение и запись архивов `ar`.
//!
//! Пакет webOS `.ipk` — это обычный Debian binary package формата 2.0, то есть
//! `ar`-архив из трёх элементов:
//!
//! ```text
//! debian-binary   -> "2.0\n"
//! control.tar.gz  -> ./control (+ опционально preinst/postinst)
//! data.tar.gz     -> ./usr/palm/applications/<id>/...
//! ```
//!
//! Модуль намеренно не тянет внешних зависимостей: формат прост, а нам важно,
//! чтобы код собирался под `armv7-unknown-linux-gnueabi` (soft-float).

use crate::error::{NovaError, NovaResult};

/// Глобальный заголовок `ar`.
pub const AR_MAGIC: &[u8; 8] = b"!<arch>\n";

/// Размер заголовка элемента (в байтах).
const HEADER_LEN: usize = 60;

/// Элемент `ar`-архива.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArMember {
    /// Имя элемента (например, `control.tar.gz`).
    pub name: String,
    /// Время изменения (UNIX, секунды).
    pub mtime: u64,
    /// UID владельца.
    pub uid: u32,
    /// GID владельца.
    pub gid: u32,
    /// Права доступа (восьмеричное значение, например 0o100644).
    pub mode: u32,
    /// Содержимое.
    pub data: Vec<u8>,
}

impl ArMember {
    /// Новый элемент с типовыми правами обычного файла.
    pub fn new(name: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            mtime: 0,
            uid: 0,
            gid: 0,
            mode: 0o100644,
            data,
        }
    }
}

fn parse_field(raw: &[u8], radix: u32) -> u64 {
    let text = String::from_utf8_lossy(raw);
    let text = text.trim().trim_end_matches('\0');
    if text.is_empty() {
        return 0;
    }
    u64::from_str_radix(text, radix).unwrap_or(0)
}

fn write_field(out: &mut Vec<u8>, value: &str, width: usize) {
    out.extend_from_slice(value.as_bytes());
    for _ in value.len()..width {
        out.push(b' ');
    }
}

/// Разбирает `ar`-архив целиком в память.
///
/// Поддерживает короткие имена в стиле GNU (`name/`) и таблицу длинных имён `//`.
/// Служебные элементы (таблица символов `/`, `/SYM64/`, `//`) в результат не попадают.
pub fn read(bytes: &[u8]) -> NovaResult<Vec<ArMember>> {
    if bytes.len() < AR_MAGIC.len() || &bytes[..AR_MAGIC.len()] != AR_MAGIC {
        return Err(NovaError::Package(
            "файл не является ar-архивом (IPK)".into(),
        ));
    }
    let mut offset = AR_MAGIC.len();
    let mut long_names: Vec<u8> = Vec::new();
    let mut members = Vec::new();

    while offset + HEADER_LEN <= bytes.len() {
        let header = &bytes[offset..offset + HEADER_LEN];
        if &header[58..60] != b"`\n" {
            return Err(NovaError::Package(
                "повреждён заголовок элемента ar-архива".into(),
            ));
        }
        let raw_name = String::from_utf8_lossy(&header[0..16])
            .trim_end()
            .to_string();
        let size = parse_field(&header[48..58], 10) as usize;
        let mtime = parse_field(&header[16..28], 10);
        let uid = parse_field(&header[28..34], 10) as u32;
        let gid = parse_field(&header[34..40], 10) as u32;
        let mode = parse_field(&header[40..48], 8) as u32;

        let start = offset + HEADER_LEN;
        let end = start
            .checked_add(size)
            .ok_or_else(|| NovaError::Package("переполнение размера элемента ar".into()))?;
        if end > bytes.len() {
            return Err(NovaError::Package("ar-архив обрезан".into()));
        }
        let data = &bytes[start..end];

        let name = match raw_name.as_str() {
            "/" | "/SYM64/" | "__.SYMDEF" => String::new(),
            "//" | "/ARFILENAMES/" => {
                long_names = data.to_vec();
                String::new()
            }
            other => {
                if let Some(index) = other.strip_prefix('/') {
                    // GNU: смещение в таблице длинных имён.
                    let from: usize = index.parse().unwrap_or(usize::MAX);
                    let slice = long_names.get(from..).unwrap_or(b"");
                    let stop = slice
                        .iter()
                        .position(|b| *b == b'\n' || *b == b'/')
                        .unwrap_or(slice.len());
                    String::from_utf8_lossy(&slice[..stop])
                        .trim_end()
                        .to_string()
                } else {
                    other.trim_end_matches('/').to_string()
                }
            }
        };

        if !name.is_empty() {
            members.push(ArMember {
                name,
                mtime,
                uid,
                gid,
                mode,
                data: data.to_vec(),
            });
        }
        offset = end + (size % 2); // выравнивание по чётной границе
    }
    Ok(members)
}

/// Собирает `ar`-архив из элементов.
pub fn write(members: &[ArMember]) -> Vec<u8> {
    let mut out = Vec::with_capacity(members.iter().map(|m| m.data.len() + HEADER_LEN + 1).sum());
    out.extend_from_slice(AR_MAGIC);
    for member in members {
        let mut header = Vec::with_capacity(HEADER_LEN);
        let mut name = member.name.clone();
        name.truncate(15);
        write_field(&mut header, &format!("{name}/"), 16);
        write_field(&mut header, &member.mtime.to_string(), 12);
        write_field(&mut header, &member.uid.to_string(), 6);
        write_field(&mut header, &member.gid.to_string(), 6);
        write_field(&mut header, &format!("{:o}", member.mode), 8);
        write_field(&mut header, &member.data.len().to_string(), 10);
        header.extend_from_slice(b"`\n");
        debug_assert_eq!(header.len(), HEADER_LEN);
        out.extend_from_slice(&header);
        out.extend_from_slice(&member.data);
        if member.data.len() % 2 == 1 {
            out.push(b'\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let members = vec![
            ArMember::new("debian-binary", b"2.0\n".to_vec()),
            ArMember::new("control.tar.gz", vec![1, 2, 3, 4, 5]),
            ArMember::new("data.tar.gz", b"payload".to_vec()),
        ];
        let bytes = write(&members);
        assert_eq!(&bytes[..8], AR_MAGIC);
        let parsed = read(&bytes).expect("чтение ar");
        assert_eq!(parsed, members);
    }

    #[test]
    fn rejects_foreign_file() {
        assert!(read(b"PK\x03\x04 not an ar archive").is_err());
    }

    #[test]
    fn gnu_long_names() {
        // Ручная сборка: таблица "//" + элемент со ссылкой "/0".
        let mut raw = Vec::from(AR_MAGIC.as_slice());
        let long = b"very-long-member-name.tar.gz\n";
        for (name, size, data) in [("//", long.len(), &long[..]), ("/0", 3usize, &b"abc"[..])] {
            let mut header = Vec::new();
            write_field(&mut header, name, 16);
            write_field(&mut header, "0", 12);
            write_field(&mut header, "0", 6);
            write_field(&mut header, "0", 6);
            write_field(&mut header, "0", 8);
            write_field(&mut header, &size.to_string(), 10);
            header.extend_from_slice(b"`\n");
            raw.extend_from_slice(&header);
            raw.extend_from_slice(data);
            if size % 2 == 1 {
                raw.push(b'\n');
            }
        }
        let parsed = read(&raw).expect("чтение длинных имён");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "very-long-member-name.tar.gz");
    }
}

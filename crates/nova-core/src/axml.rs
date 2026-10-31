//! Разбор бинарного XML Android (`AndroidManifest.xml`).
//!
//! Формат — последовательность чанков `ResChunk_header { type: u16, headerSize: u16, size: u32 }`:
//! строковый пул (0x0001), карта ресурсов (0x0180), начало/конец namespace (0x0100/0x0101),
//! начало/конец элемента (0x0102/0x0103), текст (0x0104).
//!
//! Парсер намеренно «прощающий»: любые неожиданности приводят к ошибке разбора,
//! а не к панике, потому что APK в дикой природе бывают повреждёнными.

use crate::error::{NovaError, NovaResult};

const CHUNK_STRING_POOL: u16 = 0x0001;
const CHUNK_RESOURCE_MAP: u16 = 0x0180;
const CHUNK_XML: u16 = 0x0003;
const CHUNK_START_NAMESPACE: u16 = 0x0100;
const CHUNK_END_NAMESPACE: u16 = 0x0101;
const CHUNK_CDATA: u16 = 0x0104;
const CHUNK_START_ELEMENT: u16 = 0x0102;
const CHUNK_END_ELEMENT: u16 = 0x0103;

const TYPE_NULL: u8 = 0x00;
const TYPE_REFERENCE: u8 = 0x01;
const TYPE_ATTRIBUTE: u8 = 0x02;
const TYPE_STRING: u8 = 0x03;
const TYPE_FLOAT: u8 = 0x04;
const TYPE_INT_DEC: u8 = 0x10;
const TYPE_INT_HEX: u8 = 0x11;
const TYPE_INT_BOOLEAN: u8 = 0x12;

/// Значение атрибута.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    /// Строка.
    Str(String),
    /// Целое число.
    Int(i64),
    /// Логическое значение.
    Bool(bool),
    /// Ссылка на ресурс (`@0x7f0a0001`).
    Ref(u32),
    /// Число с плавающей точкой.
    Float(f32),
    /// Прочее (тип + сырые данные).
    Raw(u8, u32),
}

impl AttrValue {
    /// Значение как строка (для показа в интерфейсе).
    pub fn as_text(&self) -> String {
        match self {
            AttrValue::Str(s) => s.clone(),
            AttrValue::Int(i) => i.to_string(),
            AttrValue::Bool(b) => b.to_string(),
            AttrValue::Ref(r) => format!("@0x{r:08x}"),
            AttrValue::Float(f) => f.to_string(),
            AttrValue::Raw(t, d) => format!("тип {t:#04x}, данные {d:#010x}"),
        }
    }

    /// Значение как строка, если это строка или ссылка на ресурс.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            AttrValue::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Значение как целое.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            AttrValue::Int(i) => Some(*i),
            AttrValue::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }

    /// Если значение — ссылка на ресурс, вернуть её идентификатор.
    pub fn as_ref(&self) -> Option<u32> {
        match self {
            AttrValue::Ref(id) => Some(*id),
            _ => None,
        }
    }
}

/// Атрибут XML-элемента.
#[derive(Debug, Clone, PartialEq)]
pub struct XmlAttr {
    /// Имя атрибута.
    pub name: String,
    /// Пространство имён (например, `http://schemas.android.com/apk/res/android`).
    pub namespace: String,
    /// Значение.
    pub value: AttrValue,
}

/// Элемент XML.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmlNode {
    /// Имя элемента (`manifest`, `application`, …).
    pub name: String,
    /// Атрибуты.
    pub attributes: Vec<XmlAttr>,
    /// Дочерние элементы.
    pub children: Vec<XmlNode>,
}

impl XmlNode {
    /// Атрибут по локальному имени.
    pub fn attr(&self, name: &str) -> Option<&XmlAttr> {
        self.attributes.iter().find(|a| a.name == name)
    }

    /// Строковое значение атрибута.
    pub fn attr_str(&self, name: &str) -> Option<&str> {
        self.attr(name).and_then(|a| a.value.as_str())
    }

    /// Целочисленное значение атрибута.
    pub fn attr_int(&self, name: &str) -> Option<i64> {
        self.attr(name).and_then(|a| a.value.as_int())
    }

    /// Все потомки с указанным именем (поиск в глубину).
    pub fn find_all(&self, name: &str) -> Vec<&XmlNode> {
        let mut out = Vec::new();
        fn walk<'a>(node: &'a XmlNode, name: &str, out: &mut Vec<&'a XmlNode>) {
            for child in &node.children {
                if child.name == name {
                    out.push(child);
                }
                walk(child, name, out);
            }
        }
        walk(self, name, &mut out);
        out
    }

    /// Первый потомок с указанным именем.
    pub fn find(&self, name: &str) -> Option<&XmlNode> {
        self.find_all(name).into_iter().next()
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    fn take(&mut self, n: usize) -> NovaResult<&'a [u8]> {
        if self.remaining() < n {
            return Err(NovaError::Package("AXML: неожиданный конец данных".into()));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }
    fn u16(&mut self) -> NovaResult<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> NovaResult<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn skip(&mut self, n: usize) -> NovaResult<()> {
        self.take(n).map(|_| ())
    }
    fn seek(&mut self, pos: usize) -> NovaResult<()> {
        if pos > self.data.len() {
            return Err(NovaError::Package("AXML: недопустимое смещение".into()));
        }
        self.pos = pos;
        Ok(())
    }
}

/// Строковый пул AXML (общий формат с ARSC, см. [`crate::arsc::read_string_pool`]).
struct StringPool {
    strings: Vec<String>,
}

impl StringPool {
    fn get(&self, index: u32) -> String {
        if index == u32::MAX {
            return String::new();
        }
        self.strings
            .get(index as usize)
            .cloned()
            .unwrap_or_default()
    }
}

/// Разбирает бинарный XML и возвращает корневой элемент.
pub fn parse(data: &[u8]) -> NovaResult<XmlNode> {
    let mut reader = Reader::new(data);
    let pool = {
        // Первый чанк — заголовок файла XML.
        let file_type = reader.u16()?;
        let header_size = reader.u16()? as usize;
        let _file_size = reader.u32()?;
        if file_type != CHUNK_XML {
            return Err(NovaError::Package(format!(
                "AXML: ожидался чанк 0x0003, получен {file_type:#06x}"
            )));
        }
        reader.skip(header_size.saturating_sub(8))?;

        // Ищем строковый пул среди чанков верхнего уровня.
        let mut pool = StringPool {
            strings: Vec::new(),
        };
        let mut cursor = reader.pos;
        while cursor + 8 <= data.len() {
            let mut chunk = Reader::new(&data[cursor..]);
            let chunk_type = chunk.u16()?;
            let _chunk_header = chunk.u16()? as usize;
            let chunk_size = chunk.u32()? as usize;
            if chunk_size < 8 || cursor + chunk_size > data.len() {
                break;
            }
            if chunk_type == CHUNK_STRING_POOL {
                pool = StringPool {
                    strings: crate::arsc::read_string_pool(data, cursor)?,
                };
                cursor += chunk_size;
                break;
            }
            cursor += chunk_size;
        }
        reader.seek(cursor)?;
        pool
    };

    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;

    while reader.remaining() >= 8 {
        let chunk_start = reader.pos;
        let chunk_type = reader.u16()?;
        let chunk_header = reader.u16()? as usize;
        let chunk_size = reader.u32()? as usize;
        if chunk_size < 8 || chunk_start + chunk_size > data.len() {
            break;
        }
        let next = chunk_start + chunk_size;
        match chunk_type {
            CHUNK_START_ELEMENT => {
                reader.skip(chunk_header.saturating_sub(8))?; // lineNumber + comment
                let _ns = reader.u32()?;
                let name_index = reader.u32()?;
                let attribute_start = reader.u16()? as usize;
                let attribute_size = reader.u16()? as usize;
                let attribute_count = reader.u16()? as usize;
                reader.skip(6)?; // idIndex, classIndex, styleIndex
                let mut node = XmlNode {
                    name: pool.get(name_index),
                    ..Default::default()
                };
                if attribute_count > 4096 {
                    return Err(NovaError::Package("AXML: слишком много атрибутов".into()));
                }
                let attr_size = if attribute_size == 0 {
                    20
                } else {
                    attribute_size
                };
                // Атрибуты начинаются в attrExt + attributeStart (обычно 20).
                let mut attr_pos = chunk_start + chunk_header + attribute_start.max(20);
                for _ in 0..attribute_count {
                    if attr_pos + attr_size > data.len() {
                        break;
                    }
                    let mut a = Reader::new(&data[attr_pos..]);
                    let ns_index = a.u32()?;
                    let name_idx = a.u32()?;
                    let raw_value = a.u32()?;
                    let _typed_size = a.u16()?;
                    let _res0 = a.take(1)?[0];
                    let data_type = a.take(1)?[0];
                    let value_data = a.u32()?;
                    let value = match data_type {
                        TYPE_NULL => AttrValue::Str(String::new()),
                        TYPE_STRING => {
                            let text = pool.get(value_data);
                            if text.is_empty() && raw_value != u32::MAX {
                                AttrValue::Str(pool.get(raw_value))
                            } else {
                                AttrValue::Str(text)
                            }
                        }
                        TYPE_INT_DEC => AttrValue::Int(value_data as i32 as i64),
                        TYPE_INT_HEX => AttrValue::Int(value_data as i64),
                        TYPE_INT_BOOLEAN => AttrValue::Bool(value_data != 0),
                        TYPE_REFERENCE | TYPE_ATTRIBUTE => AttrValue::Ref(value_data),
                        TYPE_FLOAT => AttrValue::Float(f32::from_bits(value_data)),
                        // Размеры, дроби, цвета и прочие типы сохраняем как есть.
                        other => AttrValue::Raw(other, value_data),
                    };
                    node.attributes.push(XmlAttr {
                        name: pool.get(name_idx),
                        namespace: pool.get(ns_index),
                        value,
                    });
                    attr_pos += attr_size;
                }
                stack.push(node);
            }
            CHUNK_END_ELEMENT => {
                if let Some(node) = stack.pop() {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else if root.is_none() {
                        root = Some(node);
                    }
                }
            }
            // Пространства имён (0x0100/0x0101), карта ресурсов (0x0180)
            // и текст (0x0104) для разбора манифеста не нужны.
            CHUNK_START_NAMESPACE | CHUNK_END_NAMESPACE | CHUNK_RESOURCE_MAP | CHUNK_CDATA => {}
            _ => {}
        }
        reader.seek(next)?;
    }

    // Если документ оборвался, сворачиваем незакрытые элементы в дерево.
    while stack.len() > 1 {
        let node = stack.pop().expect("стек не пуст");
        stack
            .last_mut()
            .expect("родитель существует")
            .children
            .push(node);
    }
    if root.is_none() {
        root = stack.pop();
    }
    root.ok_or_else(|| NovaError::Package("AXML: не найден корневой элемент".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Собирает минимальный валидный AXML: <manifest package="com.test" versionCode="7"><uses-sdk minSdkVersion="21"/></manifest>
    fn sample_axml() -> Vec<u8> {
        let strings = [
            "",                                           // 0: пустой namespace
            "manifest",                                   // 1
            "package",                                    // 2
            "com.test.app",                               // 3
            "versionCode",                                // 4
            "uses-sdk",                                   // 5
            "minSdkVersion",                              // 6
            "http://schemas.android.com/apk/res/android", // 7
        ];
        // UTF-16 пул
        let mut pool_data: Vec<u8> = Vec::new();
        let mut offsets: Vec<u32> = Vec::new();
        for s in strings {
            offsets.push(pool_data.len() as u32);
            let chars: Vec<u16> = s.encode_utf16().collect();
            pool_data.extend_from_slice(&(chars.len() as u16).to_le_bytes());
            for c in chars {
                pool_data.extend_from_slice(&c.to_le_bytes());
            }
            pool_data.extend_from_slice(&0u16.to_le_bytes());
        }
        let string_count = strings.len() as u32;
        let header_size = 28u32;
        let strings_start = header_size + string_count * 4;
        let mut pool = Vec::new();
        pool.extend_from_slice(&CHUNK_STRING_POOL.to_le_bytes());
        pool.extend_from_slice(&28u16.to_le_bytes());
        let pool_size_pos = pool.len();
        pool.extend_from_slice(&0u32.to_le_bytes());
        pool.extend_from_slice(&string_count.to_le_bytes());
        pool.extend_from_slice(&0u32.to_le_bytes()); // styleCount
        pool.extend_from_slice(&0u32.to_le_bytes()); // flags (UTF-16)
        pool.extend_from_slice(&strings_start.to_le_bytes());
        pool.extend_from_slice(&0u32.to_le_bytes()); // stylesStart
        for o in offsets {
            pool.extend_from_slice(&o.to_le_bytes());
        }
        pool.extend_from_slice(&pool_data);
        let pool_size = pool.len() as u32;
        pool[pool_size_pos..pool_size_pos + 4].copy_from_slice(&pool_size.to_le_bytes());

        let mut body = Vec::new();
        body.extend_from_slice(&pool);

        // START_ELEMENT manifest
        let start = |name: u32, attrs: Vec<(u32, u32, u8, u32)>| -> Vec<u8> {
            let mut chunk = Vec::new();
            chunk.extend_from_slice(&CHUNK_START_ELEMENT.to_le_bytes());
            chunk.extend_from_slice(&16u16.to_le_bytes()); // headerSize
            let size_pos = chunk.len();
            chunk.extend_from_slice(&0u32.to_le_bytes());
            chunk.extend_from_slice(&1u32.to_le_bytes()); // lineNumber
            chunk.extend_from_slice(&u32::MAX.to_le_bytes()); // comment
            chunk.extend_from_slice(&0u32.to_le_bytes()); // ns
            chunk.extend_from_slice(&name.to_le_bytes());
            chunk.extend_from_slice(&20u16.to_le_bytes()); // attributeStart
            chunk.extend_from_slice(&20u16.to_le_bytes()); // attributeSize
            chunk.extend_from_slice(&(attrs.len() as u16).to_le_bytes());
            chunk.extend_from_slice(&0u16.to_le_bytes());
            chunk.extend_from_slice(&0u16.to_le_bytes());
            chunk.extend_from_slice(&0u16.to_le_bytes());
            for (ns, nm, ty, data) in attrs {
                chunk.extend_from_slice(&ns.to_le_bytes());
                chunk.extend_from_slice(&nm.to_le_bytes());
                chunk.extend_from_slice(&u32::MAX.to_le_bytes()); // rawValue
                chunk.extend_from_slice(&8u16.to_le_bytes());
                chunk.push(0);
                chunk.push(ty);
                chunk.extend_from_slice(&data.to_le_bytes());
            }
            let size = chunk.len() as u32;
            chunk[size_pos..size_pos + 4].copy_from_slice(&size.to_le_bytes());
            chunk
        };
        let end = |name: u32| -> Vec<u8> {
            let mut chunk = Vec::new();
            chunk.extend_from_slice(&CHUNK_END_ELEMENT.to_le_bytes());
            chunk.extend_from_slice(&16u16.to_le_bytes());
            chunk.extend_from_slice(&24u32.to_le_bytes());
            chunk.extend_from_slice(&1u32.to_le_bytes());
            chunk.extend_from_slice(&u32::MAX.to_le_bytes());
            chunk.extend_from_slice(&0u32.to_le_bytes());
            chunk.extend_from_slice(&name.to_le_bytes());
            chunk
        };

        let android_ns = 7u32;
        body.extend_from_slice(&start(
            1,
            vec![(0, 2, TYPE_STRING, 3), (android_ns, 4, TYPE_INT_DEC, 7)],
        ));
        body.extend_from_slice(&start(5, vec![(android_ns, 6, TYPE_INT_DEC, 21)]));
        body.extend_from_slice(&end(5));
        body.extend_from_slice(&end(1));

        let mut file = Vec::new();
        file.extend_from_slice(&CHUNK_XML.to_le_bytes());
        file.extend_from_slice(&8u16.to_le_bytes());
        file.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    #[test]
    fn parses_manifest() {
        let data = sample_axml();
        let root = parse(&data).expect("разбор AXML");
        assert_eq!(root.name, "manifest");
        assert_eq!(root.attr_str("package"), Some("com.test.app"));
        assert_eq!(root.attr_int("versionCode"), Some(7));
        let sdk = root.find("uses-sdk").expect("uses-sdk");
        assert_eq!(sdk.attr_int("minSdkVersion"), Some(21));
        assert_eq!(
            sdk.attr("minSdkVersion").unwrap().namespace,
            "http://schemas.android.com/apk/res/android"
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"not xml at all").is_err());
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn attr_value_text() {
        assert_eq!(AttrValue::Ref(0x7f0a0001).as_text(), "@0x7f0a0001");
        assert_eq!(AttrValue::Bool(true).as_text(), "true");
        assert_eq!(AttrValue::Int(-5).as_int(), Some(-5));
    }
}

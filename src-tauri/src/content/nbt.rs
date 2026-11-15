//! Минимальный читатель NBT (формат `level.dat`): только чтение, без записи.

use std::io::Read;

#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    /// Массивы не нужны — храним только длину.
    Array(usize),
    String(String),
    List(Vec<Tag>),
    Compound(Vec<(String, Tag)>),
}

impl Tag {
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn path(&self, keys: &[&str]) -> Option<&Tag> {
        keys.iter().try_fold(self, |t, k| t.get(k))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Tag::Byte(v) => Some(v as i64),
            Tag::Short(v) => Some(v as i64),
            Tag::Int(v) => Some(v as i64),
            Tag::Long(v) => Some(v),
            _ => None,
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }
    fn string(&mut self) -> Option<String> {
        let len = u16::from_be_bytes(self.take(2)?.try_into().ok()?) as usize;
        // Modified UTF-8 в именах миров почти всегда совпадает с обычным.
        Some(String::from_utf8_lossy(self.take(len)?).to_string())
    }
    fn len(&mut self) -> Option<usize> {
        usize::try_from(self.i32()?).ok()
    }

    fn payload(&mut self, kind: u8) -> Option<Tag> {
        if self.depth > 512 {
            return None;
        }
        Some(match kind {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
            6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                self.take(n)?;
                Tag::Array(n)
            }
            8 => Tag::String(self.string()?),
            9 => {
                let item = self.u8()?;
                let n = self.len()?;
                self.depth += 1;
                let mut v = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    v.push(self.payload(item)?);
                }
                self.depth -= 1;
                Tag::List(v)
            }
            10 => {
                self.depth += 1;
                let mut items = Vec::new();
                loop {
                    let k = self.u8()?;
                    if k == 0 {
                        break;
                    }
                    let name = self.string()?;
                    items.push((name, self.payload(k)?));
                }
                self.depth -= 1;
                Tag::Compound(items)
            }
            11 => {
                let n = self.len()?;
                self.take(n.checked_mul(4)?)?;
                Tag::Array(n)
            }
            12 => {
                let n = self.len()?;
                self.take(n.checked_mul(8)?)?;
                Tag::Array(n)
            }
            _ => return None,
        })
    }
}

/// Разбирает несжатый NBT (корневой именованный compound).
pub fn parse(data: &[u8]) -> Option<Tag> {
    let mut r = Reader { data, pos: 0, depth: 0 };
    let kind = r.u8()?;
    if kind != 10 {
        return None;
    }
    r.string()?;
    r.payload(10)
}

/// Читает NBT, сжатый gzip (как `level.dat`) или несжатый.
pub fn read_file(path: &std::path::Path) -> Option<Tag> {
    let raw = std::fs::read(path).ok()?;
    if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&raw[..]).take(64 * 1024 * 1024).read_to_end(&mut out).ok()?;
        parse(&out)
    } else {
        parse(&raw)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Строит NBT вручную: `Data { LevelName, LastPlayed, GameType, hardcore, Version { Name } }`.
    pub fn level_dat(name: &str, last_played: i64, game_type: i32, hardcore: bool) -> Vec<u8> {
        fn s(out: &mut Vec<u8>, v: &str) {
            out.extend((v.len() as u16).to_be_bytes());
            out.extend(v.as_bytes());
        }
        let mut b = vec![10];
        s(&mut b, "");
        b.push(10);
        s(&mut b, "Data");
        b.push(8);
        s(&mut b, "LevelName");
        s(&mut b, name);
        b.push(4);
        s(&mut b, "LastPlayed");
        b.extend(last_played.to_be_bytes());
        b.push(3);
        s(&mut b, "GameType");
        b.extend(game_type.to_be_bytes());
        b.push(1);
        s(&mut b, "hardcore");
        b.push(hardcore as u8);
        b.push(11);
        s(&mut b, "DataPacks");
        b.extend(2i32.to_be_bytes());
        b.extend([0u8; 8]);
        b.push(9);
        s(&mut b, "ServerBrands");
        b.push(8);
        b.extend(1i32.to_be_bytes());
        s(&mut b, "fabric");
        b.push(10);
        s(&mut b, "Version");
        b.push(8);
        s(&mut b, "Name");
        s(&mut b, "1.21.1");
        b.push(0); // конец Version
        b.push(0); // конец Data
        b.push(0); // конец корня
        b
    }

    #[test]
    fn parses_level_dat() {
        let t = parse(&level_dat("Мой мир", 1_700_000_000_000, 1, false)).unwrap();
        assert_eq!(t.path(&["Data", "LevelName"]).and_then(Tag::as_str), Some("Мой мир"));
        assert_eq!(t.path(&["Data", "LastPlayed"]).and_then(Tag::as_i64), Some(1_700_000_000_000));
        assert_eq!(t.path(&["Data", "GameType"]).and_then(Tag::as_i64), Some(1));
        assert_eq!(t.path(&["Data", "Version", "Name"]).and_then(Tag::as_str), Some("1.21.1"));
        assert!(matches!(t.path(&["Data", "ServerBrands"]), Some(Tag::List(v)) if v.len() == 1));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(&[]).is_none());
        assert!(parse(&[10, 0, 0, 3]).is_none());
        assert!(parse(&[10, 0, 0, 9, 0, 1, b'x', 3, 0x7f, 0xff, 0xff, 0xff]).is_none());
    }
}

//! Метаданные модов и паков из самих файлов.
//!
//! Моды: `fabric.mod.json`, `quilt.mod.json`, `META-INF/neoforge.mods.toml`,
//! `META-INF/mods.toml` (Forge 1.13+), `mcmod.info` (Forge до 1.13).
//! Ресурспаки и шейдеры: `pack.mcmeta` + `pack.png`.
//! Поддерживаются и архивы, и распакованные папки.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use zip::ZipArchive;

/// Иконки больше этого не передаём в UI (их сотни в списке).
const MAX_ICON: u64 = 192 * 1024;
/// Файлы метаданных больше этого не читаем (защита от мусора).
const MAX_META: u64 = 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// data:-URL иконки.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Для каких загрузчиков мод: `fabric`, `quilt`, `forge`, `neoforge`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub loaders: Vec<String>,
}

/// Источник файлов: zip-архив или папка.
enum Source {
    Zip(ZipArchive<File>),
    Dir(std::path::PathBuf),
}

impl Source {
    fn open(path: &Path) -> Option<Self> {
        if path.is_dir() {
            return Some(Source::Dir(path.to_path_buf()));
        }
        ZipArchive::new(File::open(path).ok()?).ok().map(Source::Zip)
    }

    fn read(&mut self, name: &str, limit: u64) -> Option<Vec<u8>> {
        let name = name.trim_start_matches('/');
        match self {
            Source::Zip(z) => {
                let e = z.by_name(name).ok()?;
                if e.size() > limit {
                    return None;
                }
                let mut buf = Vec::with_capacity(e.size() as usize);
                e.take(limit).read_to_end(&mut buf).ok()?;
                Some(buf)
            }
            Source::Dir(d) => {
                // Путь из метаданных не должен выходить за пределы папки пака.
                if name.split(['/', '\\']).any(|p| p == "..") {
                    return None;
                }
                let p = d.join(name);
                if p.metadata().ok()?.len() > limit {
                    return None;
                }
                std::fs::read(p).ok()
            }
        }
    }

    fn text(&mut self, name: &str) -> Option<String> {
        self.read(name, MAX_META).map(|b| String::from_utf8_lossy(&b).trim_start_matches('\u{feff}').to_string())
    }

    fn json(&mut self, name: &str) -> Option<Value> {
        let text = self.text(name)?;
        serde_json::from_str(&text).ok().or_else(|| {
            // mcmod.info часто содержит переводы строк внутри строк — убираем их.
            serde_json::from_str(&text.replace(['\n', '\r'], " ")).ok()
        })
    }

    fn icon(&mut self, name: &str) -> Option<String> {
        let bytes = self.read(name, MAX_ICON)?;
        let mime = if bytes.starts_with(b"\x89PNG") {
            "image/png"
        } else if bytes.starts_with(b"\xff\xd8") {
            "image/jpeg"
        } else {
            return None;
        };
        Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    }
}

fn s(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}

fn people(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|p| match p {
                Value::String(s) => Some(s.clone()),
                Value::Object(o) => s(o.get("name")),
                _ => None,
            })
            .collect(),
        // quilt: { "Имя": "роль" }
        Some(Value::Object(o)) => o.keys().cloned().collect(),
        Some(Value::String(x)) => x.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
        _ => vec![],
    }
}

/// Путь к иконке: строка или `{ "размер": "путь" }` (берём наибольший).
fn icon_path(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(p) => Some(p.clone()),
        Value::Object(o) => o
            .iter()
            .max_by_key(|(k, _)| k.parse::<u32>().unwrap_or(0))
            .and_then(|(_, p)| p.as_str().map(str::to_string)),
        _ => None,
    }
}

/// Текст из JSON-компонента Minecraft (строка, массив или `{text, extra}`).
fn component_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(component_text).collect(),
        Value::Object(o) => {
            let mut out = o.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
            if let Some(extra) = o.get("extra") {
                out.push_str(&component_text(extra));
            }
            out
        }
        _ => String::new(),
    }
}

/// Убирает коды форматирования `§x`.
fn strip_formatting(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

fn manifest_version(src: &mut Source) -> Option<String> {
    let text = src.text("META-INF/MANIFEST.MF")?;
    text.lines().find_map(|l| l.strip_prefix("Implementation-Version:").map(|v| v.trim().to_string()))
}

fn fabric(src: &mut Source, file: &str, loader: &str) -> Option<ContentMeta> {
    let j = src.json(file)?;
    Some(ContentMeta {
        id: s(j.get("id")),
        name: s(j.get("name")),
        version: s(j.get("version")),
        description: s(j.get("description")),
        authors: people(j.get("authors")),
        icon: icon_path(j.get("icon")).and_then(|p| src.icon(&p)),
        loaders: vec![loader.into()],
    })
}

fn quilt(src: &mut Source) -> Option<ContentMeta> {
    let j = src.json("quilt.mod.json")?;
    let l = j.get("quilt_loader")?;
    let m = l.get("metadata");
    Some(ContentMeta {
        id: s(l.get("id")),
        name: s(m.and_then(|m| m.get("name"))),
        version: s(l.get("version")),
        description: s(m.and_then(|m| m.get("description"))),
        authors: people(m.and_then(|m| m.get("contributors"))),
        icon: icon_path(m.and_then(|m| m.get("icon"))).and_then(|p| src.icon(&p)),
        loaders: vec!["quilt".into()],
    })
}

fn forge_toml(src: &mut Source, file: &str, loader: &str) -> Option<ContentMeta> {
    let t: toml::Table = src.text(file)?.parse().ok()?;
    let m = t.get("mods")?.as_array()?.first()?.as_table()?;
    let ts = |k: &str| m.get(k).and_then(|v| v.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    let mut version = ts("version");
    if version.as_deref().is_some_and(|v| v.contains("${")) {
        version = manifest_version(src).or(version);
    }
    let authors = m
        .get("authors")
        .or_else(|| t.get("authors"))
        .and_then(|v| v.as_str())
        .map(|a| a.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let logo = m.get("logoFile").or_else(|| t.get("logoFile")).and_then(|v| v.as_str()).map(str::to_string);
    Some(ContentMeta {
        id: ts("modId"),
        name: ts("displayName"),
        version,
        description: ts("description"),
        authors,
        icon: logo.and_then(|p| src.icon(&p)),
        loaders: vec![loader.into()],
    })
}

fn mcmod_info(src: &mut Source) -> Option<ContentMeta> {
    let j = src.json("mcmod.info")?;
    let m = match &j {
        Value::Array(a) => a.first()?,
        Value::Object(o) => o.get("modList")?.as_array()?.first()?,
        _ => return None,
    };
    Some(ContentMeta {
        id: s(m.get("modid")),
        name: s(m.get("name")),
        version: s(m.get("version")),
        description: s(m.get("description")),
        authors: people(m.get("authorList").or(m.get("authors"))),
        icon: s(m.get("logoFile")).and_then(|p| src.icon(&p)),
        loaders: vec!["forge".into()],
    })
}

/// Метаданные мода. Если мод собран под несколько загрузчиков — перечисляем все.
pub fn read_mod(path: &Path) -> Option<ContentMeta> {
    let mut src = Source::open(path)?;
    let candidates = [
        fabric(&mut src, "fabric.mod.json", "fabric"),
        quilt(&mut src),
        forge_toml(&mut src, "META-INF/neoforge.mods.toml", "neoforge"),
        forge_toml(&mut src, "META-INF/mods.toml", "forge"),
        mcmod_info(&mut src),
    ];
    let mut found = candidates.into_iter().flatten();
    let mut meta = found.next()?;
    for other in found {
        for l in other.loaders {
            if !meta.loaders.contains(&l) {
                meta.loaders.push(l);
            }
        }
    }
    Some(meta)
}

/// Метаданные ресурспака / шейдерпака / датапака.
pub fn read_pack(path: &Path) -> Option<ContentMeta> {
    let mut src = Source::open(path)?;
    let j = src.json("pack.mcmeta");
    let description = j
        .as_ref()
        .and_then(|j| j.get("pack"))
        .and_then(|p| p.get("description"))
        .map(|d| strip_formatting(&component_text(d)))
        .filter(|d| !d.is_empty());
    let icon = src.icon("pack.png");
    if j.is_none() && icon.is_none() {
        return None;
    }
    Some(ContentMeta { description, icon, ..Default::default() })
}

/// Подходит ли мод (по его метаданным) к загрузчику инстанса.
/// Quilt грузит моды Fabric; NeoForge для 1.20.1 — моды Forge.
pub fn mod_compatible(mod_loaders: &[String], instance_loader: &str, minecraft: &str) -> bool {
    if mod_loaders.is_empty() {
        return true;
    }
    let ok = |l: &str| mod_loaders.iter().any(|m| m == l);
    match instance_loader {
        "quilt" => ok("quilt") || ok("fabric"),
        "neoforge" => ok("neoforge") || (minecraft == "1.20.1" && ok("forge")),
        other => ok(other),
    }
}

#[cfg(test)]
pub mod tests_support {
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    pub fn zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::zip as make_zip;
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

    #[test]
    fn fabric_mod() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sodium.jar");
        make_zip(
            &p,
            &[
                ("fabric.mod.json", br#"{ "id": "sodium", "name": "Sodium", "version": "0.6.0",
                    "description": "Fast", "authors": ["JellySquid", { "name": "IMS" }],
                    "icon": { "32": "small.png", "128": "assets/sodium/icon.png" } }"#),
                ("assets/sodium/icon.png", PNG),
            ],
        );
        let m = read_mod(&p).unwrap();
        assert_eq!(m.id.as_deref(), Some("sodium"));
        assert_eq!(m.name.as_deref(), Some("Sodium"));
        assert_eq!(m.authors, vec!["JellySquid", "IMS"]);
        assert!(m.icon.unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(m.loaders, vec!["fabric"]);
    }

    #[test]
    fn forge_toml_with_jar_version() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("jei.jar");
        make_zip(
            &p,
            &[
                ("META-INF/mods.toml", br#"
modLoader="javafml"
loaderVersion="[47,)"
authors="mezz"
[[mods]]
modId="jei"
version="${file.jarVersion}"
displayName="Just Enough Items"
description='''
Item list
'''
"#),
                ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\r\nImplementation-Version: 15.2.0.27\r\n"),
            ],
        );
        let m = read_mod(&p).unwrap();
        assert_eq!(m.id.as_deref(), Some("jei"));
        assert_eq!(m.version.as_deref(), Some("15.2.0.27"));
        assert_eq!(m.description.as_deref(), Some("Item list"));
        assert_eq!(m.authors, vec!["mezz"]);
        assert_eq!(m.loaders, vec!["forge"]);
    }

    #[test]
    fn multiloader_and_mcmod_info() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("multi.jar");
        make_zip(
            &p,
            &[
                ("fabric.mod.json", br#"{ "id": "multi", "version": "1" }"#),
                ("META-INF/neoforge.mods.toml", b"[[mods]]\nmodId=\"multi\"\nversion=\"1\"\n"),
            ],
        );
        assert_eq!(read_mod(&p).unwrap().loaders, vec!["fabric", "neoforge"]);

        let old = d.path().join("old.jar");
        make_zip(&old, &[("mcmod.info", b"[{\"modid\": \"jei\", \"name\": \"JEI\", \"version\": \"4.16\",\n\"description\": \"line1\nline2\"}]")]);
        let m = read_mod(&old).unwrap();
        assert_eq!(m.name.as_deref(), Some("JEI"));
        assert_eq!(m.loaders, vec!["forge"]);

        let none = d.path().join("none.jar");
        make_zip(&none, &[("a.class", b"x")]);
        assert!(read_mod(&none).is_none());
    }

    #[test]
    fn resource_pack_zip_and_dir() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("faithful.zip");
        make_zip(
            &p,
            &[
                ("pack.mcmeta", r#"{ "pack": { "pack_format": 15, "description": [{"text":"§aFaithful "}, {"text":"32x"}] } }"#.as_bytes()),
                ("pack.png", PNG),
            ],
        );
        let m = read_pack(&p).unwrap();
        assert_eq!(m.description.as_deref(), Some("Faithful 32x"));
        assert!(m.icon.is_some());

        let dir = d.path().join("MyPack");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("pack.mcmeta"), r#"{"pack":{"description":"Папка"}}"#).unwrap();
        assert_eq!(read_pack(&dir).unwrap().description.as_deref(), Some("Папка"));
    }

    #[test]
    fn compatibility() {
        let f = vec!["fabric".to_string()];
        assert!(mod_compatible(&f, "fabric", "1.21.1"));
        assert!(mod_compatible(&f, "quilt", "1.21.1"));
        assert!(!mod_compatible(&f, "forge", "1.21.1"));
        let forge = vec!["forge".to_string()];
        assert!(mod_compatible(&forge, "neoforge", "1.20.1"));
        assert!(!mod_compatible(&forge, "neoforge", "1.21.1"));
        assert!(mod_compatible(&[], "forge", "1.21.1"));
    }
}

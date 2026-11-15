//! Инстансы — изолированные установки Minecraft.
//!
//! Раскладка: `instances/<id>/instance.json` + `instances/<id>/.minecraft/` (игровая папка:
//! моды, миры, ресурспаки, конфиги). `id` — имя папки, не меняется при переименовании.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::util::{read_json_opt, write_json_atomic};

pub mod archive;
pub mod groups;

pub const INSTANCE_FILE: &str = "instance.json";
pub const GAME_DIR: &str = ".minecraft";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl LoaderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LoaderKind::Fabric => "fabric",
            LoaderKind::Quilt => "quilt",
            LoaderKind::Forge => "forge",
            LoaderKind::NeoForge => "neoforge",
        }
    }

    pub fn display(&self) -> &'static str {
        match self {
            LoaderKind::Fabric => "Fabric",
            LoaderKind::Quilt => "Quilt",
            LoaderKind::Forge => "Forge",
            LoaderKind::NeoForge => "NeoForge",
        }
    }

    /// Загрузчик ставится установщиком с процессорами (Forge, NeoForge).
    pub fn uses_installer(&self) -> bool {
        matches!(self, LoaderKind::Forge | LoaderKind::NeoForge)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderSpec {
    pub kind: LoaderKind,
    pub version: String,
}

/// Настройки инстанса, переопределяющие глобальные. `None` — брать глобальное значение.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InstanceOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_memory_mb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jvm_args: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fullscreen: Option<bool>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_launch_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_exit_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    /// Имя папки (заполняется при чтении, в файл не пишется).
    #[serde(skip_deserializing, default)]
    pub id: String,
    #[serde(default = "format_version")]
    pub format: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Версия Minecraft (id из манифеста).
    pub minecraft: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<LoaderSpec>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played: Option<DateTime<Utc>>,
    #[serde(default)]
    pub playtime_secs: u64,
    /// Файлы версии успешно скачаны и проверены.
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub overrides: InstanceOverrides,
    /// Модпак, из которого создан инстанс.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modpack: Option<crate::modpacks::ModpackLink>,
}

fn format_version() -> u32 {
    FORMAT_VERSION
}

/// Транслитерация кириллицы для имён папок.
fn translit(c: char) -> Option<&'static str> {
    Some(match c {
        'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' => "e", 'ё' => "e",
        'ж' => "zh", 'з' => "z", 'и' => "i", 'й' => "y", 'к' => "k", 'л' => "l", 'м' => "m",
        'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r", 'с' => "s", 'т' => "t", 'у' => "u",
        'ф' => "f", 'х' => "kh", 'ц' => "ts", 'ч' => "ch", 'ш' => "sh", 'щ' => "shch", 'ъ' => "",
        'ы' => "y", 'ь' => "", 'э' => "e", 'ю' => "yu", 'я' => "ya", 'є' => "ye", 'і' => "i",
        'ї' => "yi", 'ґ' => "g", 'ў' => "u",
        _ => return None,
    })
}

/// Делает из имени безопасное ASCII-имя папки: латиница/цифры, кириллица
/// транслитерируется, остальное → `-`. ASCII важен: часть нативных библиотек
/// старых версий не умеет работать с не-ASCII путями.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
        } else if let Some(t) = translit(c) {
            out.push_str(t);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut s: String = out.trim_matches(|c| c == '-' || c == '.').chars().take(48).collect();
    s = s.trim_end_matches(['-', '.']).to_string();
    const RESERVED: [&str; 22] = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if s.is_empty() {
        s = "instance".into();
    } else if RESERVED.contains(&s.as_str()) {
        s = format!("{s}-instance");
    }
    s
}

pub fn validate_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::InvalidInput("имя инстанса не может быть пустым".into()));
    }
    if n.chars().count() > 64 {
        return Err(AppError::InvalidInput("имя инстанса длиннее 64 символов".into()));
    }
    Ok(n.to_string())
}

pub(crate) fn normalize_group(group: Option<String>) -> Option<String> {
    group.map(|g| g.trim().to_string()).filter(|g| !g.is_empty())
}

pub struct InstanceStore {
    root: PathBuf,
    /// Сериализует операции «прочитал–изменил–записал» и создание.
    lock: Mutex<()>,
}

impl InstanceStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root, lock: Mutex::new(()) }
    }

    pub fn dir(&self, id: &str) -> AppResult<PathBuf> {
        // id приходит из UI — не даём выйти за пределы папки инстансов.
        if id.is_empty() || id.contains(['/', '\\']) || id == "." || id == ".." {
            return Err(AppError::InvalidInput(format!("некорректный id инстанса: {id}")));
        }
        Ok(self.root.join(id))
    }

    pub fn game_dir(&self, id: &str) -> AppResult<PathBuf> {
        Ok(self.dir(id)?.join(GAME_DIR))
    }

    pub fn list(&self) -> AppResult<Vec<Instance>> {
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            match read_json_opt::<Instance>(&path.join(INSTANCE_FILE)) {
                Ok(Some(mut inst)) => {
                    inst.id = id;
                    out.push(inst);
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("пропущен повреждённый инстанс {id}: {e}"),
            }
        }
        out.sort_by_cached_key(|i| i.name.to_lowercase());
        Ok(out)
    }

    pub fn get(&self, id: &str) -> AppResult<Instance> {
        let mut inst: Instance = read_json_opt(&self.dir(id)?.join(INSTANCE_FILE))?
            .ok_or_else(|| AppError::NotFound(format!("инстанс {id}")))?;
        inst.id = id.to_string();
        Ok(inst)
    }

    pub fn save(&self, inst: &Instance) -> AppResult<()> {
        write_json_atomic(&self.dir(&inst.id)?.join(INSTANCE_FILE), inst)
    }

    /// Изменяет инстанс под блокировкой «прочитал–изменил–записал».
    pub fn update(&self, id: &str, f: impl FnOnce(&mut Instance)) -> AppResult<Instance> {
        let _guard = self.lock.lock().unwrap();
        let mut inst = self.get(id)?;
        f(&mut inst);
        self.save(&inst)?;
        Ok(inst)
    }

    fn unique_id(&self, name: &str) -> String {
        let base = slugify(name);
        let mut id = base.clone();
        let mut n = 2;
        while self.root.join(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    pub fn create(&self, name: &str, group: Option<String>, minecraft: &str, loader: Option<LoaderSpec>) -> AppResult<Instance> {
        let name = validate_name(name)?;
        if minecraft.trim().is_empty() {
            return Err(AppError::InvalidInput("не выбрана версия Minecraft".into()));
        }
        let _guard = self.lock.lock().unwrap();
        let id = self.unique_id(&name);
        let inst = Instance {
            id: id.clone(),
            format: FORMAT_VERSION,
            name,
            group: normalize_group(group),
            icon: None,
            minecraft: minecraft.to_string(),
            loader,
            created_at: Utc::now(),
            last_played: None,
            playtime_secs: 0,
            installed: false,
            overrides: InstanceOverrides::default(),
            modpack: None,
        };
        std::fs::create_dir_all(self.game_dir(&id)?)?;
        self.save(&inst)?;
        Ok(inst)
    }

    pub fn rename(&self, id: &str, name: &str) -> AppResult<Instance> {
        let name = validate_name(name)?;
        self.update(id, |i| i.name = name)
    }

    pub fn set_group(&self, id: &str, group: Option<String>) -> AppResult<Instance> {
        let group = normalize_group(group);
        self.update(id, |i| i.group = group)
    }

    /// Удаление в корзину ОС (миры игрока можно восстановить).
    pub fn delete(&self, id: &str) -> AppResult<()> {
        let dir = self.dir(id)?;
        if !dir.join(INSTANCE_FILE).exists() {
            return Err(AppError::NotFound(format!("инстанс {id}")));
        }
        trash::delete(&dir).map_err(|e| AppError::Other(format!("не удалось переместить в корзину: {e}")))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_names() {
        assert_eq!(slugify("My Pack 1.20!"), "my-pack-1.20");
        assert_eq!(slugify("  Выживание  "), "vyzhivanie");
        assert_eq!(slugify("Щит и Ёлка"), "shchit-i-elka");
        assert_eq!(slugify("日本"), "instance");
        assert_eq!(slugify("///"), "instance");
        assert_eq!(slugify("CON"), "con-instance");
        assert_eq!(slugify("..hidden.."), "hidden");
    }

    #[test]
    fn create_list_rename_group() {
        let dir = tempfile::tempdir().unwrap();
        let store = InstanceStore::new(dir.path().to_path_buf());
        let a = store.create("Ванилла", None, "1.20.1", None).unwrap();
        let b = store.create("Ванилла", Some("  Тесты ".into()), "1.8.9", None).unwrap();
        assert_eq!(a.id, "vanilla");
        assert_eq!(b.id, "vanilla-2");
        assert_eq!(b.group.as_deref(), Some("Тесты"));
        assert!(dir.path().join("vanilla").join(GAME_DIR).is_dir());

        store.rename(&a.id, "Alpha").unwrap();
        store.set_group(&b.id, Some("".into())).unwrap();
        let list = store.list().unwrap();
        assert_eq!(list.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), vec!["Alpha", "Ванилла"]);
        assert_eq!(list[0].id, "vanilla", "id не меняется при переименовании");
        assert!(list[1].group.is_none());
    }

    #[test]
    fn rejects_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let store = InstanceStore::new(dir.path().to_path_buf());
        assert!(store.create("   ", None, "1.20.1", None).is_err());
        assert!(store.create("ok", None, "", None).is_err());
        assert!(store.get("../etc").is_err());
        assert!(matches!(store.get("missing"), Err(AppError::NotFound(_))));
    }

    #[test]
    fn instance_json_is_forward_compatible() {
        let inst: Instance = serde_json::from_str(
            r#"{ "name": "x", "minecraft": "1.20.1", "createdAt": "2026-01-01T00:00:00Z", "futureField": 1 }"#,
        )
        .unwrap();
        assert_eq!(inst.format, FORMAT_VERSION);
        assert!(!inst.installed);
    }
}

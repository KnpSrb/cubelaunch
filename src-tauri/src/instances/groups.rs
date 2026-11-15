//! Порядок групп инстансов — `groups.json` в папке данных.
//!
//! Принадлежность инстанса группе хранится в его `instance.json` (поле `group`),
//! а здесь — только порядок и пустые группы (созданные заранее). Группы, которые
//! есть у инстансов, но отсутствуют в файле, идут в конце по алфавиту.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::{normalize_group, InstanceStore};
use crate::error::{AppError, AppResult};
use crate::util::{read_json_opt, write_json_atomic};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct GroupsFile {
    groups: Vec<String>,
}

pub struct GroupStore {
    file: PathBuf,
    lock: Mutex<()>,
}

fn validate(name: &str) -> AppResult<String> {
    let n = normalize_group(Some(name.to_string())).ok_or_else(|| AppError::InvalidInput("имя группы не может быть пустым".into()))?;
    if n.chars().count() > 64 {
        return Err(AppError::InvalidInput("имя группы длиннее 64 символов".into()));
    }
    Ok(n)
}

impl GroupStore {
    pub fn new(file: PathBuf) -> Self {
        Self { file, lock: Mutex::new(()) }
    }

    fn read(&self) -> Vec<String> {
        match read_json_opt::<GroupsFile>(&self.file) {
            Ok(f) => f.unwrap_or_default().groups,
            Err(e) => {
                tracing::warn!("groups.json повреждён, порядок групп сброшен: {e}");
                vec![]
            }
        }
    }

    fn write(&self, groups: &[String]) -> AppResult<()> {
        let mut seen = BTreeSet::new();
        let groups: Vec<String> = groups.iter().filter(|g| seen.insert(g.to_string())).cloned().collect();
        write_json_atomic(&self.file, &GroupsFile { groups })
    }

    /// Все группы в пользовательском порядке (включая пустые).
    pub fn list(&self, instances: &InstanceStore) -> AppResult<Vec<String>> {
        let _g = self.lock.lock().unwrap();
        let mut out = self.read();
        let mut extra: Vec<String> = instances.list()?.into_iter().filter_map(|i| i.group).filter(|g| !out.contains(g)).collect();
        extra.sort_by_key(|g| g.to_lowercase());
        extra.dedup();
        out.extend(extra);
        Ok(out)
    }

    /// Сохраняет новый порядок. Группы, которых нет в `order`, сохраняют
    /// относительный порядок и идут после.
    pub fn set_order(&self, instances: &InstanceStore, order: Vec<String>) -> AppResult<Vec<String>> {
        let current = self.list(instances)?;
        let _g = self.lock.lock().unwrap();
        let mut out: Vec<String> = order.into_iter().filter_map(|g| normalize_group(Some(g))).filter(|g| current.contains(g)).collect();
        out.extend(current.into_iter().filter(|g| !out.contains(g)).collect::<Vec<_>>());
        self.write(&out)?;
        Ok(out)
    }

    /// Создаёт пустую группу (в конце списка).
    pub fn create(&self, instances: &InstanceStore, name: &str) -> AppResult<Vec<String>> {
        let name = validate(name)?;
        let mut all = self.list(instances)?;
        if all.contains(&name) {
            return Err(AppError::InvalidInput(format!("группа «{name}» уже есть")));
        }
        all.push(name);
        let _g = self.lock.lock().unwrap();
        self.write(&all)?;
        Ok(all)
    }

    /// Переименовывает группу у всех её инстансов. Если группа с новым именем
    /// уже есть — группы сливаются.
    pub fn rename(&self, instances: &InstanceStore, old: &str, new: &str) -> AppResult<Vec<String>> {
        let new = validate(new)?;
        let all = self.list(instances)?;
        for inst in instances.list()? {
            if inst.group.as_deref() == Some(old) {
                instances.set_group(&inst.id, Some(new.clone()))?;
            }
        }
        let merged = all.contains(&new);
        let out: Vec<String> = all.into_iter().filter(|g| !(merged && g == old)).map(|g| if g == old { new.clone() } else { g }).collect();
        let _g = self.lock.lock().unwrap();
        self.write(&out)?;
        Ok(out)
    }

    /// Удаляет группу; её инстансы остаются без группы.
    pub fn delete(&self, instances: &InstanceStore, name: &str) -> AppResult<Vec<String>> {
        let all = self.list(instances)?;
        for inst in instances.list()? {
            if inst.group.as_deref() == Some(name) {
                instances.set_group(&inst.id, None)?;
            }
        }
        let out: Vec<String> = all.into_iter().filter(|g| g != name).collect();
        let _g = self.lock.lock().unwrap();
        self.write(&out)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_create_rename_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = InstanceStore::new(dir.path().join("instances"));
        let groups = GroupStore::new(dir.path().join("groups.json"));
        let a = store.create("a", Some("Бета".into()), "1.20.1", None).unwrap();
        store.create("b", Some("Альфа".into()), "1.20.1", None).unwrap();
        assert_eq!(groups.list(&store).unwrap(), vec!["Альфа", "Бета"], "без файла — по алфавиту");

        groups.set_order(&store, vec!["Бета".into(), "Нет такой".into()]).unwrap();
        assert_eq!(groups.list(&store).unwrap(), vec!["Бета", "Альфа"]);

        groups.create(&store, "Пустая").unwrap();
        assert!(groups.create(&store, "Пустая").is_err());
        assert!(groups.create(&store, "  ").is_err());
        assert_eq!(groups.list(&store).unwrap(), vec!["Бета", "Альфа", "Пустая"]);

        groups.rename(&store, "Бета", "Гамма").unwrap();
        assert_eq!(store.get(&a.id).unwrap().group.as_deref(), Some("Гамма"));
        assert_eq!(groups.list(&store).unwrap(), vec!["Гамма", "Альфа", "Пустая"]);

        // Слияние: переименование в существующую группу.
        groups.rename(&store, "Гамма", "Альфа").unwrap();
        assert_eq!(groups.list(&store).unwrap(), vec!["Альфа", "Пустая"]);
        assert_eq!(store.get(&a.id).unwrap().group.as_deref(), Some("Альфа"));

        groups.delete(&store, "Альфа").unwrap();
        assert!(store.get(&a.id).unwrap().group.is_none());
        assert_eq!(groups.list(&store).unwrap(), vec!["Пустая"]);
    }
}

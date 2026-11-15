//! Расположение папок лаунчера.
//!
//! Порядок выбора папки данных:
//! 1. Портативный режим — рядом с исполняемым файлом лежит `portable.txt`:
//!    данные хранятся в `<папка exe>/data`.
//! 2. Переопределение из `bootstrap.json` в системной папке конфигурации
//!    (пишется, когда пользователь меняет папку данных в настройках).
//! 3. По умолчанию — системная папка данных (`%APPDATA%\Cubelaunch`,
//!    `~/.local/share/Cubelaunch`, `~/Library/Application Support/Cubelaunch`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::util::{read_json_opt, write_json_atomic};

pub const APP_DIR_NAME: &str = "Cubelaunch";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub portable: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Bootstrap {
    data_dir: Option<PathBuf>,
}

impl AppPaths {
    pub fn resolve() -> AppResult<Self> {
        if let Some(dir) = portable_dir() {
            return Ok(Self { data_dir: dir, portable: true });
        }
        let data_dir = read_bootstrap()
            .and_then(|b| b.data_dir)
            .or_else(default_data_dir)
            .ok_or_else(|| AppError::Other("Не удалось определить папку данных".into()))?;
        Ok(Self { data_dir, portable: false })
    }

    /// Создаёт все нужные подпапки.
    pub fn ensure(&self) -> AppResult<()> {
        for dir in [
            self.data_dir.clone(),
            self.instances_dir(),
            self.versions_dir(),
            self.meta_dir(),
            self.libraries_dir(),
            self.assets_dir(),
            self.java_dir(),
            self.logs_dir(),
            self.cache_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }
    pub fn accounts_file(&self) -> PathBuf {
        self.data_dir.join("accounts.json")
    }
    pub fn instances_dir(&self) -> PathBuf {
        self.data_dir.join("instances")
    }
    /// Ванильные версии: `versions/<id>/<id>.json` и `<id>.jar` (общие для всех инстансов).
    pub fn versions_dir(&self) -> PathBuf {
        self.data_dir.join("versions")
    }
    /// Кэш манифестов.
    pub fn meta_dir(&self) -> PathBuf {
        self.data_dir.join("meta")
    }
    /// Общие библиотеки для всех инстансов.
    pub fn libraries_dir(&self) -> PathBuf {
        self.data_dir.join("libraries")
    }
    /// Общие assets (objects/indexes) для всех инстансов.
    pub fn assets_dir(&self) -> PathBuf {
        self.data_dir.join("assets")
    }
    /// JVM, скачанные лаунчером.
    pub fn java_dir(&self) -> PathBuf {
        self.data_dir.join("java")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache")
    }
}

pub fn default_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_DIR_NAME))
}

fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join("portable.txt").exists().then(|| dir.join("data"))
}

fn bootstrap_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_DIR_NAME).join("bootstrap.json"))
}

fn read_bootstrap() -> Option<Bootstrap> {
    read_json_opt(&bootstrap_file()?).ok().flatten()
}

/// Сохраняет новую папку данных (`None` — вернуть по умолчанию).
/// Применяется после перезапуска лаунчера.
pub fn set_data_dir_override(dir: Option<&Path>) -> AppResult<()> {
    let file = bootstrap_file()
        .ok_or_else(|| AppError::Other("Не удалось определить папку конфигурации".into()))?;
    let b = Bootstrap { data_dir: dir.map(Path::to_path_buf) };
    write_json_atomic(&file, &b)
}

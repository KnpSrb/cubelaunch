//! Глобальные настройки лаунчера (`<data>/settings.json`).
//!
//! Все поля имеют значения по умолчанию (`#[serde(default)]`), поэтому старый
//! или частично заполненный файл читается без ошибок, а новые поля
//! подставляются автоматически. Секреты (пароль прокси, ключ CurseForge)
//! сюда не попадают — они будут храниться в системном keyring.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::util::{read_json_opt, write_json_atomic};
use std::path::Path;

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub language: String,
    pub appearance: Appearance,
    pub downloads: DownloadSettings,
    pub proxy: ProxySettings,
    pub java: JavaSettings,
    pub game: GameSettings,
    pub updates: UpdateSettings,
    /// Client ID приложения Azure для входа через Microsoft (не секрет).
    /// Пусто — используется вшитый при сборке.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msa_client_id: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            language: "ru".into(),
            appearance: Appearance::default(),
            downloads: DownloadSettings::default(),
            proxy: ProxySettings::default(),
            java: JavaSettings::default(),
            game: GameSettings::default(),
            updates: UpdateSettings::default(),
            msa_client_id: None,
        }
    }
}

/// Самообновление лаунчера.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    /// Проверять обновления при запуске.
    pub check_on_startup: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { check_on_startup: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum InstanceView {
    #[default]
    Grid,
    List,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    pub theme: ThemeMode,
    /// Акцентный цвет в формате `#rrggbb`.
    pub accent: String,
    pub instance_view: InstanceView,
    /// Размер иконок инстансов в сетке, px.
    pub icon_size: u32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            accent: "#4caf50".into(),
            instance_view: InstanceView::Grid,
            icon_size: 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadSettings {
    /// Число одновременных загрузок.
    pub concurrency: u32,
    /// Число повторных попыток для одного файла.
    pub retries: u32,
    /// Таймаут соединения, секунды.
    pub timeout_secs: u32,
    /// Сколько задач с загрузками (установки, модпаки, Java) идёт одновременно.
    pub max_jobs: u32,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self { concurrency: 8, retries: 3, timeout_secs: 30, max_jobs: 2 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ProxyMode {
    #[default]
    None,
    System,
    Http,
    Socks5,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProxySettings {
    pub mode: ProxyMode,
    pub host: String,
    pub port: u16,
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JavaSettings {
    /// Явно выбранный путь к java; `None` — выбирать автоматически под версию MC.
    pub path: Option<String>,
    /// Автоматически скачивать подходящую Java (Temurin), если не найдена.
    pub auto_download: bool,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub jvm_args: String,
}

impl Default for JavaSettings {
    fn default() -> Self {
        Self {
            path: None,
            auto_download: true,
            min_memory_mb: 512,
            max_memory_mb: 4096,
            jvm_args: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LauncherVisibility {
    /// Ничего не делать.
    #[default]
    Keep,
    /// Свернуть окно лаунчера.
    Minimize,
    /// Скрыть окно и вернуть после выхода из игры.
    Hide,
    /// Закрыть лаунчер.
    Close,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameSettings {
    pub window_width: u32,
    pub window_height: u32,
    pub fullscreen: bool,
    pub on_launch: LauncherVisibility,
    /// Показывать окно лога при запуске игры.
    pub show_log_on_launch: bool,
    /// Записывать время игры.
    pub record_playtime: bool,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            window_width: 854,
            window_height: 480,
            fullscreen: false,
            on_launch: LauncherVisibility::Keep,
            show_log_on_launch: true,
            record_playtime: true,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> AppResult<()> {
        let err = |m: &str| Err(AppError::InvalidSettings(m.to_string()));
        if !is_hex_color(&self.appearance.accent) {
            return err("акцентный цвет должен быть в формате #rrggbb");
        }
        if !(16..=256).contains(&self.appearance.icon_size) {
            return err("размер иконок должен быть от 16 до 256");
        }
        if !(1..=64).contains(&self.downloads.concurrency) {
            return err("число параллельных загрузок должно быть от 1 до 64");
        }
        if self.downloads.retries > 10 {
            return err("число повторов не может превышать 10");
        }
        if !(1..=8).contains(&self.downloads.max_jobs) {
            return err("число одновременных задач должно быть от 1 до 8");
        }
        if !(5..=600).contains(&self.downloads.timeout_secs) {
            return err("таймаут должен быть от 5 до 600 секунд");
        }
        if matches!(self.proxy.mode, ProxyMode::Http | ProxyMode::Socks5)
            && (self.proxy.host.trim().is_empty() || self.proxy.port == 0)
        {
            return err("для прокси нужно указать адрес и порт");
        }
        if self.java.min_memory_mb < 128 {
            return err("минимальная память не может быть меньше 128 МБ");
        }
        if self.java.max_memory_mb < self.java.min_memory_mb {
            return err("максимальная память меньше минимальной");
        }
        if self.game.window_width < 320 || self.game.window_height < 240 {
            return err("размер окна игры слишком мал");
        }
        if let Some(id) = self.msa_client_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let ok = id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
            if !ok {
                return err("Client ID должен быть GUID вида 00000000-0000-0000-0000-000000000000");
            }
        }
        Ok(())
    }

    /// Загружает настройки; при отсутствии файла возвращает значения по умолчанию.
    /// Повреждённый файл переименовывается в `settings.json.bak`, чтобы не потерять его.
    pub fn load(path: &Path) -> AppResult<Self> {
        match read_json_opt::<Settings>(path) {
            Ok(Some(s)) => Ok(s),
            Ok(None) => Ok(Self::default()),
            Err(AppError::Json(e)) => {
                tracing::warn!("settings.json повреждён ({e}), используются значения по умолчанию");
                let _ = std::fs::rename(path, path.with_extension("json.bak"));
                Ok(Self::default())
            }
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> AppResult<()> {
        self.validate()?;
        write_json_atomic(path, self)
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Settings::default().validate().unwrap();
    }

    #[test]
    fn partial_json_fills_defaults() {
        let s: Settings =
            serde_json::from_str(r#"{ "language": "en", "java": { "maxMemoryMb": 8192 } }"#)
                .unwrap();
        assert_eq!(s.language, "en");
        assert_eq!(s.java.max_memory_mb, 8192);
        assert_eq!(s.java.min_memory_mb, 512);
        assert_eq!(s.downloads.concurrency, 8);
    }

    #[test]
    fn rejects_bad_values() {
        let mut s = Settings::default();
        s.java.max_memory_mb = 256;
        assert!(s.validate().is_err());

        let mut s = Settings::default();
        s.appearance.accent = "green".into();
        assert!(s.validate().is_err());

        let mut s = Settings::default();
        s.proxy.mode = ProxyMode::Http;
        assert!(s.validate().is_err());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        assert_eq!(Settings::load(&file).unwrap(), Settings::default());

        let mut s = Settings::default();
        s.appearance.theme = ThemeMode::Light;
        s.downloads.concurrency = 16;
        s.save(&file).unwrap();
        assert_eq!(Settings::load(&file).unwrap(), s);
    }

    #[test]
    fn corrupted_file_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, "{ not json").unwrap();
        assert_eq!(Settings::load(&file).unwrap(), Settings::default());
        assert!(dir.path().join("settings.json.bak").exists());
    }
}

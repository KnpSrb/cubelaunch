//! Хранилище секретов: системный keyring (Windows Credential Manager,
//! macOS Keychain, Secret Service на Linux). Токены никогда не пишутся на диск
//! в открытом виде.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

use crate::error::{AppError, AppResult};

pub const SERVICE: &str = "Cubelaunch";

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> AppResult<Option<String>>;
    fn set(&self, key: &str, value: &str) -> AppResult<()>;
    fn delete(&self, key: &str) -> AppResult<()>;
}

fn keyring_err(e: keyring::Error) -> AppError {
    AppError::Keyring(e.to_string())
}

/// Системный keyring.
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> AppResult<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(keyring_err)?;
        // Секрет храним как байты UTF-8: на Windows `set_password` пишет UTF-16
        // и упирается в лимит 2560 байт на запись раньше времени.
        match entry.get_secret() {
            Ok(bytes) => Ok(Some(String::from_utf8(bytes).map_err(|e| AppError::Keyring(e.to_string()))?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(e)),
        }
    }

    fn set(&self, key: &str, value: &str) -> AppResult<()> {
        keyring::Entry::new(SERVICE, key)
            .and_then(|e| e.set_secret(value.as_bytes()))
            .map_err(keyring_err)
    }

    fn delete(&self, key: &str) -> AppResult<()> {
        match keyring::Entry::new(SERVICE, key).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(e)),
        }
    }
}

/// Хранилище в памяти — для тестов.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> AppResult<Option<String>> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> AppResult<()> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> AppResult<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

/// Ключ CurseForge API.
pub const CURSEFORGE_KEY: &str = "curseforge-api-key";

pub fn refresh_key(account_id: &str) -> String {
    format!("{account_id}.msa-refresh")
}

pub fn minecraft_key(account_id: &str) -> String {
    format!("{account_id}.minecraft")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Запись/чтение/удаление в настоящем keyring ОС.
    /// Запуск: `cargo test -- --ignored keyring_roundtrip`
    #[test]
    #[ignore = "использует системный keyring"]
    fn keyring_roundtrip() {
        let s = KeyringStore;
        let key = format!("cubelaunch-test-{}", uuid::Uuid::new_v4().simple());
        // Длина как у реального токена Minecraft (~1.5–2 КБ).
        let value = "x".repeat(2000);
        s.set(&key, &value).unwrap();
        assert_eq!(s.get(&key).unwrap().as_deref(), Some(value.as_str()));
        s.delete(&key).unwrap();
        assert_eq!(s.get(&key).unwrap(), None);
        s.delete(&key).unwrap(); // повторное удаление — не ошибка
    }
}

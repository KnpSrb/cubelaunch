//! Tauri-команды — единственная точка входа фронтенда в бэкенд.
//! Каждая команда возвращает `AppResult`, ошибка сериализуется в `{ kind, message, details }`.

pub mod accounts;
pub mod app;
pub mod content;
pub mod instances;
pub mod java;
pub mod modpacks;
pub mod transfer;
pub mod settings;
pub mod versions;
pub mod updates;

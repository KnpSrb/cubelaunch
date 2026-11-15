//! Manifest Mojang, version JSON, правила библиотек, загрузка клиента,
//! библиотек и assets, сборка командной строки, запуск и мониторинг игры.

pub mod arguments;
pub mod assets;
pub mod install;
pub mod launch;
pub mod libraries;
pub mod logparse;
pub mod manifest;
pub mod prepare;
pub mod rules;
pub mod session;
pub mod version;

//! Имена событий бэкенд → фронтенд. Дублируются в `src/lib/events.ts`.

pub const SETTINGS_CHANGED: &str = "settings://changed";
/// Payload: `JobInfo`.
pub const JOB_UPDATE: &str = "jobs://update";
/// Список инстансов изменился (payload пустой — фронтенд перечитывает список).
pub const INSTANCES_CHANGED: &str = "instances://changed";
/// Список аккаунтов изменился.
pub const ACCOUNTS_CHANGED: &str = "accounts://changed";
/// Payload: строка этапа входа (`waiting`, `xbox`, `xsts`, `minecraft`, `profile`).
pub const LOGIN_STAGE: &str = "accounts://login-stage";
/// Payload: `{ instanceId, lines: LogLine[] }` — новые строки лога игры.
pub const GAME_LOG: &str = "game://log";
/// Payload: `GameStatus` — игра запущена / завершена / упала.
pub const GAME_STATE: &str = "game://state";

//! Единый тип ошибки приложения.
//!
//! На фронтенд ошибка уходит как `{ kind, message, details }`. `kind` — стабильный
//! машинный код, по которому UI выбирает локализованный текст; `message` — запасной
//! русский текст на случай, если перевода для кода нет; `details` — технические подробности.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Ошибка файловой системы: {0}")]
    Io(#[from] std::io::Error),

    #[error("Не удалось разобрать JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Сетевая ошибка: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Некорректные настройки: {0}")]
    InvalidSettings(String),

    #[error("Не найдено: {0}")]
    NotFound(String),

    #[error("Операция отменена")]
    Cancelled,

    #[error("Не удалось скачать {url}: {reason}")]
    Download { url: String, reason: String },

    #[error("Контрольная сумма не совпала для {file}")]
    Checksum { file: String, expected: String, actual: String },

    #[error("Некорректные данные: {0}")]
    InvalidInput(String),

    #[error("Java не работает: {0}")]
    JavaInvalid(String),

    #[error("Не найдена Java {0}")]
    JavaMissing(u32),

    #[error("Не выбран аккаунт")]
    NoAccount,

    #[error("Игра уже запущена")]
    AlreadyRunning,

    #[error("Ошибка запуска игры: {0}")]
    Launch(String),

    #[error("{0}")]
    Content(String),

    #[error("CurseForge: {0}")]
    CurseforgeKey(String),

    #[error("Не удалось установить загрузчик модов: {0}")]
    Loader(String),

    #[error("Системное хранилище паролей недоступно: {0}")]
    Keyring(String),

    #[error("{0}")]
    Auth(AuthError),

    #[error("Обновление лаунчера: {0}")]
    Update(String),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Io(_) => "io",
            AppError::Json(_) => "json",
            AppError::Network(_) => "network",
            AppError::InvalidSettings(_) => "invalid_settings",
            AppError::NotFound(_) => "not_found",
            AppError::Cancelled => "cancelled",
            AppError::Download { .. } => "download",
            AppError::Checksum { .. } => "checksum",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::JavaInvalid(_) => "java_invalid",
            AppError::JavaMissing(_) => "java_missing",
            AppError::NoAccount => "no_account",
            AppError::AlreadyRunning => "already_running",
            AppError::Launch(_) => "launch",
            AppError::Keyring(_) => "keyring",
            AppError::Loader(_) => "loader",
            AppError::Content(_) => "content",
            AppError::CurseforgeKey(_) => "curseforge_key",
            AppError::Auth(a) => a.code(),
            AppError::Update(_) => "update",
            AppError::Other(_) => "other",
        }
    }

    fn details(&self) -> Option<String> {
        match self {
            AppError::Io(e) => Some(e.to_string()),
            AppError::Json(e) => Some(e.to_string()),
            AppError::Network(e) => Some(e.to_string()),
            AppError::InvalidSettings(s)
            | AppError::NotFound(s)
            | AppError::InvalidInput(s)
            | AppError::JavaInvalid(s)
            | AppError::Launch(s)
            | AppError::Keyring(s)
            | AppError::Loader(s)
            | AppError::Content(s)
            | AppError::CurseforgeKey(s)
            | AppError::Update(s)
            | AppError::Other(s) => Some(s.clone()),
            AppError::Auth(AuthError::Other(s)) => Some(s.clone()),
            AppError::Auth(_) => None,
            AppError::JavaMissing(m) => Some(format!(
                "Нужна Java {m}. Включите автоскачивание Java в настройках или установите её вручную."
            )),
            AppError::NoAccount | AppError::AlreadyRunning => None,
            AppError::Download { url, reason } => Some(format!("{url}\n{reason}")),
            AppError::Checksum { file, expected, actual } => {
                Some(format!("{file}\nожидалось: {expected}\nполучено: {actual}"))
            }
            AppError::Cancelled => None,
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 3)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.serialize_field("details", &self.details())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// Ошибки входа через Microsoft. Каждая — отдельный `kind` (`auth_<код>`),
/// чтобы UI мог показать понятное объяснение и что делать.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("Не задан Client ID приложения Microsoft")]
    NotConfigured,
    #[error("Вход отклонён пользователем")]
    Declined,
    #[error("Код входа истёк, начните вход заново")]
    CodeExpired,
    #[error("У этой учётной записи Microsoft нет профиля Xbox")]
    NoXbox,
    #[error("Xbox Live недоступен в стране учётной записи")]
    CountryBlocked,
    #[error("Учётной записи нужно подтверждение возраста на xbox.com")]
    AdultVerification,
    #[error("Детской учётной записи нужно разрешение в семейной группе Microsoft")]
    ChildAccount,
    #[error("Приложение не одобрено Mojang для доступа к Minecraft API")]
    AppNotApproved,
    #[error("На этой учётной записи нет Minecraft: Java Edition")]
    NoGame,
    #[error("Сессия истекла, войдите в аккаунт заново")]
    ReloginRequired,
    #[error("Ошибка входа: {0}")]
    Other(String),
}

impl AuthError {
    pub fn code(&self) -> &'static str {
        match self {
            AuthError::NotConfigured => "auth_not_configured",
            AuthError::Declined => "auth_declined",
            AuthError::CodeExpired => "auth_code_expired",
            AuthError::NoXbox => "auth_no_xbox",
            AuthError::CountryBlocked => "auth_country_blocked",
            AuthError::AdultVerification => "auth_adult_verification",
            AuthError::ChildAccount => "auth_child_account",
            AuthError::AppNotApproved => "auth_app_not_approved",
            AuthError::NoGame => "auth_no_game",
            AuthError::ReloginRequired => "auth_relogin",
            AuthError::Other(_) => "auth_other",
        }
    }
}

impl From<AuthError> for AppError {
    fn from(e: AuthError) -> Self {
        AppError::Auth(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_kind_and_details() {
        let e = AppError::NotFound("instance".into());
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "not_found");
        assert_eq!(v["details"], "instance");
        assert_eq!(v["message"], "Не найдено: instance");
    }
}

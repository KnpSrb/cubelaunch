// Имена событий бэкенд → фронтенд. Дублируют src-tauri/src/events.rs.

export const EVENTS = {
  settingsChanged: "settings://changed",
  jobUpdate: "jobs://update",
  instancesChanged: "instances://changed",
  accountsChanged: "accounts://changed",
  loginStage: "accounts://login-stage",
  gameLog: "game://log",
  gameState: "game://state",
} as const;

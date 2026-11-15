//! Запущенная игра: чтение stdout/stderr, разбор лога, пересылка в UI
//! пачками, остановка процесса, определение краша.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::logparse::{LogLevel, LogLine, LogParser};
use crate::events;

/// Сколько строк лога держим в памяти на одну игру.
const MAX_LINES: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GameState {
    Running,
    Exited,
    Crashed,
    Killed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    pub instance_id: String,
    pub state: GameState,
    pub pid: Option<u32>,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crash_report: Option<CrashReport>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LogBatch<'a> {
    instance_id: &'a str,
    lines: &'a [LogLine],
}

pub struct Session {
    pub status: Mutex<GameStatus>,
    lines: Mutex<VecDeque<LogLine>>,
    pub kill: CancellationToken,
}

impl Session {
    fn push(&self, line: LogLine) {
        let mut l = self.lines.lock().unwrap();
        if l.len() >= MAX_LINES {
            l.pop_front();
        }
        l.push_back(line);
    }

    pub fn lines(&self) -> Vec<LogLine> {
        self.lines.lock().unwrap().iter().cloned().collect()
    }
}

#[derive(Default)]
pub struct GameSessions {
    map: Mutex<HashMap<String, Arc<Session>>>,
}

impl GameSessions {
    pub fn get(&self, instance_id: &str) -> Option<Arc<Session>> {
        self.map.lock().unwrap().get(instance_id).cloned()
    }

    pub fn is_running(&self, instance_id: &str) -> bool {
        self.get(instance_id).is_some_and(|s| s.status.lock().unwrap().state == GameState::Running)
    }

    pub fn running(&self) -> Vec<GameStatus> {
        self.map
            .lock()
            .unwrap()
            .values()
            .map(|s| s.status.lock().unwrap().clone())
            .filter(|s| s.state == GameState::Running)
            .collect()
    }

    /// Регистрирует новую игру (старый лог этого инстанса заменяется).
    pub fn start(&self, instance_id: &str, pid: Option<u32>, preface: Vec<LogLine>) -> Arc<Session> {
        let s = Arc::new(Session {
            status: Mutex::new(GameStatus {
                instance_id: instance_id.to_string(),
                state: GameState::Running,
                pid,
                started_at: Utc::now(),
                ended_at: None,
                exit_code: None,
                crash_report: None,
            }),
            lines: Mutex::new(preface.into_iter().collect()),
            kill: CancellationToken::new(),
        });
        self.map.lock().unwrap().insert(instance_id.to_string(), s.clone());
        s
    }
}

pub fn info_line(text: impl Into<String>) -> LogLine {
    LogLine {
        level: LogLevel::Info,
        time: Some(chrono::Local::now().format("%H:%M:%S").to_string()),
        thread: Some("Cubelaunch".into()),
        text: text.into(),
    }
}

/// Читает поток построчно; невалидный UTF-8 (например, cp1251 от JVM) не ломает чтение.
async fn pump(reader: impl AsyncRead + Unpin, stderr: bool, tx: mpsc::UnboundedSender<(String, bool)>) {
    let mut r = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match r.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string();
                if tx.send((line, stderr)).is_err() {
                    break;
                }
            }
        }
    }
}

/// Самый свежий файл в папке, изменённый после `since`, с подходящим именем.
fn newest_since(dir: &Path, since: SystemTime, pred: impl Fn(&str) -> bool) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| pred(&e.file_name().to_string_lossy()))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(t, _)| *t >= since)
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

pub fn find_crash_report(game_dir: &Path, since: SystemTime) -> Option<CrashReport> {
    let path = newest_since(&game_dir.join("crash-reports"), since, |n| n.ends_with(".txt"))
        .or_else(|| newest_since(game_dir, since, |n| n.starts_with("hs_err_pid") && n.ends_with(".log")))?;
    let mut text = std::fs::read_to_string(&path).ok()?;
    if text.len() > 256 * 1024 {
        let mut cut = 256 * 1024;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    Some(CrashReport { path: path.display().to_string(), text })
}

/// Колбэк после завершения игры: статус и сколько она проработала.
pub type OnExit = Box<dyn FnOnce(&GameStatus, Duration) + Send>;

/// Что сделать после завершения игры.
pub struct ExitHooks {
    pub on_exit: OnExit,
}

/// Следит за процессом до завершения. Запускается в отдельной задаче.
pub async fn monitor(
    app: AppHandle,
    session: Arc<Session>,
    mut child: tokio::process::Child,
    game_dir: PathBuf,
    hooks: ExitHooks,
) {
    let instance_id = session.status.lock().unwrap().instance_id.clone();
    let started = SystemTime::now() - Duration::from_secs(2);
    let t0 = std::time::Instant::now();
    let (tx, mut rx) = mpsc::unbounded_channel();
    if let Some(out) = child.stdout.take() {
        tokio::spawn(pump(out, false, tx.clone()));
    }
    if let Some(err) = child.stderr.take() {
        tokio::spawn(pump(err, true, tx.clone()));
    }
    drop(tx);

    let mut parsers = [LogParser::default(), LogParser::default()];
    let mut batch: Vec<LogLine> = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut killed = false;
    let emit = |batch: &mut Vec<LogLine>| {
        if !batch.is_empty() {
            let _ = app.emit(events::GAME_LOG, LogBatch { instance_id: &instance_id, lines: batch });
            batch.clear();
        }
    };

    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Some((line, is_err)) => {
                    if let Some(l) = parsers[is_err as usize].feed(&line, is_err) {
                        session.push(l.clone());
                        batch.push(l);
                    }
                }
                None => break,
            },
            _ = tick.tick() => emit(&mut batch),
            _ = session.kill.cancelled(), if !killed => {
                killed = true;
                let _ = child.start_kill();
            }
        }
    }
    for p in &mut parsers {
        if let Some(l) = p.flush() {
            session.push(l.clone());
            batch.push(l);
        }
    }

    let code = child.wait().await.ok().and_then(|s| s.code());
    let crash = if killed { None } else { find_crash_report(&game_dir, started) };
    let state = if killed {
        GameState::Killed
    } else if crash.is_some() || code.is_some_and(|c| c != 0) {
        GameState::Crashed
    } else {
        GameState::Exited
    };
    let msg = match state {
        GameState::Killed => "Игра остановлена пользователем".to_string(),
        GameState::Crashed => format!("Игра завершилась с ошибкой (код {})", code.map_or("?".into(), |c| c.to_string())),
        _ => format!("Игра завершена (код {})", code.unwrap_or(0)),
    };
    let mut line = info_line(msg);
    if state == GameState::Crashed {
        line.level = LogLevel::Error;
    }
    session.push(line.clone());
    batch.push(line);
    emit(&mut batch);

    let status = {
        let mut s = session.status.lock().unwrap();
        s.state = state;
        s.exit_code = code;
        s.ended_at = Some(Utc::now());
        s.crash_report = crash;
        s.clone()
    };
    tracing::info!("игра {instance_id} завершилась: {state:?}, код {code:?}");
    (hooks.on_exit)(&status, t0.elapsed());
    let _ = app.emit(events::GAME_STATE, &status);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_new_crash_report_only() {
        let dir = tempfile::tempdir().unwrap();
        let reports = dir.path().join("crash-reports");
        std::fs::create_dir_all(&reports).unwrap();
        let since = SystemTime::now() - Duration::from_secs(1);
        assert!(find_crash_report(dir.path(), since).is_none());
        std::fs::write(reports.join("crash-2026-01-01_client.txt"), "---- Minecraft Crash Report ----").unwrap();
        let r = find_crash_report(dir.path(), since).unwrap();
        assert!(r.text.contains("Crash Report"));
        assert!(find_crash_report(dir.path(), SystemTime::now() + Duration::from_secs(60)).is_none());
    }
}

//! Параллельная загрузка файлов: проверка SHA1, докачка (`Range`), повторы с
//! экспоненциальной задержкой, отмена и агрегированный прогресс.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::{StreamExt, TryStreamExt};
use reqwest::header::RANGE;
use reqwest::StatusCode;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadTask {
    pub url: String,
    pub dest: PathBuf,
    /// Ожидаемый SHA1 (hex, нижний регистр). Если задан — файл проверяется.
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct DownloadOptions {
    pub concurrency: usize,
    pub retries: u32,
}

#[derive(Default)]
struct Counters {
    files_done: AtomicUsize,
    bytes_done: AtomicU64,
}

pub async fn sha1_file(path: &Path) -> AppResult<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> AppResult<String> {
        use std::io::Read;
        let mut f = std::fs::File::open(&path)?;
        let mut h = sha1_smol::Sha1::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(h.digest().to_string())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

/// Файл уже на месте и корректен?
pub async fn is_valid(task: &DownloadTask) -> AppResult<bool> {
    let meta = match tokio::fs::metadata(&task.dest).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    if let Some(size) = task.size {
        if meta.len() != size {
            return Ok(false);
        }
    }
    match &task.sha1 {
        Some(expected) => Ok(sha1_file(&task.dest).await?.eq_ignore_ascii_case(expected)),
        None => Ok(true),
    }
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".part");
    dest.with_file_name(name)
}

/// Одна попытка скачать файл. `counted` — сколько байт этой задачи уже учтено в прогрессе.
async fn try_download(
    client: &reqwest::Client,
    task: &DownloadTask,
    cancel: &CancellationToken,
    counters: &Counters,
    counted: &mut u64,
) -> AppResult<()> {
    if let Some(parent) = task.dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = part_path(&task.dest);
    let existing = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    // Частичный файл не больше ожидаемого размера — пробуем докачать.
    let can_resume = existing > 0 && task.size.is_none_or(|s| existing < s);

    let mut req = client.get(&task.url);
    if can_resume {
        req = req.header(RANGE, format!("bytes={existing}-"));
    }
    let resp = tokio::select! {
        r = req.send() => r?,
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
    };
    let resp = resp.error_for_status()?;

    let mut hasher = sha1_smol::Sha1::new();
    let mut file = if can_resume && resp.status() == StatusCode::PARTIAL_CONTENT {
        let prev = tokio::fs::read(&part).await?;
        hasher.update(&prev);
        counters.bytes_done.fetch_add(prev.len() as u64, Ordering::Relaxed);
        *counted += prev.len() as u64;
        tokio::fs::OpenOptions::new().append(true).open(&part).await?
    } else {
        tokio::fs::File::create(&part).await?
    };

    let mut stream = resp.bytes_stream();
    loop {
        let chunk = tokio::select! {
            c = stream.next() => c,
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        counters.bytes_done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        *counted += chunk.len() as u64;
    }
    file.flush().await?;
    drop(file);

    if let Some(expected) = &task.sha1 {
        let actual = hasher.digest().to_string();
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(AppError::Checksum {
                file: task.dest.display().to_string(),
                expected: expected.clone(),
                actual,
            });
        }
    }
    tokio::fs::rename(&part, &task.dest).await?;
    Ok(())
}

async fn fetch_one(
    client: &reqwest::Client,
    task: DownloadTask,
    opts: DownloadOptions,
    cancel: &CancellationToken,
    counters: &Counters,
) -> AppResult<()> {
    if cancel.is_cancelled() {
        return Err(AppError::Cancelled);
    }
    if is_valid(&task).await? {
        counters.bytes_done.fetch_add(task.size.unwrap_or(0), Ordering::Relaxed);
        counters.files_done.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }
    let mut attempt = 0;
    loop {
        let mut counted = 0u64;
        match try_download(client, &task, cancel, counters, &mut counted).await {
            Ok(()) => {
                // Если размер известен — выравниваем учёт (сервер мог отдать иной объём).
                if let Some(size) = task.size {
                    counters.bytes_done.fetch_sub(counted, Ordering::Relaxed);
                    counters.bytes_done.fetch_add(size, Ordering::Relaxed);
                }
                counters.files_done.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            Err(e) => {
                counters.bytes_done.fetch_sub(counted, Ordering::Relaxed);
                if matches!(e, AppError::Cancelled) {
                    return Err(e);
                }
                // 404 и подобные клиентские ошибки повторять бессмысленно.
                let fatal = matches!(&e, AppError::Network(n) if n.status().is_some_and(|s| s.is_client_error()));
                if fatal || attempt >= opts.retries {
                    tracing::warn!("загрузка {} не удалась: {e}", task.url);
                    return Err(AppError::Download { url: task.url.clone(), reason: e.to_string() });
                }
                attempt += 1;
                let delay = Duration::from_millis(500 * 2u64.pow(attempt.min(5)));
                tracing::debug!("повтор {attempt}/{} для {} через {delay:?}: {e}", opts.retries, task.url);
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = cancel.cancelled() => return Err(AppError::Cancelled),
                }
            }
        }
    }
}

/// Скачивает все задачи. Прогресс сообщается через `on_progress` (вызывается часто —
/// вызывающая сторона сама решает, как часто пересылать его в UI).
pub async fn download_all(
    client: &reqwest::Client,
    tasks: Vec<DownloadTask>,
    opts: DownloadOptions,
    cancel: &CancellationToken,
    on_progress: impl Fn(Progress) + Send + Sync,
) -> AppResult<()> {
    // Дедупликация по пути назначения: два одинаковых файла не пишем параллельно.
    let mut seen = std::collections::HashSet::new();
    let tasks: Vec<_> = tasks.into_iter().filter(|t| seen.insert(t.dest.clone())).collect();

    let files_total = tasks.len();
    let bytes_total = tasks.iter().filter_map(|t| t.size).sum();
    let counters = Arc::new(Counters::default());
    let snapshot = |c: &Counters| Progress {
        files_done: c.files_done.load(Ordering::Relaxed),
        files_total,
        bytes_done: c.bytes_done.load(Ordering::Relaxed),
        bytes_total,
    };
    on_progress(snapshot(&counters));

    // Тикер прогресса, пока идут загрузки.
    let ticker_cancel = CancellationToken::new();
    let progress_fut = async {
        let mut iv = tokio::time::interval(Duration::from_millis(150));
        loop {
            tokio::select! {
                _ = iv.tick() => on_progress(snapshot(&counters)),
                _ = ticker_cancel.cancelled() => break,
            }
        }
    };
    let work = async {
        let r = futures::stream::iter(tasks.into_iter().map(Ok))
            .try_for_each_concurrent(opts.concurrency.max(1), |t| {
                let counters = counters.clone();
                async move { fetch_one(client, t, opts, cancel, &counters).await }
            })
            .await;
        ticker_cancel.cancel();
        r
    };
    let (result, ()) = tokio::join!(work, progress_fut);
    on_progress(snapshot(&counters));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn validates_existing_file_by_sha1_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.txt");
        std::fs::write(&dest, b"hello").unwrap();
        let good = DownloadTask {
            url: String::new(),
            dest: dest.clone(),
            sha1: Some("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d".into()),
            size: Some(5),
        };
        assert!(is_valid(&good).await.unwrap());
        assert!(!is_valid(&DownloadTask { size: Some(6), ..good.clone() }).await.unwrap());
        assert!(!is_valid(&DownloadTask { sha1: Some("00".repeat(20)), ..good.clone() }).await.unwrap());
        assert!(!is_valid(&DownloadTask { dest: dir.path().join("missing"), ..good }).await.unwrap());
    }

    #[test]
    fn part_path_appends_suffix() {
        assert_eq!(part_path(Path::new("/x/client.jar")), PathBuf::from("/x/client.jar.part"));
    }

    #[tokio::test]
    async fn already_valid_files_need_no_network() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.txt");
        std::fs::write(&dest, b"hello").unwrap();
        let task = DownloadTask {
            // Недоступный адрес: если бы загрузчик полез в сеть, тест упал бы.
            url: "http://127.0.0.1:9/never".into(),
            dest,
            sha1: Some("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d".into()),
            size: Some(5),
        };
        let last = std::sync::Mutex::new(Progress::default());
        download_all(
            &reqwest::Client::new(),
            vec![task.clone(), task],
            DownloadOptions { concurrency: 4, retries: 0 },
            &CancellationToken::new(),
            |p| *last.lock().unwrap() = p,
        )
        .await
        .unwrap();
        let p = *last.lock().unwrap();
        assert_eq!((p.files_done, p.files_total, p.bytes_done, p.bytes_total), (1, 1, 5, 5));
    }

    #[tokio::test]
    async fn cancelled_before_start() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let r = download_all(
            &reqwest::Client::new(),
            vec![DownloadTask { url: "http://127.0.0.1:9/x".into(), dest: dir.path().join("x"), sha1: None, size: None }],
            DownloadOptions { concurrency: 1, retries: 0 },
            &cancel,
            |_| {},
        )
        .await;
        assert!(matches!(r, Err(AppError::Cancelled)));
    }
}

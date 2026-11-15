//! Фоновые задачи (установка версии, модпаки, Java, контент, экспорт и т. д.).
//!
//! Каждая задача имеет состояние, этап и прогресс; изменения уходят на фронтенд
//! событием `jobs://update`. Задачу можно отменить по id, упавшую — повторить.
//!
//! «Тяжёлые» задачи (сетевые загрузки) идут через очередь: одновременно
//! выполняется не больше `limit` штук (настройка `downloads.maxJobs`), остальные
//! ждут в состоянии `queued` в порядке постановки.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::queue::Progress;
use crate::error::{AppError, AppResult};
use crate::events;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    /// Ждёт свободного места в очереди.
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn is_active(self) -> bool {
        matches!(self, JobState::Queued | JobState::Running)
    }
}

/// Задачи, которые идут через очередь (качают из сети).
pub const QUEUED_KINDS: [&str; 7] =
    ["installVersion", "installContent", "updateContent", "downloadJava", "installModpack", "updateModpack", "importInstance"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: u64,
    /// Тип задачи, например `installVersion`.
    pub kind: String,
    /// Что обрабатывается (имя инстанса, версия) — показывается в UI.
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    pub state: JobState,
    /// Ключ текущего этапа (`manifest`, `versionJson`, `assetIndex`, `files`), UI переводит его.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub progress: Progress,
    /// Сериализованный `AppError`, если задача упала.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    /// Упавшую/отменённую задачу можно запустить заново.
    pub retryable: bool,
}

type Retry = Arc<dyn Fn(&Arc<Jobs>, &AppHandle) -> u64 + Send + Sync>;
type BoxBody = Box<dyn FnOnce(JobCtx) -> Pin<Box<dyn Future<Output = AppResult<()>> + Send>> + Send>;

struct Entry {
    info: JobInfo,
    cancel: CancellationToken,
    retry: Option<Retry>,
}

/// Очередь «тяжёлых» задач: FIFO с изменяемым лимитом.
struct Slots {
    limit: AtomicUsize,
    inner: Mutex<SlotsInner>,
    notify: Notify,
}

#[derive(Default)]
struct SlotsInner {
    active: usize,
    waiting: VecDeque<u64>,
}

impl Default for Slots {
    fn default() -> Self {
        Self { limit: AtomicUsize::new(2), inner: Mutex::default(), notify: Notify::new() }
    }
}

/// Занятое место в очереди; освобождается при drop.
struct SlotGuard(Arc<Jobs>);

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.0.slots.inner.lock().unwrap().active -= 1;
        self.0.slots.notify.notify_waiters();
    }
}

#[derive(Default)]
pub struct Jobs {
    entries: Mutex<HashMap<u64, Entry>>,
    next_id: AtomicU64,
    slots: Slots,
}

/// Контекст, который получает тело задачи.
#[derive(Clone)]
pub struct JobCtx {
    pub id: u64,
    pub cancel: CancellationToken,
    pub app: AppHandle,
    jobs: Arc<Jobs>,
}

impl JobCtx {
    pub fn stage(&self, stage: &str) {
        self.jobs.update(&self.app, self.id, |i| {
            i.stage = Some(stage.to_string());
            i.progress = Progress::default();
        });
    }

    pub fn progress(&self, p: Progress) {
        self.jobs.update(&self.app, self.id, |i| i.progress = p);
    }
}

impl Jobs {
    pub fn list(&self) -> Vec<JobInfo> {
        let mut v: Vec<_> = self.entries.lock().unwrap().values().map(|e| e.info.clone()).collect();
        v.sort_by_key(|j| j.id);
        v
    }

    /// Сколько тяжёлых задач может идти одновременно.
    pub fn set_limit(&self, limit: usize) {
        self.slots.limit.store(limit.max(1), Ordering::Relaxed);
        self.slots.notify.notify_waiters();
    }

    /// Есть ли активная (идущая или ждущая) задача для инстанса.
    pub fn running_for_instance(&self, instance_id: &str) -> bool {
        self.entries
            .lock()
            .unwrap()
            .values()
            .any(|e| e.info.state.is_active() && e.info.instance_id.as_deref() == Some(instance_id))
    }

    pub fn cancel(&self, id: u64) -> bool {
        match self.entries.lock().unwrap().get(&id) {
            Some(e) => {
                e.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Убирает завершённую задачу из списка.
    pub fn dismiss(&self, id: u64) {
        let mut map = self.entries.lock().unwrap();
        if map.get(&id).is_some_and(|e| !e.info.state.is_active()) {
            map.remove(&id);
        }
    }

    /// Запускает упавшую или отменённую задачу заново. Возвращает id новой задачи;
    /// старая убирается из списка.
    pub fn retry(self: &Arc<Self>, app: &AppHandle, id: u64) -> AppResult<u64> {
        let (retry, instance) = {
            let map = self.entries.lock().unwrap();
            let e = map.get(&id).ok_or_else(|| AppError::NotFound(format!("задача {id}")))?;
            if !matches!(e.info.state, JobState::Failed | JobState::Cancelled) {
                return Err(AppError::InvalidInput("повторить можно только упавшую или отменённую задачу".into()));
            }
            let retry = e.retry.clone().ok_or_else(|| AppError::InvalidInput("эту задачу нельзя повторить".into()))?;
            (retry, e.info.instance_id.clone())
        };
        if instance.as_deref().is_some_and(|i| self.running_for_instance(i)) {
            return Err(AppError::InvalidInput("для этого инстанса уже выполняется задача".into()));
        }
        self.entries.lock().unwrap().remove(&id);
        Ok(retry(self, app))
    }

    fn update(&self, app: &AppHandle, id: u64, f: impl FnOnce(&mut JobInfo)) {
        let info = {
            let mut map = self.entries.lock().unwrap();
            let Some(e) = map.get_mut(&id) else { return };
            f(&mut e.info);
            e.info.clone()
        };
        let _ = app.emit(events::JOB_UPDATE, &info);
    }

    /// Ждёт своей очереди (FIFO). Отмена во время ожидания — `Cancelled`.
    async fn acquire(self: &Arc<Self>, id: u64, cancel: &CancellationToken) -> AppResult<SlotGuard> {
        self.slots.inner.lock().unwrap().waiting.push_back(id);
        loop {
            let notified = self.slots.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut s = self.slots.inner.lock().unwrap();
                if s.active < self.slots.limit.load(Ordering::Relaxed) && s.waiting.front() == Some(&id) {
                    s.waiting.pop_front();
                    s.active += 1;
                    drop(s);
                    // Следующий в очереди тоже может пройти, если мест несколько.
                    self.slots.notify.notify_waiters();
                    return Ok(SlotGuard(self.clone()));
                }
            }
            tokio::select! {
                _ = &mut notified => {}
                _ = cancel.cancelled() => {
                    self.slots.inner.lock().unwrap().waiting.retain(|w| *w != id);
                    self.slots.notify.notify_waiters();
                    return Err(AppError::Cancelled);
                }
            }
        }
    }

    /// Запускает задачу. Тело должно быть `Clone`, чтобы задачу можно было повторить.
    pub fn spawn<F, Fut>(
        self: &Arc<Self>,
        app: &AppHandle,
        kind: &str,
        title: String,
        instance_id: Option<String>,
        body: F,
    ) -> u64
    where
        F: FnOnce(JobCtx) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = AppResult<()>> + Send + 'static,
    {
        // Запуск игры не повторяем: проверки (аккаунт, уже запущена) делает команда.
        let retry: Option<Retry> = (kind != "launch").then(|| {
            let (kind, title, instance_id, body) = (kind.to_string(), title.clone(), instance_id.clone(), body.clone());
            Arc::new(move |jobs: &Arc<Jobs>, app: &AppHandle| {
                jobs.spawn(app, &kind, title.clone(), instance_id.clone(), body.clone())
            }) as Retry
        });
        let boxed: BoxBody = Box::new(move |ctx| Box::pin(body(ctx)));
        self.spawn_inner(app, kind, title, instance_id, retry, boxed)
    }

    fn spawn_inner(
        self: &Arc<Self>,
        app: &AppHandle,
        kind: &str,
        title: String,
        instance_id: Option<String>,
        retry: Option<Retry>,
        body: BoxBody,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = CancellationToken::new();
        let queued = QUEUED_KINDS.contains(&kind);
        let info = JobInfo {
            id,
            kind: kind.into(),
            title,
            instance_id,
            state: if queued { JobState::Queued } else { JobState::Running },
            stage: None,
            progress: Progress::default(),
            error: None,
            started_at: Utc::now(),
            finished_at: None,
            retryable: retry.is_some(),
        };
        self.entries.lock().unwrap().insert(id, Entry { info: info.clone(), cancel: cancel.clone(), retry });
        let _ = app.emit(events::JOB_UPDATE, &info);

        let ctx = JobCtx { id, cancel: cancel.clone(), app: app.clone(), jobs: self.clone() };
        let jobs = self.clone();
        tauri::async_runtime::spawn(async move {
            let app = ctx.app.clone();
            let result = async {
                let _slot = if queued {
                    let slot = jobs.acquire(id, &cancel).await?;
                    jobs.update(&app, id, |i| {
                        i.state = JobState::Running;
                        i.started_at = Utc::now();
                    });
                    Some(slot)
                } else {
                    None
                };
                body(ctx).await
            }
            .await;
            jobs.update(&app, id, |i| {
                i.finished_at = Some(Utc::now());
                match result {
                    Ok(()) => i.state = JobState::Done,
                    Err(AppError::Cancelled) => i.state = JobState::Cancelled,
                    Err(e) => {
                        tracing::error!("задача {id} ({}) завершилась с ошибкой: {e}", i.title);
                        i.state = JobState::Failed;
                        i.error = serde_json::to_value(&e).ok();
                    }
                }
            });
        });
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Очередь пропускает не больше `limit` задач и соблюдает порядок.
    #[tokio::test]
    async fn slots_limit_and_order() {
        let jobs = Arc::new(Jobs::default());
        jobs.set_limit(1);
        let cancel = CancellationToken::new();
        let a = jobs.acquire(1, &cancel).await.unwrap();
        let j2 = jobs.clone();
        let c2 = cancel.clone();
        let order = Arc::new(Mutex::new(Vec::new()));
        let o2 = order.clone();
        let t2 = tokio::spawn(async move {
            let g = j2.acquire(2, &c2).await.unwrap();
            o2.lock().unwrap().push(2);
            g
        });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        let j3 = jobs.clone();
        let o3 = order.clone();
        let c3 = CancellationToken::new();
        let c3b = c3.clone();
        let t3 = tokio::spawn(async move { j3.acquire(3, &c3b).await.map(|_| o3.lock().unwrap().push(3)) });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(order.lock().unwrap().is_empty(), "лимит 1 — остальные ждут");
        // Отмена ждущей задачи убирает её из очереди.
        c3.cancel();
        assert!(matches!(t3.await.unwrap(), Err(AppError::Cancelled)));
        drop(a);
        let g2 = t2.await.unwrap();
        assert_eq!(*order.lock().unwrap(), vec![2]);
        jobs.set_limit(2);
        let g4 = jobs.acquire(4, &cancel).await.unwrap();
        drop((g2, g4));
        assert_eq!(jobs.slots.inner.lock().unwrap().active, 0);
        assert!(jobs.slots.inner.lock().unwrap().waiting.is_empty());
    }
}

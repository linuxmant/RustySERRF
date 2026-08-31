use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(uuid::Uuid);

impl JobId {
    pub fn new() -> Self {
        JobId(uuid::Uuid::new_v4())
    }

    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(JobId(uuid::Uuid::parse_str(s)?))
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum JobEvent {
    Queued,
    Progress { stage: String, current: usize, total: usize },
    Completed,
    Failed { error: String },
}

impl JobEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobEvent::Completed | JobEvent::Failed { .. })
    }
}

pub struct CompletedJob {
    pub compound_labels: Vec<String>,
    pub sample_type: Vec<Option<String>>,
    pub output: serrf_core::PipelineOutput,
}

enum JobResult {
    Pending,
    Done(CompletedJob),
    Errored(String),
}

struct JobHandle {
    events: tokio::sync::watch::Sender<JobEvent>,
    result: JobResult,
}

#[derive(Debug)]
pub enum JobStoreLookup<R> {
    NotReady,
    Failed(String),
    Ready(R),
}

#[derive(Clone)]
pub struct JobStore {
    jobs: Arc<RwLock<HashMap<JobId, JobHandle>>>,
}

impl JobStore {
    pub fn new() -> Self {
        JobStore { jobs: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn create(&self) -> (JobId, tokio::sync::watch::Receiver<JobEvent>) {
        let id = JobId::new();
        let (tx, rx) = tokio::sync::watch::channel(JobEvent::Queued);
        self.jobs.write().unwrap().insert(id, JobHandle { events: tx, result: JobResult::Pending });
        (id, rx)
    }

    pub fn push_progress(&self, id: JobId, event: JobEvent) {
        if let Some(handle) = self.jobs.read().unwrap().get(&id) {
            let _ = handle.events.send(event);
        }
    }

    pub fn complete(&self, id: JobId, completed: CompletedJob) {
        if let Some(handle) = self.jobs.write().unwrap().get_mut(&id) {
            handle.result = JobResult::Done(completed);
            let _ = handle.events.send(JobEvent::Completed);
        }
    }

    pub fn fail(&self, id: JobId, error: String) {
        if let Some(handle) = self.jobs.write().unwrap().get_mut(&id) {
            let _ = handle.events.send(JobEvent::Failed { error: error.clone() });
            handle.result = JobResult::Errored(error);
        }
    }

    pub fn subscribe(&self, id: JobId) -> Option<tokio::sync::watch::Receiver<JobEvent>> {
        self.jobs.read().unwrap().get(&id).map(|h| h.events.subscribe())
    }

    pub fn with_completed<R>(&self, id: JobId, f: impl FnOnce(&CompletedJob) -> R) -> Option<JobStoreLookup<R>> {
        let jobs = self.jobs.read().unwrap();
        let handle = jobs.get(&id)?;
        Some(match &handle.result {
            JobResult::Pending => JobStoreLookup::NotReady,
            JobResult::Errored(e) => JobStoreLookup::Failed(e.clone()),
            JobResult::Done(completed) => JobStoreLookup::Ready(f(completed)),
        })
    }
}

impl Default for JobStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_completed() -> CompletedJob {
        CompletedJob {
            compound_labels: vec!["c1".to_string()],
            sample_type: vec![Some("qc".to_string())],
            output: serrf_core::PipelineOutput {
                raw: ndarray::Array2::zeros((1, 1)),
                serrf: ndarray::Array2::zeros((1, 1)),
                qc_rsd_raw: vec![0.1],
                qc_rsd_serrf: vec![0.01],
                validate_rsd_raw: std::collections::HashMap::new(),
                validate_rsd_serrf: std::collections::HashMap::new(),
                sample_order: vec!["s1".to_string()],
            },
        }
    }

    #[test]
    fn a_new_job_starts_queued() {
        let store = JobStore::new();
        let (id, rx) = store.create();
        assert_eq!(*rx.borrow(), JobEvent::Queued);
        assert!(store.subscribe(id).is_some());
    }

    #[test]
    fn subscribe_returns_none_for_an_unknown_job() {
        let store = JobStore::new();
        assert!(store.subscribe(JobId::new()).is_none());
    }

    #[test]
    fn push_progress_updates_the_watched_event() {
        let store = JobStore::new();
        let (id, mut rx) = store.create();
        store.push_progress(id, JobEvent::Progress { stage: "SERRF normalization".into(), current: 3, total: 10 });
        assert!(rx.has_changed().unwrap());
        assert_eq!(
            *rx.borrow_and_update(),
            JobEvent::Progress { stage: "SERRF normalization".into(), current: 3, total: 10 }
        );
    }

    #[test]
    fn push_progress_on_an_unknown_job_is_a_silent_no_op() {
        let store = JobStore::new();
        store.push_progress(JobId::new(), JobEvent::Progress { stage: "x".into(), current: 1, total: 1 });
    }

    #[test]
    fn complete_sets_a_terminal_event_and_stores_the_result() {
        let store = JobStore::new();
        let (id, mut rx) = store.create();
        store.complete(id, sample_completed());
        assert!(rx.has_changed().unwrap());
        assert_eq!(*rx.borrow_and_update(), JobEvent::Completed);
        match store.with_completed(id, |c| c.compound_labels.clone()) {
            Some(JobStoreLookup::Ready(labels)) => assert_eq!(labels, vec!["c1".to_string()]),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn fail_sets_a_terminal_event_with_the_error_message() {
        let store = JobStore::new();
        let (id, mut rx) = store.create();
        store.fail(id, "boom".to_string());
        assert!(rx.has_changed().unwrap());
        assert_eq!(*rx.borrow_and_update(), JobEvent::Failed { error: "boom".to_string() });
        match store.with_completed(id, |c| c.compound_labels.clone()) {
            Some(JobStoreLookup::Failed(msg)) => assert_eq!(msg, "boom"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn with_completed_reports_not_ready_before_completion() {
        let store = JobStore::new();
        let (id, _rx) = store.create();
        match store.with_completed(id, |c| c.compound_labels.clone()) {
            Some(JobStoreLookup::NotReady) => {}
            other => panic!("expected NotReady, got {other:?}"),
        }
    }

    #[test]
    fn with_completed_reports_not_found_for_an_unknown_job() {
        let store = JobStore::new();
        assert!(store.with_completed(JobId::new(), |c| c.compound_labels.clone()).is_none());
    }
}

use crate::app::AppState;
use crate::error::ApiError;
use crate::job::{JobEvent, JobId};
use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_core::Stream;

pub async fn events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    let job_id = JobId::parse(&id).map_err(|_| ApiError::BadRequest("invalid job id".to_string()))?;
    let (history, mut rx) = state.jobs.subscribe(job_id).ok_or(ApiError::NotFound)?;

    let stream = async_stream::stream! {
        // Replay everything that happened before this client connected -- a `watch`
        // channel alone only ever exposes the LATEST value, so without this a client
        // connecting even a moment after the job starts would permanently miss every
        // earlier stage (see job.rs's history-replay tests for the underlying bug this
        // fixes). `rx` was subscribed atomically with capturing `history`, so it's
        // guaranteed to only fire for events strictly after the last one replayed here.
        let mut already_terminal = false;
        for event in &history {
            yield Ok(to_sse_event(event));
            if event.is_terminal() {
                already_terminal = true;
                break;
            }
        }
        if !already_terminal {
            loop {
                if rx.changed().await.is_err() {
                    break;
                }
                let event = rx.borrow_and_update().clone();
                yield Ok(to_sse_event(&event));
                if event.is_terminal() {
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(3))))
}

fn to_sse_event(event: &JobEvent) -> Event {
    let name = match event {
        JobEvent::Queued => "queued",
        JobEvent::Progress { .. } => "progress",
        JobEvent::Completed => "completed",
        JobEvent::Failed { .. } => "failed",
    };
    Event::default().event(name).json_data(event).unwrap()
}

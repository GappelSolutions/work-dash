use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use tokio_stream::wrappers::{BroadcastStream, IntervalStream};
use tokio_stream::{Stream, StreamExt};

use crate::state::AppState;
use crate::time;

pub async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let events = BroadcastStream::new(state.bus.subscribe()).filter_map(|msg| match msg {
        Ok(ev) => {
            let data = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".to_string());
            Some(Ok(Event::default().event(ev.event_name()).data(data)))
        }
        // Slow subscriber missed messages — it resyncs via a plain GET; drop the gap.
        Err(_lagged) => None,
    });

    let pings = IntervalStream::new(tokio::time::interval(Duration::from_secs(20)))
        .map(|_| Ok::<_, Infallible>(Event::default().event("ping").data("{}")));

    let hello = tokio_stream::once(Ok::<_, Infallible>(
        Event::default()
            .event("hello")
            .data(serde_json::json!({ "server_time": time::now_iso() }).to_string()),
    ));

    Sse::new(hello.chain(events.merge(pings)))
}

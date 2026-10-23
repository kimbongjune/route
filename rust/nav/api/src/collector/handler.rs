//! `GET /events`: 가중치 갱신 이벤트를 SSE로 보낸다

use std::convert::Infallible;

use axum::extract::State;
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::state::AppState;

/// 접속한 브라우저마다 broadcast 수신자를 하나 만들어 이벤트를 흘려보낸다. 수신이 밀려 놓친 이벤트는 건너뛴다
pub async fn events(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.events.subscribe())
        .filter_map(|received| received.ok())
        .map(|event| event.to_sse());
    Sse::new(stream).keep_alive(KeepAlive::default())
}

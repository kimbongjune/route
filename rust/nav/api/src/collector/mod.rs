//! 실시간 가중치 반영. 폴링 태스크가 만든 배율 묶음을 받아 `RouteDb`에 적용하고, 적용 결과를 SSE로 브라우저에 알린다.
//! 폴링 태스크와 적용 태스크는 모두 `nav-api` 프로세스 안의 tokio 태스크이다

pub mod handler;
pub mod incident;
pub mod traffic;
pub mod utic;

use std::convert::Infallible;
use std::sync::Arc;

use std::time::{Duration, Instant};

use axum::response::sse::Event;
use routedb_core::route::{FactorKind, RouteDb, WeightUpdate};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

/// 폴링이 이 횟수만큼 연속으로 실패하면 그 종류의 배율을 1.0으로 되돌린다
pub const STALE_POLLS: u32 = 3;

/// 가중치 갱신 한 번의 결과. SSE `weights` 이벤트로 보낸다
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WeightEvent {
    /// 배율을 바꾼 링크 수
    pub applied: usize,
    /// 그래프에 없는 LINK_ID 수
    pub unknown: usize,
    /// 막힌 링크 수
    pub blocked: usize,
    /// 가중치 계산과 customize에 걸린 시간(ms)
    pub elapsed_ms: f64,
    /// 갱신 순번. 1부터 센다
    pub sequence: u64,
    /// 배율을 되돌린 갱신이면 true
    pub reset: bool,
}

/// 적용 태스크에 보내는 묶음
#[derive(Clone, Debug, PartialEq)]
pub enum Batch {
    /// 링크별 배율
    Updates(Vec<WeightUpdate>),
    /// 한 종류의 배율을 모두 1.0으로 되돌림. 폴링이 오래 실패했을 때 보낸다
    Reset(FactorKind),
}

/// 폴링 성공 시각으로 오래됨을 판정한다. 마지막 성공 뒤 `interval × STALE_POLLS`가 지나면 되돌리며, 한 번 되돌린 뒤에는
/// 다시 성공할 때까지 되돌리지 않는다
#[derive(Clone, Copy, Debug)]
pub struct Freshness {
    last_ok: Option<Instant>,
    reset_sent: bool,
}

impl Freshness {
    pub fn new() -> Self {
        Self {
            last_ok: Some(Instant::now()),
            reset_sent: false,
        }
    }

    /// 폴링 성공
    pub fn succeeded(&mut self, now: Instant) {
        self.last_ok = Some(now);
        self.reset_sent = false;
    }

    /// 폴링 실패. 되돌려야 하면 true
    pub fn failed(&mut self, now: Instant, interval: Duration) -> bool {
        let stale = self
            .last_ok
            .is_none_or(|ok| now.duration_since(ok) >= interval * STALE_POLLS);
        if stale && !self.reset_sent {
            self.reset_sent = true;
            return true;
        }
        false
    }
}

impl Default for Freshness {
    fn default() -> Self {
        Self::new()
    }
}

impl WeightEvent {
    /// SSE 이벤트. 이름은 weights, 데이터는 JSON이다
    pub fn to_sse(&self) -> Result<Event, Infallible> {
        Ok(Event::default()
            .event("weights")
            .json_data(self)
            .expect("WeightEvent 직렬화 실패"))
    }
}

/// 배율 묶음을 보내는 쪽. 폴링 태스크가 갖는다
pub type UpdateSender = mpsc::Sender<Batch>;

/// 적용 태스크를 시작한다. 받은 묶음마다 블로킹 스레드에서 `update_weights`를 호출하고 결과를 `events`로 보낸다.
/// 보내는 쪽이 모두 사라지면 태스크가 끝난다
pub fn start(db: Arc<RouteDb>, events: broadcast::Sender<WeightEvent>) -> UpdateSender {
    let (sender, mut inbox) = mpsc::channel::<Batch>(16);
    tokio::spawn(async move {
        let mut sequence = 0u64;
        while let Some(batch) = inbox.recv().await {
            let db = Arc::clone(&db);
            let reset = matches!(batch, Batch::Reset(_));
            let report = tokio::task::spawn_blocking(move || match batch {
                Batch::Updates(updates) => db.update_weights(&updates),
                Batch::Reset(kind) => db.reset_factors(kind),
            })
            .await
            .expect("가중치 갱신 작업이 중단됨");
            sequence += 1;
            // 구독자가 없으면 보내기가 실패하지만 갱신은 이미 적용되었으므로 무시한다
            let _ = events.send(WeightEvent {
                applied: report.applied,
                unknown: report.unknown,
                blocked: report.blocked,
                elapsed_ms: report.elapsed_ms,
                sequence,
                reset,
            });
        }
    });
    sender
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_after_three_missed_polls() {
        let interval = Duration::from_secs(300);
        let start = Instant::now();
        let mut fresh = Freshness::new();
        fresh.succeeded(start);
        // 두 번 실패까지는 유지하고 세 번째 간격이 지나면 한 번만 되돌린다
        assert!(!fresh.failed(start + interval, interval));
        assert!(!fresh.failed(start + interval * 2, interval));
        assert!(fresh.failed(start + interval * 3, interval));
        assert!(!fresh.failed(start + interval * 4, interval));
        // 다시 성공하면 처음부터 센다
        fresh.succeeded(start + interval * 5);
        assert!(!fresh.failed(start + interval * 6, interval));
        assert!(fresh.failed(start + interval * 8, interval));
    }
}

//! 돌발 경고. 돌발정보 폴링 태스크가 국가교통정보센터 돌발 이벤트와 UTIC 돌발정보를 받을 때마다 목록을 교체하고,
//! 경로가 돌발 링크를 지나면 돌발 좌표의 경로 위 위치에 경고한다. 받은 지 INCIDENT_MAX_AGE가 지난 자료는 쓰지 않는다

use std::collections::HashSet;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use super::{nearest_on_route, route_position};

/// 이보다 오래된 자료는 쓰지 않는다
pub const INCIDENT_MAX_AGE: Duration = Duration::from_secs(30 * 60);
/// 돌발 좌표가 경로에서 이 거리보다 멀면 돌발 링크를 지나도 쓰지 않는다(m)
pub const INCIDENT_LINK_M: f64 = 500.0;

/// 돌발 하나
#[derive(Clone, Debug, PartialEq)]
pub struct Incident {
    /// 경고 문구
    pub text: String,
    pub links: Vec<u64>,
    pub lat: f64,
    pub lon: f64,
}

/// 경로가 지나는 돌발
#[derive(Clone, Debug, PartialEq)]
pub struct IncidentHit {
    pub at_m: f64,
    pub incident: Incident,
}

struct Snapshot {
    incidents: Vec<Incident>,
    fetched: Instant,
}

/// 마지막으로 받은 돌발. 폴링 태스크가 바꾸고 경로 응답이 읽는다
pub struct IncidentStore {
    snapshot: RwLock<Option<Snapshot>>,
}

impl Default for IncidentStore {
    fn default() -> Self {
        Self::new()
    }
}

impl IncidentStore {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(None),
        }
    }

    pub fn replace(&self, incidents: Vec<Incident>) {
        *self.snapshot.write().expect("돌발 잠금 실패") = Some(Snapshot {
            incidents,
            fetched: Instant::now(),
        });
    }

    /// 경로가 돌발 링크를 지나는 돌발. 거리 순서이다
    pub fn hits(
        &self,
        links: &[u64],
        geometry: &[(f64, f64)],
        distance_m: f64,
    ) -> Vec<IncidentHit> {
        let guard = self.snapshot.read().expect("돌발 잠금 실패");
        let Some(snap) = guard
            .as_ref()
            .filter(|s| s.fetched.elapsed() <= INCIDENT_MAX_AGE)
        else {
            return Vec::new();
        };
        let on_route: HashSet<u64> = links.iter().copied().collect();
        let position = route_position(geometry, distance_m);
        let mut out: Vec<IncidentHit> = snap
            .incidents
            .iter()
            .filter(|i| i.links.iter().any(|l| on_route.contains(l)))
            .filter_map(|i| {
                let (d, at_m) = nearest_on_route(geometry, &position, i.lat, i.lon)?;
                (d <= INCIDENT_LINK_M).then(|| IncidentHit {
                    at_m,
                    incident: i.clone(),
                })
            })
            .collect();
        out.sort_by(|a, b| a.at_m.total_cmp(&b.at_m));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_match() {
        let store = IncidentStore::new();
        let incident = |text: &str, link: u64, lat: f64| Incident {
            text: text.to_string(),
            links: vec![link],
            lat,
            lon: 127.0045,
        };
        store.replace(vec![
            incident("가", 100, 37.5003),
            incident("나", 200, 37.5003),
            incident("다", 100, 37.6),
        ]);
        // 링크 100을 지나는 경로에서 좌표가 33m인 가만 들어가고, 링크가 다른 나와 11km 떨어진 다는 빠진다
        let hits = store.hits(&[100], &[(37.5, 127.0), (37.5, 127.01)], 880.0);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].incident.text, "가");
    }
}

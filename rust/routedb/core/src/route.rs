use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use cch::bundle::{CchBundle, CchView, MetricBundle, MetricView};
use cch::path::PathQuery;
use cch::{Cch, Metric};

use crate::alley::{AlleyFile, PORTAL_IN, PORTAL_OUT};
use crate::file::GraphFile;
use crate::hierarchy;
use crate::linegraph;
use crate::profile::{self, ProfileInput};
use crate::route_alley::{alley_factor, reverse_pieces};
use crate::snap::{LinkIndex, Snap};
use serde::{Deserialize, Serialize};

/// 가중치 프로파일
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// 거리
    Distance,
    /// 최소시간
    Time,
    /// 무료도로
    Free,
}

/// 경로 위 지점의 종류
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PoiKind {
    /// 무인교통단속카메라
    Camera,
    /// 과속방지턱
    Bump,
}

/// 경로 위 지점. at_m은 출발점부터의 거리이다.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RoutePoi {
    pub kind: PoiKind,
    /// 카메라번호 또는 방지턱 지점번호
    pub id: u32,
    pub at_m: f64,
    pub lat: f64,
    pub lon: f64,
    /// 카메라 제한속도(km/h). 0은 없음
    pub limit: u8,
    /// 카메라 단속구분 비트(1 속도, 2 신호, 4 통행위반, 8 불법주정차, 16 기타). 방지턱은 1이 연속형이다
    pub enforce: u8,
}

/// 경로 위 구간. 출발점부터의 시작·끝 거리이다.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Section {
    pub from_m: f64,
    pub to_m: f64,
}

/// 경로 위 큰길 링크 하나의 구간과 통행 속도. 통행 속도는 제한속도를 소통정보 속도 배율과 가변속도 배율 중 큰 값으로 나눈 값이다
#[derive(Clone, Debug, PartialEq)]
pub struct SpeedSpan {
    pub from_m: f64,
    pub to_m: f64,
    pub kmh: f64,
}

/// 지나는 요금소. 영업소코드와 요금 링크 가운데의 출발점부터의 거리이다
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TollPass {
    pub unit: u32,
    pub at_m: f64,
}

/// 회전 방향. 각도는 진행 방향 기준이며 시계 방향이 양수이다.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Turn {
    /// |각도| < 20°
    Straight,
    /// 20° 이상 50° 미만
    SlightRight,
    SlightLeft,
    /// 50° 이상 150° 미만
    Right,
    Left,
    /// 150° 이상
    UTurn,
    /// 본선에서 램프로 나가는 출구. 각도가 50° 미만일 때이며 방향은 각도의 부호이다
    ExitRight,
    ExitLeft,
}

/// 안내 지점. 출발점부터의 거리와 회전 방향, 들어가는 도로명이다.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Step {
    pub at_m: f64,
    /// 안내 지점의 위경도. 앞 링크의 끝점이다
    pub lat: f64,
    pub lon: f64,
    /// 안내 지점 교차로의 이름 NODE_NAME. 없으면 빈 문자열
    pub node_name: String,
    pub turn: Turn,
    /// 진행 방향 변화(°). 시계 방향이 양수
    pub angle: f64,
    /// 안내 전 도로명
    pub from_name: String,
    /// 안내 후 도로명
    pub to_name: String,
    /// 들어가는 도로의 도로등급. 램프로 들어가면 램프 뒤 본선의 도로등급
    pub to_rank: u8,
    /// 들어가는 도로의 안내 표지 색
    pub sign: SignColor,
    /// 교차로에서 나갈 수 있는 분기 수. 돌아가는 방향은 세지 않는다
    pub branches: u8,
    /// 분기가 BRANCHES_FOR_CLOCK 이상이면 시계 방향(1~12시)
    pub clock: Option<u8>,
}

/// 시계 방향으로 안내하는 최소 분기 수
pub const BRANCHES_FOR_CLOCK: u8 = 4;

/// 안내 표지의 바탕색. 도로표지규칙 제8조에 따라 고속국도·일반국도·자동차전용도로는 녹색, 그 밖의 도시지역 도로는 청색이다
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SignColor {
    Green,
    Blue,
}

/// 들어가는 도로의 도로등급으로 표지 색을 정한다. 101 고속국도, 102 도시고속국도, 103 일반국도는 녹색, 그 밖은 청색
pub fn sign_color(road_rank: u8) -> SignColor {
    if (101..=103).contains(&road_rank) {
        SignColor::Green
    } else {
        SignColor::Blue
    }
}

/// 회전 방향을 정할 때 쓰는 링크의 도로명 번호, 길이, 시작·끝 방위각(°, 북 0, 동 90)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkHeading {
    pub name: u16,
    /// 도로등급 ROAD_RANK
    pub rank: u8,
    pub length: f64,
    pub start: f64,
    pub end: f64,
    /// 연결로(램프) 여부
    pub ramp: bool,
    /// 앞 링크의 끝 교차로와 이 링크의 시작 교차로가 달라 건너가는 통행으로 이어졌는지
    pub bridged: bool,
}

/// 안내 지점이 되는 링크 번호와 회전
#[derive(Clone, Debug, PartialEq)]
pub struct TurnPoint {
    pub link: usize,
    pub angle: f64,
    pub turn: Turn,
    pub from_name: u16,
    pub to_name: u16,
    /// 들어가는 도로의 도로등급. 램프로 들어가면 램프 뒤 본선의 도로등급
    pub to_rank: u8,
}

/// 교차로 안 짧은 링크로 보는 길이(m). 이보다 짧은 링크는 회전 방향을 정하는 데 쓰지 않는다
pub const SHORT_LINK_M: f64 = 30.0;
/// 교차로를 가로지르는 링크로 보는 길이(m). 이보다 짧고 앞뒤 링크의 도로명이 같은 다른 이름의 링크는 건너뛴다
pub const CROSSING_LINK_M: f64 = 60.0;

/// 방위각 a에서 b로의 변화(°)를 -180~180으로 정규화한다. 시계 방향이 양수이다.
pub fn turn_angle(a: f64, b: f64) -> f64 {
    let d = (b - a).rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

/// 각도로 회전 방향을 정한다.
/// 진행 방향 변화를 시계 방향(1~12시)으로 바꾼다. 30°마다 한 시간이며 반올림한다
pub fn clock_hour(angle: f64) -> u8 {
    let hour = ((angle + 360.0) / 30.0).round() as u32 % 12;
    if hour == 0 { 12 } else { hour as u8 }
}

/// 교차로 node에서 나가는 분기 수. SHORT_LINK_M보다 짧은 링크는 교차로 안 링크로 보고 그 끝에서 이어서 세며,
/// 들어온 방향에서 150° 이상 돌아가는 링크는 세지 않는다. 회전 금지와 상관없이 도로가 있는 분기를 센다
pub fn count_branches(
    first_out: &[u32],
    head: &[u32],
    length: &[f64],
    node: usize,
    end_bearing: f64,
    start_bearing_of: impl Fn(usize) -> f64,
) -> u8 {
    let mut n = 0;
    let mut seen = vec![node];
    let mut queue = vec![(node, 0usize)];
    while let Some((v, depth)) = queue.pop() {
        for e in first_out[v]..first_out[v + 1] {
            let e = e as usize;
            if turn_angle(end_bearing, start_bearing_of(e)).abs() >= 150.0 {
                continue;
            }
            let w = head[e] as usize;
            if length[e] < SHORT_LINK_M && depth < 3 {
                if !seen.contains(&w) {
                    seen.push(w);
                    queue.push((w, depth + 1));
                }
            } else {
                n += 1;
            }
        }
    }
    n
}

pub fn classify(angle: f64) -> Turn {
    let a = angle.abs();
    if a < 20.0 {
        Turn::Straight
    } else if a >= 150.0 {
        Turn::UTurn
    } else if a < 50.0 {
        if angle > 0.0 {
            Turn::SlightRight
        } else {
            Turn::SlightLeft
        }
    } else if angle > 0.0 {
        Turn::Right
    } else {
        Turn::Left
    }
}

/// 두 점 사이의 방위각(°). 북이 0, 동이 90이다.
pub fn bearing(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dy = (b.0 - a.0) * 111_320.0;
    let dx = (b.1 - a.1) * 111_320.0 * 0.809_017;
    dx.atan2(dy).to_degrees().rem_euclid(360.0)
}

/// 좌표 목록의 시작 방위각과 끝 방위각. 각각 처음과 마지막 span(m) 구간으로 잰다.
pub fn end_bearings(points: &[(f64, f64)], span: f64) -> (f64, f64) {
    let total = planar_length(points);
    let start = bearing(points[0], point_at(points, span.min(total)));
    let end = bearing(
        point_at(points, (total - span).max(0.0)),
        *points.last().unwrap(),
    );
    (start, end)
}

/// 링크 열에서 안내 지점을 찾는다.
///
/// SHORT_LINK_M보다 짧은 링크와 CROSSING_LINK_M보다 짧으면서 앞뒤 링크가 같은 도로명인 다른 이름의 링크는 교차로 안 링크로 보고
/// 건너뛰며, 그 앞의 긴 링크 끝 방위각과 뒤의 긴 링크 시작 방위각으로
/// 각도를 잰다. 각도가 50° 이상이거나, 20° 이상이면서 도로명이 바뀌면 안내 지점이고 위치는 앞의 긴 링크 끝이다.
/// 도로명만 바뀌는 직진과 같은 도로명에서 50° 미만으로 꺾이는 곳은 안내하지 않는다.
/// 본선에서 램프로 들어가는 50° 미만의 분기는 출구이고, 램프에서 본선으로 들어가는 50° 미만의 합류와
/// 램프 끝에서 건너가는 통행으로 이어지는 회차는 안내하지 않는다. 램프로 들어가는 안내의 뒤 도로명은
/// 램프 뒤에 처음 나오는 본선의 이름이며, 램프에서 램프로 넘어갈 때는 도로명이 바뀌어도 안내하지 않는다.
pub fn turn_points(links: &[LinkHeading]) -> Vec<TurnPoint> {
    let mut out = Vec::new();
    let mut prev: Option<usize> = None;
    for (i, l) in links.iter().enumerate() {
        let inner = i != 0 && i != links.len() - 1;
        // 넓은 도로가 옆길과 만나는 교차로는 길이 30m를 넘는 횡단 링크가 옆길의 이름을 갖는다.
        // 60m 미만이고 앞뒤 링크가 같은 도로명인 다른 이름의 링크는 교차로 안 링크로 본다
        let crossing = inner
            && l.length < CROSSING_LINK_M
            && links[i - 1].name == links[i + 1].name
            && links[i - 1].name != l.name;
        if inner && (l.length < SHORT_LINK_M || crossing) {
            continue;
        }
        if let Some(p) = prev {
            let angle = turn_angle(links[p].end, l.start);
            let bridged = links[p + 1..=i].iter().any(|x| x.bridged);
            let small = angle.abs() < 50.0;
            let turn = if links[p].ramp && (bridged || (!l.ramp && small)) {
                None
            } else if !links[p].ramp && l.ramp && small {
                Some(if angle < 0.0 {
                    Turn::ExitLeft
                } else {
                    Turn::ExitRight
                })
            } else {
                Some(classify(angle))
            };
            // 램프로 들어가면 안내하는 도로명은 램프 뒤에 처음 나오는 본선의 이름이다
            let to = if l.ramp {
                links[i..].iter().find(|x| !x.ramp).unwrap_or(l)
            } else {
                l
            };
            let to_name = to.name;
            // 램프에서 램프로 넘어갈 때는 도로명이 바뀌어도 안내하지 않는다
            let name_changed = !(links[p].ramp && l.ramp) && links[p].name != to_name;
            if let Some(turn) = turn {
                let slight = matches!(turn, Turn::SlightLeft | Turn::SlightRight);
                if turn != Turn::Straight && (!slight || name_changed) {
                    out.push(TurnPoint {
                        link: p,
                        angle,
                        turn,
                        from_name: links[p].name,
                        to_name,
                        to_rank: to.rank,
                    });
                }
            }
        }
        prev = Some(i);
    }
    out
}

/// 같은 도로명이 이어지는 구간
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RoadSegment {
    /// 도로명. 없으면 빈 문자열
    pub name: String,
    pub from_m: f64,
    pub to_m: f64,
    /// 구간의 링크 수
    pub links: usize,
    /// 구간 첫 링크의 도로등급 ROAD_RANK. 골목은 108이다
    pub rank: u8,
}

/// 간선 열의 도로명 번호와 지나는 길이를 받아 같은 도로명이 이어지는 구간으로 합친다.
/// 반환값은 (도로명 번호, 시작 거리, 끝 거리, 링크 수)이다.
pub fn merge_runs(names: &[u16], lengths: &[f64]) -> Vec<(u16, f64, f64, usize)> {
    let mut runs: Vec<(u16, f64, f64, usize)> = Vec::new();
    let mut walked = 0.0;
    for (&name, &len) in names.iter().zip(lengths) {
        match runs.last_mut() {
            Some(run) if run.0 == name => {
                run.2 += len;
                run.3 += 1;
            }
            _ => runs.push((name, walked, walked + len, 1)),
        }
        walked += len;
    }
    runs
}

/// 경로
#[derive(Clone, Debug, Serialize)]
pub struct Route {
    pub profile: Profile,
    pub from: Snap,
    pub to: Snap,
    /// 출발 스냅 지점부터 도착 스냅 지점까지의 거리(m)
    pub distance_m: f64,
    /// 링크 길이를 제한속도로 나눈 시간에 소통정보 속도 배율을 곱한 값의 합(초)
    pub duration_s: f64,
    /// 지나는 LINK_ID 열
    pub links: Vec<u64>,
    /// 경로 좌표 (위도, 경도)
    pub geometry: Vec<(f64, f64)>,
    /// 지나는 요금소. 바로 앞 링크에도 있던 영업소코드는 같은 요금소이므로 한 번만 넣는다
    pub tolls: Vec<TollPass>,
    /// 경로 위 카메라와 방지턱. 불법주정차만 단속하는 카메라는 뺀다
    pub pois: Vec<RoutePoi>,
    /// 같은 도로명이 이어지는 구간
    pub segments: Vec<RoadSegment>,
    /// 안내 지점. 회전하거나 도로명이 바뀌는 곳이다
    pub steps: Vec<Step>,
    /// 스쿨존 구간. 스쿨존 데이터를 적재하기 전까지 비어 있다
    pub school_zones: Vec<Section>,
    /// 경로 링크의 통과제한높이 최솟값(cm). 0은 제한 없음
    pub min_height_cm: u16,
    /// 경로 링크의 통과제한하중 최솟값(kg). 0은 제한 없음
    pub min_weight_kg: u16,
    /// 경로 링크에 있는 통행제한차량 코드
    pub rest_veh: Vec<u8>,
    /// 큰길 링크마다의 구간과 통행 속도. 골목은 없고 응답 JSON에는 넣지 않는다
    #[serde(skip)]
    pub speeds: Vec<SpeedSpan>,
}

/// 그래프 파일, 링크 R-tree, CCH 구조, 프로파일별 metric
/// 프로파일 하나의 metric. 파일을 mmap으로 연 것이거나 customize하여 메모리에 만든 것이다
pub enum MetricSource {
    File(MetricBundle),
    Memory(Metric),
}

impl MetricSource {
    pub fn view(&self) -> MetricView<'_> {
        match self {
            MetricSource::File(bundle) => bundle.view(),
            MetricSource::Memory(metric) => metric.view(),
        }
    }
}

/// 프로파일 3개의 metric 묶음. `RouteDb`가 `Arc`로 갖고 있다가 가중치를 갱신하면 새 묶음으로 바꾼다
pub struct Metrics {
    sources: [Arc<MetricSource>; 3],
}

impl Metrics {
    pub fn view(&self, profile: Profile) -> MetricView<'_> {
        self.sources[profile as usize].view()
    }
}

/// 링크 하나의 실시간 배율. 통행 시간에 속도 배율과 가변속도 배율 중 큰 값과 돌발 배율을 곱한다. 전면 통제는 돌발 배율 무한대이다.
/// None인 배율은 이전 값을 유지하므로 소통정보, 돌발정보, 가변속도제한을 따로 반영할 수 있다
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeightUpdate {
    pub link_id: u64,
    pub speed_factor: Option<f32>,
    pub incident_factor: Option<f32>,
    /// 가변속도제한 배율. 제한속도 ÷ 가변 제한속도이며 1.0 아래로 내리지 않는다
    pub vsl_factor: Option<f32>,
}

/// 실시간 배율의 종류
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactorKind {
    /// 소통정보의 속도 배율
    Speed,
    /// 돌발정보의 돌발 배율
    Incident,
    /// 가변속도제한(VSL) 배율
    Vsl,
}

/// 간선마다 실시간 배율 세 가지. 기본 1.0이다
struct Factors {
    speed: Vec<f32>,
    incident: Vec<f32>,
    vsl: Vec<f32>,
}

impl Factors {
    fn new(n: usize) -> Self {
        Self {
            speed: vec![1.0; n],
            incident: vec![1.0; n],
            vsl: vec![1.0; n],
        }
    }

    /// 통행 속도 배율. 소통정보 속도와 가변 제한속도 중 느린 쪽이다
    fn speed(&self, e: usize) -> f32 {
        self.speed[e].max(self.vsl[e])
    }

    /// 간선마다 시간 배율
    fn time(&self) -> Vec<f32> {
        (0..self.speed.len())
            .map(|e| self.speed(e) * self.incident[e])
            .collect()
    }
}

/// `update_weights` 결과
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UpdateReport {
    /// 배율을 바꾼 링크 수
    pub applied: usize,
    /// 그래프에 없는 LINK_ID 수
    pub unknown: usize,
    /// 배율이 무한대라 막힌 링크 수
    pub blocked: usize,
    /// 가중치 계산과 customize에 걸린 시간(ms)
    pub elapsed_ms: f64,
}

pub struct RouteDb {
    pub file: GraphFile,
    pub index: LinkIndex,
    /// 간선마다 출발 교차로 번호
    pub tail: Vec<u32>,
    cch: CchBundle,
    /// customize용 CCH 구조. 파일을 메모리로 읽은 것이다
    structure: Cch,
    /// 현재 metric 묶음. 갱신하면 `Arc`를 통째로 바꾼다
    metrics: RwLock<Arc<Metrics>>,
    /// 파일에서 연 정적 metric 묶음. 배율을 되돌릴 때 사용한다
    base_metrics: Arc<Metrics>,
    /// 간선마다 속도·돌발·가변속도 배율. 기본 1.0이다
    factors: RwLock<Factors>,
    /// LINK_ID에서 간선 번호
    edge_of_link: HashMap<u64, u32>,
    /// 골목 파일. 없으면 골목 없이 탐색한다
    pub(crate) alleys: Option<AlleyFile>,
}

/// 경로 탐색기. CCH 쿼리 버퍼를 갖고 있어 여러 경로에 재사용한다.
pub struct Router<'a> {
    db: &'a RouteDb,
    metrics: &'a Metrics,
    query: PathQuery<'a>,
    /// 출발·도착이 가까우면 큰길과 골목을 함께 탐색한다(route_local.rs). 대안경로 탐색에서는 끈다
    pub local: bool,
}

/// 좌표 목록의 평면 길이(m)
pub(crate) fn planar_length(points: &[(f64, f64)]) -> f64 {
    points.windows(2).map(|w| planar_distance(w[0], w[1])).sum()
}

pub(crate) fn planar_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dy = (b.0 - a.0) * 111_320.0;
    let dx = (b.1 - a.1) * 111_320.0 * 0.809_017;
    (dx * dx + dy * dy).sqrt()
}

/// 좌표 목록에서 길이 비율 from..to 부분을 잘라낸다. 양 끝은 선분 위를 보간한 점이다.
pub fn sub_polyline(points: &[(f64, f64)], from: f64, to: f64) -> Vec<(f64, f64)> {
    let total = planar_length(points);
    if points.len() < 2 || total == 0.0 {
        return points.to_vec();
    }
    let (start, end) = (from.clamp(0.0, 1.0) * total, to.clamp(0.0, 1.0) * total);
    let mut out = vec![point_at(points, start)];
    let mut walked = 0.0;
    for w in points.windows(2) {
        let seg = planar_distance(w[0], w[1]);
        if walked > start && walked < end {
            out.push(w[0]);
        }
        walked += seg;
    }
    out.push(point_at(points, end));
    out
}

/// 좌표 목록에서 시작점부터 at(m) 떨어진 점
pub fn point_at(points: &[(f64, f64)], at: f64) -> (f64, f64) {
    let mut walked = 0.0;
    for w in points.windows(2) {
        let seg = planar_distance(w[0], w[1]);
        if walked + seg >= at && seg > 0.0 {
            let t = ((at - walked) / seg).clamp(0.0, 1.0);
            return (
                w[0].0 + (w[1].0 - w[0].0) * t,
                w[0].1 + (w[1].1 - w[0].1) * t,
            );
        }
        walked += seg;
    }
    *points.last().unwrap()
}

impl RouteDb {
    /// 그래프 파일, CCH 구조, 거리·최소시간·무료도로 metric 순서의 경로를 받아 연다.
    pub fn open(graph: &Path, cch_struct: &Path, metrics: [&Path; 3]) -> io::Result<Self> {
        let file = GraphFile::open(graph)?;
        let index = LinkIndex::build(file.geom_first(), file.geom_lat(), file.geom_lon());
        let tail = linegraph::tails(file.first_out());
        let cch = CchBundle::open(cch_struct)?;
        let structure = Cch::load_struct(cch_struct)?;
        let base_metrics = Arc::new(Metrics {
            sources: [
                Arc::new(MetricSource::File(MetricBundle::open(metrics[0])?)),
                Arc::new(MetricSource::File(MetricBundle::open(metrics[1])?)),
                Arc::new(MetricSource::File(MetricBundle::open(metrics[2])?)),
            ],
        });
        let edge_of_link = file
            .link_id()
            .iter()
            .enumerate()
            .map(|(e, &id)| (id, e as u32))
            .collect();
        let factors = Factors::new(file.edge_count());
        Ok(Self {
            file,
            index,
            tail,
            cch,
            structure,
            metrics: RwLock::new(Arc::clone(&base_metrics)),
            base_metrics,
            factors: RwLock::new(factors),
            edge_of_link,
            alleys: None,
        })
    }

    /// 간선 e의 좌표 (위도, 경도) 목록
    pub fn edge_points(&self, e: usize) -> Vec<(f64, f64)> {
        let (s, t) = (
            self.file.geom_first()[e] as usize,
            self.file.geom_first()[e + 1] as usize,
        );
        (s..t)
            .map(|i| (self.file.geom_lat()[i], self.file.geom_lon()[i]))
            .collect()
    }

    /// 스냅 위치가 간선 시작점에서 얼마나 떨어졌는지를 간선 길이 기준 비율(0~1)로 계산한다.
    pub fn fraction(&self, snap: &Snap) -> f64 {
        let planar = planar_length(&self.edge_points(snap.edge as usize));
        if planar == 0.0 {
            0.0
        } else {
            (snap.offset_m / planar).clamp(0.0, 1.0)
        }
    }

    /// CCH 구조의 view. `router`에 넘긴다.
    pub fn cch_view(&self) -> CchView<'_> {
        self.cch.view()
    }

    /// 경로 탐색기를 만든다. 쿼리 버퍼를 교차로 수만큼 할당하므로 한 번 만들어 재사용한다.
    /// 현재 metric 묶음. 요청이 끝날 때까지 `Arc`를 갖고 있으면 그 사이 갱신이 있어도 같은 묶음으로 탐색한다
    pub fn metrics(&self) -> Arc<Metrics> {
        Arc::clone(&self.metrics.read().expect("metrics 잠금 실패"))
    }

    /// 간선마다 현재 시간 배율. 속도 배율과 가변속도 배율 중 큰 값에 돌발 배율을 곱한 값이다
    pub fn factors(&self) -> Vec<f32> {
        self.factors.read().expect("factors 잠금 실패").time()
    }

    /// LINK_ID의 간선 번호
    pub fn edge_of_link(&self, link_id: u64) -> Option<u32> {
        self.edge_of_link.get(&link_id).copied()
    }

    pub fn router<'a>(&'a self, cch_view: &'a CchView<'a>, metrics: &'a Metrics) -> Router<'a> {
        Router {
            db: self,
            metrics,
            query: PathQuery::new(cch_view),
            local: true,
        }
    }

    /// 링크별 실시간 배율을 반영한다. 배율을 바꾼 뒤 최소시간과 무료도로 가중치를 다시 계산하여 새 버퍼에 customize하고
    /// metric 묶음의 `Arc`를 바꾼다. 거리 metric은 그대로이다. 진행 중인 탐색은 이전 묶음으로 끝난다
    pub fn update_weights(&self, updates: &[WeightUpdate]) -> UpdateReport {
        let started = Instant::now();
        let (applied, unknown, blocked, factors) = {
            let mut guard = self.factors.write().expect("factors 잠금 실패");
            let (mut applied, mut unknown) = (0, 0);
            for update in updates {
                match self.edge_of_link.get(&update.link_id) {
                    Some(&e) => {
                        if let Some(f) = update.speed_factor {
                            guard.speed[e as usize] = f;
                        }
                        if let Some(f) = update.incident_factor {
                            guard.incident[e as usize] = f;
                        }
                        if let Some(f) = update.vsl_factor {
                            guard.vsl[e as usize] = f;
                        }
                        applied += 1;
                    }
                    None => unknown += 1,
                }
            }
            let factors = guard.time();
            let blocked = factors.iter().filter(|f| f.is_infinite()).count();
            (applied, unknown, blocked, factors)
        };
        let new_metrics = self.customized_metrics(&factors);
        *self.metrics.write().expect("metrics 잠금 실패") = Arc::new(new_metrics);
        UpdateReport {
            applied,
            unknown,
            blocked,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        }
    }

    /// 한 종류의 배율만 모두 1.0으로 되돌리고 다시 customize한다. 다른 종류의 배율은 유지된다
    pub fn reset_factors(&self, kind: FactorKind) -> UpdateReport {
        let started = Instant::now();
        let (blocked, factors) = {
            let mut guard = self.factors.write().expect("factors 잠금 실패");
            let target = match kind {
                FactorKind::Speed => &mut guard.speed,
                FactorKind::Incident => &mut guard.incident,
                FactorKind::Vsl => &mut guard.vsl,
            };
            target.iter_mut().for_each(|f| *f = 1.0);
            let factors = guard.time();
            let blocked = factors.iter().filter(|f| f.is_infinite()).count();
            (blocked, factors)
        };
        let new_metrics = self.customized_metrics(&factors);
        *self.metrics.write().expect("metrics 잠금 실패") = Arc::new(new_metrics);
        UpdateReport {
            applied: 0,
            unknown: 0,
            blocked,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        }
    }

    /// 실시간 배율을 모두 1.0으로 되돌리고 파일에서 연 정적 metric으로 돌아간다
    pub fn reset_weights(&self) {
        let mut guard = self.factors.write().expect("factors 잠금 실패");
        *guard = Factors::new(guard.speed.len());
        drop(guard);
        *self.metrics.write().expect("metrics 잠금 실패") = Arc::clone(&self.base_metrics);
    }

    /// 배율을 반영한 최소시간·무료도로 가중치로 customize한 metric 묶음. 거리 metric은 현재 것을 그대로 쓴다
    fn customized_metrics(&self, factors: &[f32]) -> Metrics {
        let file = &self.file;
        let input = ProfileInput {
            turn_first_out: file.turn_first_out(),
            turn_head: file.turn_head(),
            turn_weight: file.turn_weight(),
            via_first: file.via_first(),
            via: file.via(),
            length: file.length(),
            max_spd: file.max_spd(),
            toll_first: file.toll_first(),
            toll_unit: file.toll_unit(),
            road_rank: file.road_rank(),
            factor: Some(factors),
        };
        let customizer = self.structure.customizer();
        let time = customizer.customize(&input.time_weights());
        let free = customizer.customize(&input.free_weights());
        let distance = Arc::clone(&self.metrics.read().expect("metrics 잠금 실패").sources[0]);
        Metrics {
            sources: [
                distance,
                Arc::new(MetricSource::Memory(time)),
                Arc::new(MetricSource::Memory(free)),
            ],
        }
    }
}

/// 대안경로를 찾을 때 이미 찾은 경로의 링크에 곱하는 배율
pub const ALTERNATIVE_PENALTY: f64 = 1.5;
/// 대안이 이미 찾은 경로들과 겹치는 길이의 비율 상한. 이보다 많이 겹치면 다른 경로로 보지 않는다
pub const ALTERNATIVE_MAX_SHARE: f64 = 0.8;
/// 대안의 비용(최소시간·무료도로는 시간, 최단거리는 거리) 상한. 기본 경로의 배수
pub const ALTERNATIVE_MAX_STRETCH: f64 = 1.3;
/// 대안 하나를 찾기 위해 customize를 다시 하는 최대 횟수
const ALTERNATIVE_MAX_TRIES: usize = 4;

impl RouteDb {
    /// penalized 간선을 지나는 통행에 `ALTERNATIVE_PENALTY`를 곱한 가중치로 customize한 metric.
    /// 최소시간·무료도로는 링크 시간 배율에 곱하고 최단거리는 통행 거리 가중치에 곱한다
    fn penalized_metric(&self, profile: Profile, penalized: &[bool]) -> Metric {
        let file = &self.file;
        let customizer = self.structure.customizer();
        if profile == Profile::Distance {
            let mut weights = profile::distance_weights(file.turn_weight());
            let turn_head = file.turn_head();
            for (t, w) in weights.iter_mut().enumerate() {
                let hit = penalized[turn_head[t] as usize]
                    || file.via_of(t).iter().any(|&e| penalized[e as usize]);
                if hit {
                    *w =
                        ((*w as f64 * ALTERNATIVE_PENALTY).round() as u32).min(cch::INF_WEIGHT - 1);
                }
            }
            return customizer.customize(&weights);
        }
        let factors: Vec<f32> = {
            let guard = self.factors.read().expect("factors 잠금 실패");
            guard
                .time()
                .into_iter()
                .enumerate()
                .map(|(e, f)| {
                    if penalized[e] {
                        f * ALTERNATIVE_PENALTY as f32
                    } else {
                        f
                    }
                })
                .collect()
        };
        let input = ProfileInput {
            turn_first_out: file.turn_first_out(),
            turn_head: file.turn_head(),
            turn_weight: file.turn_weight(),
            via_first: file.via_first(),
            via: file.via(),
            length: file.length(),
            max_spd: file.max_spd(),
            toll_first: file.toll_first(),
            toll_unit: file.toll_unit(),
            road_rank: file.road_rank(),
            factor: Some(&factors),
        };
        if profile == Profile::Free {
            customizer.customize(&input.free_weights())
        } else {
            customizer.customize(&input.time_weights())
        }
    }

    /// 경로의 비용. 최단거리는 거리(m), 나머지는 시간(초)
    fn route_cost(route: &Route) -> f64 {
        if route.profile == Profile::Distance {
            route.distance_m
        } else {
            route.duration_s
        }
    }

    /// 경로가 penalized 간선과 겹치는 길이(m)
    fn shared_length(&self, route: &Route, penalized: &[bool]) -> f64 {
        let length = self.file.length();
        route
            .links
            .iter()
            .filter_map(|&link| self.edge_of_link(link))
            .filter(|&e| penalized[e as usize])
            .map(|e| length[e as usize])
            .sum()
    }

    /// 기본 경로 base의 대안경로를 count개까지 찾는다. 이미 찾은 경로의 링크에 배율을 곱한 가중치로 다시 customize하여
    /// 같은 출발·도착을 탐색하고, 기존 경로들과 `ALTERNATIVE_MAX_SHARE` 넘게 겹치거나 비용이 `ALTERNATIVE_MAX_STRETCH`를 넘으면
    /// 그 경로도 배율에 넣고 다시 시도하며, 같은 경로가 다시 나오거나 시도 횟수를 다 쓰면 끝낸다
    pub fn alternatives(
        &self,
        base: &Route,
        from: (f64, f64),
        from_heading: Option<f64>,
        to: (f64, f64),
        count: usize,
    ) -> Vec<Route> {
        let mut penalized = vec![false; self.file.edge_count()];
        let mark = |route: &Route, penalized: &mut Vec<bool>| {
            for &link in &route.links {
                if let Some(e) = self.edge_of_link(link) {
                    penalized[e as usize] = true;
                }
            }
        };
        mark(base, &mut penalized);
        let base_cost = Self::route_cost(base);
        let cch_view = self.cch_view();
        let mut found: Vec<Route> = Vec::new();
        let mut tries = 0;
        while found.len() < count && tries < ALTERNATIVE_MAX_TRIES {
            tries += 1;
            let metric = Arc::new(MetricSource::Memory(
                self.penalized_metric(base.profile, &penalized),
            ));
            let metrics = Metrics {
                sources: [Arc::clone(&metric), Arc::clone(&metric), metric],
            };
            let mut router = self.router(&cch_view, &metrics);
            router.local = false;
            let Some(route) = router.route_with_heading(from, from_heading, to, base.profile)
            else {
                break;
            };
            if route.links == base.links || found.iter().any(|r| r.links == route.links) {
                break;
            }
            let share = self.shared_length(&route, &penalized) / route.distance_m.max(1.0);
            let stretch = Self::route_cost(&route) / base_cost.max(1e-9);
            mark(&route, &mut penalized);
            if share <= ALTERNATIVE_MAX_SHARE && stretch <= ALTERNATIVE_MAX_STRETCH {
                found.push(route);
            }
        }
        found
    }
}

/// 출발 방위각과 간선 방위각의 차이가 이 각도를 넘는 출발 후보는 뺀다. 반대 방향 간선은 180도 차이이다
pub const HEADING_TOLERANCE_DEG: f64 = 90.0;

/// 두 방위각의 차이(0~180도)
pub fn heading_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

impl Router<'_> {
    /// 출발·도착 위경도로 경로를 구한다. 가까운 간선 5개씩의 25개 조합 중 출발 스냅 거리, 경로 거리, 도착 스냅 거리의 합이 가장 작은 조합이다.
    pub fn route(&mut self, from: (f64, f64), to: (f64, f64), profile: Profile) -> Option<Route> {
        self.route_with_heading(from, None, to, profile)
    }

    /// 출발 방위각이 있으면 그 방향과 `HEADING_TOLERANCE_DEG` 안에서 맞는 출발 간선만 후보로 한다. 맞는 간선이 없으면 방위각 없이 구한다.
    pub fn route_with_heading(
        &mut self,
        from: (f64, f64),
        from_heading: Option<f64>,
        to: (f64, f64),
        profile: Profile,
    ) -> Option<Route> {
        let db = self.db;
        let file = &db.file;
        let (turn_first_out, turn_head, turn_weight, length) = (
            file.turn_first_out(),
            file.turn_head(),
            file.turn_weight(),
            file.length(),
        );
        let metric_view = &self.metrics.view(profile);
        let query = &mut self.query;
        let mut froms = db.index.nearest_n(file.into(), from.0, from.1, 5);
        if let Some(heading) = from_heading {
            let matched: Vec<Snap> = froms
                .iter()
                .filter(|s| heading_diff(s.bearing_deg, heading) <= HEADING_TOLERANCE_DEG)
                .cloned()
                .collect();
            if !matched.is_empty() {
                froms = matched;
            }
        }
        let tos = db.index.nearest_n(file.into(), to.0, to.1, 5);
        // 출발·도착이 가까우면 큰길과 골목을 함께 탐색한 경로를 쓴다
        if self.local
            && let Some(route) = db.local_route(profile, from, from_heading, &froms, to, &tos)
        {
            return Some(route);
        }
        // 큰길보다 골목에 가까우면 골목망의 포털을 출발·도착 후보로 한다
        let factor = alley_factor(profile);
        let nearest = |snaps: &[Snap]| {
            snaps
                .iter()
                .map(|s| s.distance_m)
                .fold(f64::INFINITY, f64::min)
        };
        let from_alley = db.alley_end(from, nearest(&froms), PORTAL_OUT, factor, from_heading);
        let to_alley = db.alley_end(to, nearest(&tos), PORTAL_IN, factor, None);
        let from_cands: Vec<(Snap, &[crate::alley::Piece])> = match &from_alley {
            Some(a) => a
                .candidates
                .iter()
                .map(|(s, p)| (s.clone(), p.as_slice()))
                .collect(),
            None => froms.into_iter().map(|s| (s, &[][..])).collect(),
        };
        let to_cands: Vec<(Snap, &[crate::alley::Piece])> = match &to_alley {
            Some(a) => a
                .candidates
                .iter()
                .map(|(s, p)| (s.clone(), p.as_slice()))
                .collect(),
            None => tos.into_iter().map(|s| (s, &[][..])).collect(),
        };
        let mut best: Option<(usize, usize, f64, Vec<u32>, f64)> = None;
        for (i, (from, _)) in from_cands.iter().enumerate() {
            for (j, (to, _)) in to_cands.iter().enumerate() {
                let (from_frac, to_frac) = (db.fraction(from), db.fraction(to));
                let found = if from.edge == to.edge && from_frac <= to_frac {
                    Some((
                        length[from.edge as usize] * (to_frac - from_frac),
                        vec![from.edge],
                    ))
                } else {
                    query.path(metric_view, from.edge, to.edge).map(|nodes| {
                        let turns = hierarchy::edges_of_node_path(
                            turn_first_out,
                            turn_head,
                            turn_weight,
                            &nodes,
                        );
                        let mut edges = vec![from.edge];
                        for &t in &turns {
                            edges.extend_from_slice(file.via_of(t));
                            edges.push(turn_head[t]);
                        }
                        let between: f64 = edges[1..edges.len() - 1]
                            .iter()
                            .map(|&e| length[e as usize])
                            .sum();
                        (
                            length[from.edge as usize] * (1.0 - from_frac)
                                + between
                                + length[to.edge as usize] * to_frac,
                            edges,
                        )
                    })
                };
                if let Some((total, edges)) = found {
                    let cost = from.distance_m + total + to.distance_m;
                    if best.as_ref().is_none_or(|b| cost < b.4) {
                        best = Some((i, j, total, edges, cost));
                    }
                }
            }
        }
        // 출발과 도착이 같은 골목망 안이면 골목만 지나는 경로와 비교한다
        if let (Some(fa), Some(ta)) = (&from_alley, &to_alley)
            && let Some((len, pieces)) = db.alley_only(fa, ta)
        {
            let cost = fa.snap.distance_m + len * factor + ta.snap.distance_m;
            if best.as_ref().is_none_or(|b| cost < b.4) {
                return Some(db.alley_route(profile, fa, ta, &pieces));
            }
        }
        let (i, j, distance_m, edges, _) = best?;
        let route = db.build(
            profile,
            from_cands[i].0.clone(),
            to_cands[j].0.clone(),
            distance_m,
            &edges,
        );
        Some(db.splice(
            route,
            from_cands[i].1,
            &reverse_pieces(to_cands[j].1),
            from_alley.as_ref().map(|a| &a.snap),
            to_alley.as_ref().map(|a| &a.snap),
        ))
    }
}

impl RouteDb {
    /// 간선 열로 Route를 만든다. 출발·도착 간선은 스냅 위치 비율만큼만 포함한다.
    pub fn build(
        &self,
        profile: Profile,
        from: Snap,
        to: Snap,
        distance_m: f64,
        edges: &[u32],
    ) -> Route {
        let file = &self.file;
        let (length, max_spd) = (file.length(), file.max_spd());
        let (from_frac, to_frac) = (self.fraction(&from), self.fraction(&to));
        let single = edges.len() == 1;
        // 소통정보 속도 배율과 가변속도 배율 중 큰 값. 제한속도 시간에 곱하면 통행 속도로 지나는 시간이다. 돌발 배율은 경로 선택용이라 곱하지 않는다
        let factors = self.factors.read().expect("factors 잠금 실패");
        let mut speeds = Vec::with_capacity(edges.len());
        let mut geometry = Vec::new();
        let mut pois = Vec::new();
        let mut tolls: Vec<TollPass> = Vec::new();
        let mut prev: &[u32] = &[];
        let mut duration_ms = 0.0;
        let mut walked = 0.0;
        let mut link_ends: Vec<(f64, f64)> = Vec::with_capacity(edges.len());
        let (mut min_h, mut min_w, mut veh): (u16, u16, Vec<u8>) = (0, 0, Vec::new());
        let mut names = Vec::with_capacity(edges.len());
        let mut parts = Vec::with_capacity(edges.len());
        let mut headings = Vec::with_capacity(edges.len());
        for (i, &e) in edges.iter().enumerate() {
            let e = e as usize;
            // 이 간선에서 지나는 길이 비율
            let lo = if i == 0 { from_frac } else { 0.0 };
            let hi = if i == edges.len() - 1 { to_frac } else { 1.0 };
            let part = if single { to_frac - from_frac } else { hi - lo };
            let points = self.edge_points(e);
            let piece = sub_polyline(&points, lo, hi);
            let (start, end) = end_bearings(&piece, 15.0);
            headings.push(LinkHeading {
                name: file.road_name()[e],
                rank: file.road_rank()[e],
                length: part * length[e],
                start,
                end,
                ramp: file.ramp()[e] == 1,
                bridged: i > 0 && self.tail[e] != file.head()[edges[i - 1] as usize],
            });
            geometry.extend(piece.iter().skip(if geometry.is_empty() { 0 } else { 1 }));
            let speed = factors.speed(e);
            duration_ms += profile::link_time_ms(length, max_spd, e) * part * speed as f64;
            speeds.push(SpeedSpan {
                from_m: walked,
                to_m: walked + part * length[e],
                kmh: max_spd[e] as f64 / speed as f64,
            });
            // 요금소
            let cur = file.tolls_of(e);
            tolls.extend(
                cur.iter()
                    .filter(|u| !prev.contains(u))
                    .map(|&unit| TollPass {
                        unit,
                        at_m: walked + part * length[e] / 2.0,
                    }),
            );
            prev = cur;
            // 통과제한
            let (h, w, v) = (file.rest_h()[e], file.rest_w()[e], file.rest_veh()[e]);
            if h > 0 && (min_h == 0 || h < min_h) {
                min_h = h;
            }
            if w > 0 && (min_w == 0 || w < min_w) {
                min_w = w;
            }
            if v > 0 && !veh.contains(&v) {
                veh.push(v);
            }
            // 지점. 간선 안 위치 비율이 지나는 범위 안인 것만 넣고 거리는 출발점부터 잰다
            let planar = planar_length(&points);
            for p in file.poi_range(e) {
                let at = file.poi_at()[p] as f64 / 10_000.0;
                if at < lo || at > hi || file.poi_enforce()[p] == 8 && file.poi_kind()[p] == 1 {
                    continue;
                }
                let (lat, lon) = point_at(&points, at * planar);
                pois.push(RoutePoi {
                    kind: if file.poi_kind()[p] == 1 {
                        PoiKind::Camera
                    } else {
                        PoiKind::Bump
                    },
                    id: file.poi_id()[p],
                    at_m: walked + (at - lo) * length[e],
                    lat,
                    lon,
                    limit: file.poi_limit()[p],
                    enforce: file.poi_enforce()[p],
                });
            }
            walked += part * length[e];
            names.push(file.road_name()[e]);
            parts.push(part * length[e]);
            link_ends.push(*piece.last().expect("링크 좌표가 비어 있음"));
        }
        // 안내 지점. 위치는 앞의 긴 링크가 끝나는 거리이다
        let mut ends = Vec::with_capacity(parts.len());
        let mut acc = 0.0;
        for p in &parts {
            acc += p;
            ends.push(acc);
        }
        let start_bearing_of = |x: usize| end_bearings(&self.edge_points(x), 15.0).0;
        // 안내 지점마다 분기 수를 세고, 분기가 하나(들어온 방향 제외)면 고를 길이 없으므로 꺾여도 안내하지 않는다
        let steps = turn_points(&headings)
            .into_iter()
            .filter_map(|t| {
                let branches = count_branches(
                    file.first_out(),
                    file.head(),
                    length,
                    file.head()[edges[t.link] as usize] as usize,
                    headings[t.link].end,
                    start_bearing_of,
                );
                if branches <= 1 {
                    return None;
                }
                Some(Step {
                    at_m: ends[t.link],
                    lat: link_ends[t.link].0,
                    lon: link_ends[t.link].1,
                    node_name: file
                        .node_name_of(file.head()[edges[t.link] as usize] as usize)
                        .to_string(),
                    turn: t.turn,
                    angle: t.angle,
                    from_name: file.name(t.from_name as usize).to_string(),
                    to_name: file.name(t.to_name as usize).to_string(),
                    to_rank: t.to_rank,
                    sign: sign_color(t.to_rank),
                    branches,
                    clock: (branches >= BRANCHES_FOR_CLOCK).then(|| clock_hour(t.angle)),
                })
            })
            .collect();
        let mut first_link = 0;
        let segments = merge_runs(&names, &parts)
            .into_iter()
            .map(|(name, from_m, to_m, links)| {
                let rank = file.road_rank()[edges[first_link] as usize];
                first_link += links;
                RoadSegment {
                    name: file.name(name as usize).to_string(),
                    from_m,
                    to_m,
                    links,
                    rank,
                }
            })
            .collect();
        veh.sort_unstable();
        Route {
            profile,
            from,
            to,
            distance_m,
            duration_s: duration_ms / 1000.0,
            links: edges.iter().map(|&e| file.link_id()[e as usize]).collect(),
            geometry,
            tolls,
            pois,
            segments,
            steps,
            school_zones: Vec::new(),
            speeds,
            min_height_cm: min_h,
            min_weight_kg: min_w,
            rest_veh: veh,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_colors() {
        assert_eq!(sign_color(101), SignColor::Green);
        assert_eq!(sign_color(102), SignColor::Green);
        assert_eq!(sign_color(103), SignColor::Green);
        assert_eq!(sign_color(104), SignColor::Blue);
        assert_eq!(sign_color(107), SignColor::Blue);
    }

    #[test]
    fn clock_hours_and_branches() {
        assert_eq!(clock_hour(0.0), 12);
        assert_eq!(clock_hour(-10.0), 12);
        assert_eq!(clock_hour(30.0), 1);
        assert_eq!(clock_hour(-30.0), 11);
        assert_eq!(clock_hour(90.0), 3);
        assert_eq!(clock_hour(-100.0), 9);
        assert_eq!(clock_hour(175.0), 6);
        // 북쪽으로 들어온 교차로 0에서 왼쪽(270°) 100m, 되돌아가는 링크(180°), 20m 교차로 안 링크(0°)로 교차로 1에 가고
        // 교차로 1에서 직진(0°) 100m, 오른쪽(90°) 100m, 되돌아가는 링크(180°)
        let first_out = [0u32, 3, 6, 6, 6, 6];
        let head = [2u32, 3, 1, 4, 5, 0];
        let length = [100.0, 100.0, 20.0, 100.0, 100.0, 20.0];
        let bearing = [270.0, 180.0, 0.0, 0.0, 90.0, 180.0];
        assert_eq!(
            count_branches(&first_out, &head, &length, 0, 0.0, |x| bearing[x]),
            3
        );
        // 교차로 1에서 바로 세면 직진과 오른쪽 2개
        assert_eq!(
            count_branches(&first_out, &head, &length, 1, 0.0, |x| bearing[x]),
            2
        );
    }

    #[test]
    fn turn_angles_and_classes() {
        assert_eq!(turn_angle(350.0, 10.0), 20.0);
        assert_eq!(turn_angle(10.0, 350.0), -20.0);
        assert_eq!(turn_angle(90.0, 270.0), 180.0);
        // 방위각 차이는 0~180도이며 반대 방향은 180도이다
        assert_eq!(heading_diff(350.0, 10.0), 20.0);
        assert_eq!(heading_diff(0.0, 180.0), 180.0);
        assert_eq!(heading_diff(90.0, -90.0), 180.0);
        assert_eq!(classify(5.0), Turn::Straight);
        assert_eq!(classify(30.0), Turn::SlightRight);
        assert_eq!(classify(-30.0), Turn::SlightLeft);
        assert_eq!(classify(90.0), Turn::Right);
        assert_eq!(classify(-120.0), Turn::Left);
        assert_eq!(classify(170.0), Turn::UTurn);
        // 북쪽으로 100m 가는 선의 방위각은 0, 동쪽은 90
        let north = [(37.0, 127.0), (37.0 + 100.0 / 111_320.0, 127.0)];
        let east = [
            (37.0, 127.0),
            (37.0, 127.0 + 100.0 / (111_320.0 * 0.809_017)),
        ];
        assert!(end_bearings(&north, 15.0).0.abs() < 1e-6);
        assert!((end_bearings(&east, 15.0).1 - 90.0).abs() < 1e-6);
    }

    #[test]
    fn turn_points_skip_short_links() {
        let link = |name, length, start, end| LinkHeading {
            name,
            rank: 107,
            length,
            start,
            end,
            ramp: false,
            bridged: false,
        };
        // 북으로 가는 도로 1, 교차로 안 12m 링크(도로 2), 동으로 가는 도로 3, 같은 도로 3에서 30° 꺾임, 도로 4로 이름만 바뀜,
        // 도로 5로 30° 꺾이며 이름이 바뀜
        let links = [
            link(1, 200.0, 0.0, 0.0),
            link(2, 12.0, 45.0, 45.0),
            link(3, 150.0, 90.0, 90.0),
            link(3, 150.0, 120.0, 120.0),
            link(4, 150.0, 125.0, 125.0),
            link(5, 150.0, 155.0, 155.0),
        ];
        let points: Vec<(usize, f64, Turn, u16, u16)> = turn_points(&links)
            .into_iter()
            .map(|t| (t.link, t.angle, t.turn, t.from_name, t.to_name))
            .collect();
        // 링크 0 끝에서 우회전 90°, 같은 도로의 30° 꺾임과 이름만 바뀌는 직진은 안내하지 않고, 링크 4 끝에서 우측 방향 30°
        assert_eq!(
            points,
            [
                (0, 90.0, Turn::Right, 1, 3),
                (4, 30.0, Turn::SlightRight, 4, 5)
            ]
        );
        // 마지막 링크는 짧아도 건너뛰지 않는다
        let tail = [link(1, 200.0, 0.0, 0.0), link(1, 10.0, 180.0, 180.0)];
        assert_eq!(turn_points(&tail)[0].turn, Turn::UTurn);
        // 넓은 도로 1 사이에 낀 32m의 옆길 2는 교차로를 가로지르는 링크라 건너뛰어 안내가 없고, 양쪽이 다른 도로 1과 3이면 32m 링크로 안내한다
        let crossing = [
            link(1, 100.0, 0.0, 0.0),
            link(2, 32.0, 313.0, 313.0),
            link(1, 100.0, 7.0, 7.0),
        ];
        assert!(turn_points(&crossing).is_empty());
        let jog = [
            link(1, 100.0, 0.0, 0.0),
            link(2, 32.0, 313.0, 313.0),
            link(3, 100.0, 7.0, 7.0),
        ];
        assert_eq!(turn_points(&jog).len(), 2);
    }

    #[test]
    fn exits_merges_and_bridged_loops() {
        let link = |name, length, start, end, ramp, bridged| LinkHeading {
            name,
            rank: if ramp { 101 } else { name as u8 + 100 },
            length,
            start,
            end,
            ramp,
            bridged,
        };
        // 본선 1에서 램프 2로 8° 갈라져 나가고(출구), 램프가 회차 통행으로 본선 3에 이어지며, 본선 3에서 램프 4로 합류 없이 30° 들어간 뒤 본선 5로 합류
        let links = [
            link(1, 500.0, 180.0, 180.0, false, false),
            link(2, 400.0, 188.0, 240.0, true, false),
            link(3, 600.0, 90.0, 90.0, false, true),
            link(4, 300.0, 120.0, 100.0, true, false),
            link(5, 500.0, 95.0, 95.0, false, false),
        ];
        let turns: Vec<(usize, Turn, u16, u8)> = turn_points(&links)
            .into_iter()
            .map(|t| (t.link, t.turn, t.to_name, t.to_rank))
            .collect();
        // 1→2는 우측 출구이고 뒤 도로명과 도로등급은 램프 뒤 본선 3의 것, 2→3은 램프 회차라 안내하지 않고, 3→4는 우측 출구, 4→5는 합류라 안내하지 않는다
        assert_eq!(
            turns,
            [(0, Turn::ExitRight, 3, 103), (2, Turn::ExitRight, 5, 105)]
        );
        // 램프에서 램프로 30° 꺾이며 이름이 바뀌어도 안내하지 않고, 본선에서 본선으로 30° 꺾이며 이름이 바뀌면 우측 방향이다
        let ramps = [
            link(1, 500.0, 0.0, 0.0, true, false),
            link(2, 500.0, 30.0, 30.0, true, false),
            link(3, 500.0, 30.0, 30.0, false, false),
            link(4, 500.0, 60.0, 60.0, false, false),
        ];
        let turns: Vec<(usize, Turn)> = turn_points(&ramps)
            .into_iter()
            .map(|t| (t.link, t.turn))
            .collect();
        assert_eq!(turns, [(2, Turn::SlightRight)]);
        // 본선에서 회차 통행으로 반대 방향 본선에 이어지면 유턴이다
        let uturn = [
            link(1, 500.0, 0.0, 0.0, false, false),
            link(1, 500.0, 180.0, 180.0, false, true),
        ];
        assert_eq!(turn_points(&uturn)[0].turn, Turn::UTurn);
    }

    #[test]
    fn merge_same_names() {
        // 도로명 1, 1, 2, 1, 1 순서의 링크 5개는 세 구간이 된다
        let runs = merge_runs(&[1, 1, 2, 1, 1], &[10.0, 20.0, 5.0, 1.0, 2.0]);
        assert_eq!(
            runs,
            [(1, 0.0, 30.0, 2), (2, 30.0, 35.0, 1), (1, 35.0, 38.0, 2)]
        );
        assert_eq!(merge_runs(&[], &[]), []);
    }

    #[test]
    fn sub_polyline_and_point_at() {
        // 동쪽으로 100m, 북쪽으로 100m 가는 꺾인 선. 평면 길이는 각각 약 90m, 111m이다
        let pts = [
            (37.0, 127.0),
            (37.0, 127.0 + 100.0 / (111_320.0 * 0.809_017)),
            (
                37.0 + 100.0 / 111_320.0,
                127.0 + 100.0 / (111_320.0 * 0.809_017),
            ),
        ];
        let total = planar_length(&pts);
        assert!((total - 200.0).abs() < 0.01);
        // 앞 25%와 뒤 25%를 잘라내면 모퉁이가 남고 양 끝은 선분 위의 점이다
        let part = sub_polyline(&pts, 0.25, 0.75);
        assert_eq!(part.len(), 3);
        assert_eq!(part[1], pts[1]);
        assert!((planar_length(&part) - 100.0).abs() < 0.01);
        assert!((part[0].0 - 37.0).abs() < 1e-9 && part[0].1 > 127.0);
        assert!((part[2].1 - pts[2].1).abs() < 1e-9 && part[2].0 < pts[2].0);
        // 150m 지점은 두 번째 선분의 가운데
        let p = point_at(&pts, 150.0);
        assert!((p.0 - (37.0 + 50.0 / 111_320.0)).abs() < 1e-9);
        assert_eq!(point_at(&pts, 500.0), pts[2]);
    }
}

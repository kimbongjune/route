//! 안내문과 경고. `Route`의 안내 지점·지점·사고다발 구간을 사람이 읽는 문장으로 만든다.
//! 거리는 출발점부터의 값이며 "N m 앞" 같은 표현은 프론트가 현재 위치로 계산한다

use routedb_core::route::{PoiKind, Route, SignColor, Step, Turn};
use serde::Serialize;

use crate::accident::model::AccidentSection;

/// 안내 지점 한 곳의 안내문
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Instruction {
    /// 출발점부터의 거리(m)
    pub at_m: f64,
    pub lat: f64,
    pub lon: f64,
    /// 안내 문장. 예: 남영역앞교차로에서 좌회전, 한강대로77길
    pub text: String,
    /// 회전. 안내 지점의 `turn`과 같다
    pub turn: Turn,
    /// 표지 색
    pub sign: SignColor,
}

/// 경고 종류
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    /// 과속 단속 카메라
    SpeedCamera,
    /// 신호·통행위반 단속 카메라
    Camera,
    /// 과속방지턱
    SpeedBump,
    /// 사고다발 구간
    AccidentZone,
    /// 요금소
    Toll,
    /// 통과제한 높이
    HeightLimit,
    /// 통과제한 하중
    WeightLimit,
}

/// 경고 한 건. 구간이면 `to_m`이 있고 경로 전체에 해당하면 `at_m`이 없다
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Warning {
    pub kind: WarningKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_m: Option<f64>,
    pub text: String,
}

/// 단속구분 비트. poi.rs의 enforce_bits와 같다
const ENFORCE_SPEED: u8 = 1;

/// 안내 지점마다 안내문을 만든다
pub fn instructions(route: &Route) -> Vec<Instruction> {
    route
        .steps
        .iter()
        .map(|step| Instruction {
            at_m: step.at_m,
            lat: step.lat,
            lon: step.lon,
            text: step_text(step),
            turn: step.turn,
            sign: step.sign,
        })
        .collect()
}

/// 안내 지점 하나의 문장. 교차로 이름이 지번이 아니면 "이름에서"로 시작하고, 회전 뒤에 들어가는 도로명을 붙인다
pub fn step_text(step: &Step) -> String {
    let mut text = String::new();
    if is_intersection_name(&step.node_name) {
        text.push_str(&step.node_name);
        text.push_str("에서 ");
    }
    text.push_str(&turn_text(step));
    if !step.to_name.is_empty() {
        text.push_str(", ");
        text.push_str(&step.to_name);
        if matches!(step.turn, Turn::ExitLeft | Turn::ExitRight) {
            text.push_str(" 방면");
        }
    }
    text
}

/// 회전 문구. 분기가 4개 이상이면 시계 방향으로 말한다
fn turn_text(step: &Step) -> String {
    if let Some(hour) = step.clock {
        return format!("{hour}시 방향");
    }
    match step.turn {
        Turn::Straight => "직진",
        Turn::SlightLeft => "왼쪽 방향",
        Turn::SlightRight => "오른쪽 방향",
        Turn::Left => "좌회전",
        Turn::Right => "우회전",
        Turn::UTurn => "유턴",
        Turn::ExitLeft => "왼쪽 출구",
        Turn::ExitRight => "오른쪽 출구",
    }
    .to_string()
}

/// 교차로 이름이 안내에 쓸 이름인지. 표준노드링크의 NODE_NAME은 교차로 이름이 없으면 "하월곡동 26-14"처럼 지번이므로
/// 숫자나 "숫자-숫자"로 끝나는 이름은 지번으로 보고 쓰지 않는다
pub fn is_intersection_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let tail = name.rsplit(' ').next().unwrap_or(name);
    let tail = tail.trim_start_matches(|c: char| !c.is_ascii_digit() && c != '-');
    !(tail.chars().all(|c| c.is_ascii_digit() || c == '-')
        && tail.chars().any(|c| c.is_ascii_digit()))
}

/// 경로 위 경고를 거리 순서로 만든다. 경로 전체에 해당하는 통과제한은 맨 앞이다
pub fn warnings(route: &Route, accident_sections: &[AccidentSection]) -> Vec<Warning> {
    let mut out = Vec::new();
    if route.min_height_cm > 0 {
        out.push(Warning {
            kind: WarningKind::HeightLimit,
            at_m: None,
            to_m: None,
            text: format!("높이 제한 {:.1}m", route.min_height_cm as f64 / 100.0),
        });
    }
    if route.min_weight_kg > 0 {
        out.push(Warning {
            kind: WarningKind::WeightLimit,
            at_m: None,
            to_m: None,
            text: format!("하중 제한 {:.1}t", route.min_weight_kg as f64 / 1000.0),
        });
    }
    let mut along = Vec::new();
    for poi in &route.pois {
        let (kind, text) = match poi.kind {
            PoiKind::Camera if poi.enforce & ENFORCE_SPEED != 0 && poi.limit > 0 => (
                WarningKind::SpeedCamera,
                format!("과속 단속 카메라, 제한속도 {}", poi.limit),
            ),
            PoiKind::Camera if poi.enforce & ENFORCE_SPEED != 0 => {
                (WarningKind::SpeedCamera, "과속 단속 카메라".to_string())
            }
            PoiKind::Camera => (WarningKind::Camera, "단속 카메라".to_string()),
            PoiKind::Bump => (WarningKind::SpeedBump, "과속방지턱".to_string()),
        };
        along.push(Warning {
            kind,
            at_m: Some(poi.at_m),
            to_m: None,
            text,
        });
    }
    for section in accident_sections {
        let accidents: u32 = section.zones.iter().map(|z| z.accidents).sum();
        along.push(Warning {
            kind: WarningKind::AccidentZone,
            at_m: Some(section.from_m),
            to_m: Some(section.to_m),
            text: format!(
                "{} 사고다발 구간 {:.0}m, 사고 {}건",
                section.kind,
                section.to_m - section.from_m,
                accidents
            ),
        });
    }
    along.sort_by(|a, b| a.at_m.unwrap_or(0.0).total_cmp(&b.at_m.unwrap_or(0.0)));
    out.extend(along);
    if !route.tolls.is_empty() {
        out.push(Warning {
            kind: WarningKind::Toll,
            at_m: None,
            to_m: None,
            text: format!("요금소 {}곳", route.tolls.len()),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(node_name: &str, turn: Turn, to_name: &str, clock: Option<u8>) -> Step {
        Step {
            at_m: 1453.0,
            lat: 37.54,
            lon: 126.97,
            node_name: node_name.to_string(),
            turn,
            angle: -113.0,
            from_name: "청파로".to_string(),
            to_name: to_name.to_string(),
            to_rank: 104,
            sign: SignColor::Blue,
            branches: 3,
            clock,
        }
    }

    #[test]
    fn intersection_names() {
        assert!(is_intersection_name("남영역앞교차로"));
        assert!(is_intersection_name("신갈JC(북측)"));
        assert!(is_intersection_name("반포IC"));
        assert!(!is_intersection_name("하월곡동 26-14"));
        assert!(!is_intersection_name("야당동1040"));
        assert!(!is_intersection_name(""));
    }

    #[test]
    fn step_texts() {
        assert_eq!(
            step_text(&step("남영역앞교차로", Turn::Left, "한강대로77길", None)),
            "남영역앞교차로에서 좌회전, 한강대로77길"
        );
        assert_eq!(
            step_text(&step(
                "하월곡동 26-14",
                Turn::SlightRight,
                "장월로1길",
                Some(1)
            )),
            "1시 방향, 장월로1길"
        );
        assert_eq!(
            step_text(&step("신갈JC(북측)", Turn::ExitRight, "영동고속도로", None)),
            "신갈JC(북측)에서 오른쪽 출구, 영동고속도로 방면"
        );
        assert_eq!(step_text(&step("", Turn::UTurn, "", None)), "유턴");
    }
}

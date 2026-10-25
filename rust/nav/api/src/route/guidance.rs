//! 안내문과 경고. `Route`의 안내 지점·지점·사고다발 구간을 사람이 읽는 문장으로 만든다.
//! 거리는 출발점부터의 값이며 "N m 앞" 같은 표현은 프론트가 현재 위치로 계산한다

use routedb_core::route::{PoiKind, Route, SignColor, Step, Turn};
use serde::Serialize;

use super::toll::{StopKind, TollFee};
use crate::accident::model::AccidentSection;
use crate::caution::construction::ConstructionHit;
use crate::caution::enforce::EnforceSection;
use crate::caution::risk::{RiskHit, element_text, grade_text};
use crate::caution::section::CautionSection;
use crate::caution::zone::{ZoneKind, ZoneSection};
use crate::guideline::GuideLineTable;

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
    /// 고속도로 출구의 노면색깔유도선 색. 없으면 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guide_line: Option<String>,
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
    /// 도로환경 위험요소 구간(급커브, 낙석도로, 철길 건널목 등)
    Hazard,
    /// 상습 결빙구간
    Icing,
    /// 안개 잦은 구간
    Fog,
    /// 구간단속 구간
    SectionCamera,
    /// 도로위험상황예보 지점
    RoadRisk,
    /// 공사 지점
    Construction,
    /// 어린이보호구역
    SchoolZone,
    /// 노인보호구역
    ElderlyZone,
    /// 장애인보호구역
    DisabledZone,
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

/// 단속구분 비트. poi.rs의 enforce_bits와 같다. 1 속도, 2 신호, 4 통행위반, 8 불법주정차, 16 기타
const ENFORCE_SPEED: u8 = 1;
const ENFORCE_SIGNAL: u8 = 2;
const ENFORCE_LANE: u8 = 4;

/// 카메라의 단속 종류 문구. 속도와 신호가 같이 있으면 "신호·과속"이다
pub fn camera_label(enforce: u8) -> &'static str {
    match (
        enforce & ENFORCE_SPEED != 0,
        enforce & ENFORCE_SIGNAL != 0,
        enforce & ENFORCE_LANE != 0,
    ) {
        (true, true, _) => "신호·과속 단속 카메라",
        (true, false, _) => "과속 단속 카메라",
        (false, true, _) => "신호 단속 카메라",
        (false, false, true) => "통행위반 단속 카메라",
        _ => "단속 카메라",
    }
}

/// 안내 지점마다 안내문을 만든다. 고속도로 출구는 유도선 색을 찾아 문장 끝에 붙인다
pub fn instructions(route: &Route, guide_lines: &GuideLineTable) -> Vec<Instruction> {
    route
        .steps
        .iter()
        .map(|step| {
            let guide_line = guide_lines.color_for(&step.node_name, &step.from_name, step.turn);
            let mut text = step_text(step);
            if let Some(color) = &guide_line {
                text.push_str(", ");
                text.push_str(color);
                text.push_str(" 유도선");
            }
            Instruction {
                at_m: step.at_m,
                lat: step.lat,
                lon: step.lon,
                text,
                turn: step.turn,
                sign: step.sign,
                guide_line,
            }
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

/// 경로가 지나는 주의 구간과 지점. 경고를 만들 때 사용한다
pub struct Cautions<'a> {
    pub accident_sections: &'a [AccidentSection],
    pub hazard_sections: &'a [AccidentSection],
    pub icing_sections: &'a [CautionSection],
    pub fog_sections: &'a [CautionSection],
    pub protect_zones: &'a [ZoneSection],
    pub enforce_sections: &'a [EnforceSection],
    pub risks: &'a [RiskHit],
    pub constructions: &'a [ConstructionHit],
}

/// 경로 위 경고를 거리 순서로 만든다. 경로 전체에 해당하는 통과제한은 맨 앞이다.
/// 요금소는 지나는 자리마다 하나이며 요금이 정해지는 출구에는 금액을, 입구에는 "진입"을 넣는다
pub fn warnings(route: &Route, cautions: &Cautions, toll: &TollFee) -> Vec<Warning> {
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
            PoiKind::Camera => {
                let kind = if poi.enforce & ENFORCE_SPEED != 0 {
                    WarningKind::SpeedCamera
                } else {
                    WarningKind::Camera
                };
                let mut text = camera_label(poi.enforce).to_string();
                if poi.limit > 0 {
                    text.push_str(&format!(", 제한속도 {}", poi.limit));
                }
                (kind, text)
            }
            PoiKind::Bump => (WarningKind::SpeedBump, "과속방지턱".to_string()),
        };
        along.push(Warning {
            kind,
            at_m: Some(poi.at_m),
            to_m: None,
            text,
        });
    }
    for section in cautions.accident_sections {
        let accidents: u32 = section.zones.iter().map(|z| z.accidents).sum();
        let mut text = format!(
            "{} 사고다발 구간 {:.0}m",
            section.kind,
            section.to_m - section.from_m
        );
        // TAAS 사고다발 폴리곤은 사고 건수가 없어 0이다
        if accidents > 0 {
            text.push_str(&format!(", 사고 {accidents}건"));
        }
        along.push(Warning {
            kind: WarningKind::AccidentZone,
            at_m: Some(section.from_m),
            to_m: Some(section.to_m),
            text,
        });
    }
    for section in cautions.hazard_sections {
        along.push(Warning {
            kind: WarningKind::Hazard,
            at_m: Some(section.from_m),
            to_m: Some(section.to_m),
            text: format!(
                "{} 위험 구간 {:.0}m",
                section.kind,
                section.to_m - section.from_m
            ),
        });
    }
    for section in cautions.icing_sections {
        along.push(Warning {
            kind: WarningKind::Icing,
            at_m: Some(section.from_m),
            to_m: Some(section.to_m),
            text: format!("상습 결빙구간 {:.0}m", section.to_m - section.from_m),
        });
    }
    for section in cautions.fog_sections {
        along.push(Warning {
            kind: WarningKind::Fog,
            at_m: Some(section.from_m),
            to_m: Some(section.to_m),
            text: format!("안개 잦은 구간 {:.0}m", section.to_m - section.from_m),
        });
    }
    for zone in cautions.protect_zones {
        let (kind, label) = match zone.kind {
            ZoneKind::Child => (WarningKind::SchoolZone, "어린이보호구역"),
            ZoneKind::Elderly => (WarningKind::ElderlyZone, "노인보호구역"),
            ZoneKind::Disabled => (WarningKind::DisabledZone, "장애인보호구역"),
        };
        let mut text = format!("{label} {:.0}m", zone.to_m - zone.from_m);
        if let Some(limit) = zone.limit() {
            text.push_str(&format!(", 제한속도 {limit}"));
        }
        along.push(Warning {
            kind,
            at_m: Some(zone.from_m),
            to_m: Some(zone.to_m),
            text,
        });
    }
    for section in cautions.enforce_sections {
        let mut text = format!("구간단속 {:.1}km", (section.to_m - section.from_m) / 1000.0);
        if section.limit > 0 {
            text.push_str(&format!(", 제한속도 {}", section.limit));
        }
        along.push(Warning {
            kind: WarningKind::SectionCamera,
            at_m: Some(section.from_m),
            to_m: Some(section.to_m),
            text,
        });
    }
    for hit in cautions.risks {
        let element = element_text(&hit.point.element).unwrap_or("위험 예보");
        let text = match grade_text(hit.point.grade) {
            Some(grade) => format!("도로 위험: {element}, {grade}"),
            None => format!("도로 위험: {element}"),
        };
        along.push(Warning {
            kind: WarningKind::RoadRisk,
            at_m: Some(hit.at_m),
            to_m: None,
            text,
        });
    }
    for hit in cautions.constructions {
        let c = &hit.construction;
        let name: String = c.name.chars().take(40).collect();
        let mut text = format!("공사 중: {name}");
        if !c.control.is_empty() {
            let control: String = c.control.chars().take(30).collect();
            text.push_str(&format!(" ({control})"));
        }
        along.push(Warning {
            kind: WarningKind::Construction,
            at_m: Some(hit.at_m),
            to_m: None,
            text,
        });
    }
    for stop in &toll.stops {
        let text = match stop.kind {
            StopKind::Entry => format!("{} 요금소 진입", stop.name),
            StopKind::Exit => format!(
                "{} 요금소, {}부터 통행료 {}원",
                stop.name, stop.entry_name, stop.won
            ),
            StopKind::Open => format!("{} 요금소, 통행료 {}원", stop.name, stop.won),
            StopKind::Unknown => format!("{} 요금소, 요금 미확인", stop.name),
        };
        along.push(Warning {
            kind: WarningKind::Toll,
            at_m: Some(stop.at_m),
            to_m: None,
            text,
        });
    }
    along.sort_by(|a, b| a.at_m.unwrap_or(0.0).total_cmp(&b.at_m.unwrap_or(0.0)));
    out.extend(along);
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
    fn camera_labels() {
        assert_eq!(camera_label(1), "과속 단속 카메라");
        assert_eq!(camera_label(2), "신호 단속 카메라");
        assert_eq!(camera_label(3), "신호·과속 단속 카메라");
        assert_eq!(camera_label(4), "통행위반 단속 카메라");
        assert_eq!(camera_label(16), "단속 카메라");
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

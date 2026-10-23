use serde::Serialize;

/// 사고다발지역 한 곳. 사고다발_폴리곤.csv의 한 행이다
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AccidentZone {
    /// 유형. 자전거, 보행노인, 보행어린이, 스쿨존어린이, 결빙, 연휴기간, 지자체별, 이륜차, 법규위반, 음주운전, 보행자, 화물차, 사고위험지역
    pub kind: String,
    pub id: String,
    /// 지점명 또는 사고위험지역명
    pub name: String,
    pub accidents: u32,
    pub casualties: u32,
    pub deaths: u32,
    /// 폴리곤 중심
    pub lat: f64,
    pub lon: f64,
}

/// 경로가 지나는 같은 유형의 다발지역 구간. 폴리곤이 겹치거나 이어지면 한 구간으로 합친다
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AccidentSection {
    pub kind: String,
    /// 출발점부터 구간이 시작하는 거리(m)
    pub from_m: f64,
    /// 출발점부터 구간이 끝나는 거리(m)
    pub to_m: f64,
    pub zones: Vec<AccidentZone>,
}

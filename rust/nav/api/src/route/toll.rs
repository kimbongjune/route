//! 통행료. 영업소 표와 영업소 간 요금표를 읽어 경로가 지나는 영업소 열에서 요금을 계산한다.
//!
//! 요금표는 한국도로공사 TCS_AJ의 (출발, 도착, 운영기관) 행이며 한 쌍에 운영기관이 여럿이면(원톨링) 모두 더한다.
//! 폐쇄식·갠트리형 영업소는 처음 지나면 입구로 열고, 뒤에 지나는 영업소와의 쌍이 요금표에 있으면 닫는다.
//! 개방식 영업소는 자기 쌍(출발=도착) 요금을 그 자리에서 더한다. 끝까지 닫히지 않은 입구와 표에 없는 영업소는 미확인이다.

use std::collections::HashMap;
use std::path::Path;

use routedb_core::route::TollPass;
use serde::Serialize;

/// 영업방식구분코드. 1 개방식, 2 폐쇄식, 3 갠트리형, 0 미지정
const OPEN_SYSTEM: u8 = 1;

struct Unit {
    name: String,
    mode: u8,
}

struct Fare {
    operator: String,
    /// 1종~6종 금액
    won: [u32; 6],
}

/// 요금표
pub struct TollTable {
    units: HashMap<u32, Unit>,
    fares: HashMap<(u32, u32), Vec<Fare>>,
}

/// 요금이 정해진 구간. 개방식은 입구와 출구가 같다
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TollSegment {
    pub entry: u32,
    pub entry_name: String,
    pub exit: u32,
    pub exit_name: String,
    /// 운영기관명. 여럿이면 "·"로 잇는다
    pub operators: String,
    pub won: u32,
    /// 출구 영업소의 출발점부터의 거리(m)
    pub at_m: f64,
}

/// 요금을 정하지 못한 영업소
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UnknownToll {
    pub unit: u32,
    pub name: String,
    pub at_m: f64,
    /// 표에 없는 영업소, 닫히지 않은 입구, 개방식 요금 없음
    pub reason: &'static str,
}

/// 요금소를 지나는 자리 하나. 한 요금소를 나눠 쓰는 영업소 여럿은 한 자리이다
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopKind {
    /// 폐쇄식 입구
    Entry,
    /// 폐쇄식 출구. 요금이 정해진다
    Exit,
    /// 개방식. 그 자리에서 요금이 정해진다
    Open,
    /// 요금을 정하지 못함
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TollStop {
    pub kind: StopKind,
    pub unit: u32,
    pub name: String,
    pub at_m: f64,
    /// 출구와 개방식의 요금. 출구는 입구 이름도 있다
    pub won: u32,
    pub entry_name: String,
}

/// 경로의 통행료
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TollFee {
    /// 1종 기준 합계(원). 미확인 영업소는 들어 있지 않다
    pub won: u32,
    pub segments: Vec<TollSegment>,
    pub unknown: Vec<UnknownToll>,
    /// 지나는 자리마다 하나. 거리 순서
    pub stops: Vec<TollStop>,
}

fn io_err(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

impl TollTable {
    /// 영업소.csv(영업소코드, 영업소명, 운영기관코드, 영업방식구분코드)와
    /// 요금표.csv(출발코드, 도착코드, 운영기관코드, 운영기관명, 1종~6종)를 읽는다
    pub fn load(units_path: &Path, fares_path: &Path) -> std::io::Result<Self> {
        let mut units = HashMap::new();
        let mut reader = csv::Reader::from_path(units_path)?;
        for record in reader.records() {
            let r = record?;
            let code: u32 = r[0]
                .trim()
                .parse()
                .map_err(|_| io_err(format!("영업소코드 {}", &r[0])))?;
            units.insert(
                code,
                Unit {
                    name: r[1].trim().to_string(),
                    mode: r[3].trim().parse().unwrap_or(0),
                },
            );
        }
        let mut fares: HashMap<(u32, u32), Vec<Fare>> = HashMap::new();
        let mut reader = csv::Reader::from_path(fares_path)?;
        for record in reader.records() {
            let r = record?;
            let from: u32 = r[0]
                .trim()
                .parse()
                .map_err(|_| io_err(format!("출발코드 {}", &r[0])))?;
            let to: u32 = r[1]
                .trim()
                .parse()
                .map_err(|_| io_err(format!("도착코드 {}", &r[1])))?;
            let mut won = [0u32; 6];
            for (k, w) in won.iter_mut().enumerate() {
                *w = r[4 + k].trim().parse().unwrap_or(0);
            }
            fares.entry((from, to)).or_default().push(Fare {
                operator: r[3].trim().to_string(),
                won,
            });
        }
        Ok(Self { units, fares })
    }

    pub fn unit_count(&self) -> usize {
        self.units.len()
    }

    pub fn pair_count(&self) -> usize {
        self.fares.len()
    }

    pub fn unit_name(&self, unit: u32) -> &str {
        self.units.get(&unit).map_or("", |u| u.name.as_str())
    }

    fn segment(&self, entry: u32, exit: u32, class: usize, at_m: f64) -> Option<TollSegment> {
        let fares = self.fares.get(&(entry, exit))?;
        Some(TollSegment {
            entry,
            entry_name: self.unit_name(entry).to_string(),
            exit,
            exit_name: self.unit_name(exit).to_string(),
            operators: fares
                .iter()
                .map(|f| f.operator.as_str())
                .collect::<Vec<_>>()
                .join("·"),
            won: fares.iter().map(|f| f.won[class]).sum(),
            at_m,
        })
    }

    /// 지나는 영업소 열의 통행료. class는 0이 1종이다.
    /// 같은 자리(거리 차 1m 안)의 영업소들은 한 요금소를 영업소코드 여럿이 나눠 쓰는 것이므로 한 묶음으로 보고,
    /// 묶음 안에서 열린 입구를 닫는 영업소가 있으면 그것만 적용하고 없으면 개방식 요금 하나만 더하며 그것도 없으면 입구로 연다
    pub fn fee(&self, passes: &[TollPass], class: usize) -> TollFee {
        let mut fee = TollFee::default();
        let mut open: Vec<TollPass> = Vec::new();
        let mut i = 0;
        while i < passes.len() {
            let mut j = i + 1;
            while j < passes.len() && (passes[j].at_m - passes[i].at_m).abs() < 1.0 {
                j += 1;
            }
            let group = &passes[i..j];
            i = j;
            let known: Vec<&TollPass> = group
                .iter()
                .filter(|p| self.units.contains_key(&p.unit))
                .collect();
            for p in group.iter().filter(|p| !self.units.contains_key(&p.unit)) {
                fee.unknown.push(UnknownToll {
                    unit: p.unit,
                    name: String::new(),
                    at_m: p.at_m,
                    reason: "영업소 표에 없음",
                });
                fee.stops.push(TollStop {
                    kind: StopKind::Unknown,
                    unit: p.unit,
                    name: p.unit.to_string(),
                    at_m: p.at_m,
                    won: 0,
                    entry_name: String::new(),
                });
            }
            if known.is_empty() {
                continue;
            }
            // 열린 입구 중 이 묶음의 영업소와 쌍이 있는 가장 오래된 것을 닫는다
            let closing = open.iter().enumerate().find_map(|(k, o)| {
                known
                    .iter()
                    .find(|p| self.fares.contains_key(&(o.unit, p.unit)))
                    .map(|p| (k, p.unit, p.at_m))
            });
            if let Some((k, exit, at_m)) = closing {
                let entry = open.remove(k);
                let segment = self.segment(entry.unit, exit, class, at_m).unwrap();
                fee.stops.push(TollStop {
                    kind: StopKind::Exit,
                    unit: exit,
                    name: segment.exit_name.clone(),
                    at_m,
                    won: segment.won,
                    entry_name: segment.entry_name.clone(),
                });
                fee.segments.push(segment);
                continue;
            }
            // 개방식 요금은 묶음에서 하나만 더한다
            if let Some(p) = known
                .iter()
                .find(|p| self.units[&p.unit].mode == OPEN_SYSTEM)
            {
                let name = self.unit_name(p.unit).to_string();
                match self.segment(p.unit, p.unit, class, p.at_m) {
                    Some(s) => {
                        fee.stops.push(TollStop {
                            kind: StopKind::Open,
                            unit: p.unit,
                            name: name.clone(),
                            at_m: p.at_m,
                            won: s.won,
                            entry_name: String::new(),
                        });
                        fee.segments.push(s);
                    }
                    None => {
                        fee.unknown.push(UnknownToll {
                            unit: p.unit,
                            name: name.clone(),
                            at_m: p.at_m,
                            reason: "개방식 요금 없음",
                        });
                        fee.stops.push(TollStop {
                            kind: StopKind::Unknown,
                            unit: p.unit,
                            name,
                            at_m: p.at_m,
                            won: 0,
                            entry_name: String::new(),
                        });
                    }
                }
                continue;
            }
            // 폐쇄식·갠트리형은 입구로 연다. 한 자리에 여럿이면 모두 열어 두고 닫힐 때 하나만 닫힌다
            fee.stops.push(TollStop {
                kind: StopKind::Entry,
                unit: known[0].unit,
                name: self.unit_name(known[0].unit).to_string(),
                at_m: known[0].at_m,
                won: 0,
                entry_name: String::new(),
            });
            for p in &known {
                open.push((*p).clone());
            }
        }
        for o in open {
            fee.unknown.push(UnknownToll {
                unit: o.unit,
                name: self.unit_name(o.unit).to_string(),
                at_m: o.at_m,
                reason: "출구 요금소를 지나지 않음",
            });
            // 닫히지 않은 입구의 자리는 미확인으로 바꾼다
            if let Some(stop) = fee
                .stops
                .iter_mut()
                .find(|s| s.kind == StopKind::Entry && s.at_m == o.at_m)
            {
                stop.kind = StopKind::Unknown;
            }
        }
        fee.won = fee.segments.iter().map(|s| s.won).sum();
        fee
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 폐쇄식 서울 101·부산 140·남천안 150, 개방식 인천 11, 민자 갠트리 300·301
    fn demo() -> TollTable {
        let dir = std::env::temp_dir().join(format!("toll_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("units.csv"),
            "영업소코드,영업소명,운영기관코드,영업방식구분코드\n101,서울,00,2\n140,부산,00,2\n150,남천안,00,2\n011,인천,00,1\n300,남풍세,01,3\n301,정안,01,3\n091,대동(개),00,1\n252,대동,00,2\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("fares.csv"),
            "출발코드,도착코드,운영기관코드,운영기관명,1종,2종,3종,4종,5종,6종\n\
             101,140,00,한국도로공사,18600,19000,19700,26100,30700,9300\n\
             101,150,00,한국도로공사,4900,5000,5200,6800,8000,2450\n\
             101,150,01,천안논산고속도로,200,200,200,300,300,100\n\
             011,011,00,한국도로공사,900,900,900,1000,1000,450\n\
             300,301,01,천안논산고속도로,1800,1900,2000,2600,3100,900\n\
             091,091,00,한국도로공사,800,800,800,900,900,400\n\
             101,252,00,한국도로공사,18000,18300,19000,25200,29600,9000\n",
        )
        .unwrap();
        TollTable::load(&dir.join("units.csv"), &dir.join("fares.csv")).unwrap()
    }

    fn pass(unit: u32, at_m: f64) -> TollPass {
        TollPass { unit, at_m }
    }

    #[test]
    fn closed_open_and_one_tolling() {
        let t = demo();
        assert_eq!((t.unit_count(), t.pair_count()), (8, 6));
        // 폐쇄식 입구와 출구
        let fee = t.fee(&[pass(101, 1000.0), pass(140, 400_000.0)], 0);
        assert_eq!(fee.won, 18600);
        assert_eq!(fee.segments[0].exit_name, "부산");
        assert_eq!(fee.segments[0].at_m, 400_000.0);
        assert!(fee.unknown.is_empty());
        // 원톨링: 한 쌍에 운영기관 두 행이면 더하고, 민자 갠트리끼리는 따로 닫힌다. 경차는 6종
        let fee = t.fee(
            &[
                pass(101, 0.0),
                pass(300, 50_000.0),
                pass(301, 70_000.0),
                pass(150, 90_000.0),
            ],
            0,
        );
        assert_eq!(fee.won, 1800 + 5100);
        assert_eq!(fee.segments[1].operators, "한국도로공사·천안논산고속도로");
        assert_eq!(t.fee(&[pass(101, 0.0), pass(150, 1.0)], 5).won, 2550);
        // 개방식은 자기 쌍 요금을 그 자리에서 더한다
        let fee = t.fee(&[pass(11, 500.0)], 0);
        assert_eq!(
            (fee.won, fee.segments[0].entry, fee.segments[0].exit),
            (900, 11, 11)
        );
        // 한 요금소를 나눠 쓰는 대동(개)와 대동이 같은 자리에 있으면 입구를 닫는 대동만 적용한다
        let fee = t.fee(
            &[pass(101, 0.0), pass(91, 378_000.0), pass(252, 378_000.0)],
            0,
        );
        assert_eq!(
            (fee.won, fee.segments.len(), fee.unknown.len()),
            (18000, 1, 0)
        );
        assert_eq!(fee.stops.len(), 2);
        assert_eq!(
            (fee.stops[0].kind, fee.stops[0].name.as_str()),
            (StopKind::Entry, "서울")
        );
        assert_eq!(
            (
                fee.stops[1].kind,
                fee.stops[1].won,
                fee.stops[1].entry_name.as_str()
            ),
            (StopKind::Exit, 18000, "서울")
        );
        // 입구 없이 같은 자리를 지나면 개방식 요금 하나만 더한다
        let fee = t.fee(&[pass(91, 10.0), pass(252, 10.0)], 0);
        assert_eq!(
            (fee.won, fee.segments.len(), fee.unknown.len()),
            (800, 1, 0)
        );
        // 닫히지 않은 입구와 표에 없는 영업소는 미확인이고 합계에 들어가지 않는다
        let fee = t.fee(&[pass(140, 0.0), pass(999, 10.0)], 0);
        assert_eq!(fee.won, 0);
        assert_eq!(fee.unknown.len(), 2);
        assert_eq!(fee.unknown[0].reason, "영업소 표에 없음");
        assert_eq!(fee.unknown[1].reason, "출구 요금소를 지나지 않음");
        assert_eq!(
            fee.stops.iter().map(|s| s.kind).collect::<Vec<_>>(),
            [StopKind::Unknown, StopKind::Unknown]
        );
    }
}

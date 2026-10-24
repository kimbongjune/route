//! 노면색깔유도선. 고속도로 분기점·나들목의 유도선 색을 교차로 이름과 노선으로 찾아 출구 안내문에 붙인다

use std::collections::HashMap;
use std::io;
use std::path::Path;

use routedb_core::route::Turn;
use serde::Deserialize;

/// 유도선_노드.csv 한 행
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct GuideLine {
    /// 표준노드링크 NODE_NAME 형식의 시설물 이름. 신갈JC, 부평IC
    #[serde(rename = "노드이름")]
    pub node_name: String,
    /// 고속도로 노선명. 경부선, 영동선
    #[serde(rename = "노선명")]
    pub route_name: String,
    #[serde(rename = "방향")]
    pub direction: String,
    #[serde(rename = "분기형태")]
    pub shape: String,
    /// 왼쪽 유도선 색. 유도선이 하나면 이 칸에 있다
    #[serde(rename = "좌")]
    pub left: String,
    /// 오른쪽 유도선 색
    #[serde(rename = "우")]
    pub right: String,
    #[serde(rename = "비고")]
    pub note: String,
}

/// 교차로 이름으로 찾는 유도선 표
#[derive(Default)]
pub struct GuideLineTable {
    by_node: HashMap<String, Vec<GuideLine>>,
}

impl GuideLineTable {
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut reader = csv::Reader::from_path(path)?;
        let mut by_node: HashMap<String, Vec<GuideLine>> = HashMap::new();
        for row in reader.deserialize::<GuideLine>() {
            let row = row.map_err(io::Error::other)?;
            by_node
                .entry(normalize_node(&row.node_name))
                .or_default()
                .push(row);
        }
        Ok(Self { by_node })
    }

    pub fn from_rows(rows: Vec<GuideLine>) -> Self {
        let mut table = Self::default();
        for row in rows {
            table
                .by_node
                .entry(normalize_node(&row.node_name))
                .or_default()
                .push(row);
        }
        table
    }

    pub fn len(&self) -> usize {
        self.by_node.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_node.is_empty()
    }

    /// 출구 안내의 유도선 색. 교차로 이름이 같은 행 중 주행 도로의 노선 행을 고르고(없으면 전부), 오른쪽 출구는 오른쪽 색,
    /// 오른쪽 색이 없거나 왼쪽 출구면 왼쪽 색이다. 고른 행들의 색이 서로 다르면 None이다
    pub fn color_for(&self, node_name: &str, road_name: &str, turn: Turn) -> Option<String> {
        if !matches!(turn, Turn::ExitLeft | Turn::ExitRight) {
            return None;
        }
        let rows = self.by_node.get(&normalize_node(node_name))?;
        let on_route: Vec<&GuideLine> = rows
            .iter()
            .filter(|r| route_matches(&r.route_name, road_name))
            .collect();
        let candidates = if on_route.is_empty() {
            rows.iter().collect()
        } else {
            on_route
        };
        let mut color: Option<&str> = None;
        for row in candidates {
            let picked = match turn {
                Turn::ExitRight if !row.right.is_empty() => row.right.as_str(),
                _ => row.left.as_str(),
            };
            if picked.is_empty() {
                continue;
            }
            match color {
                None => color = Some(picked),
                Some(c) if c == picked => {}
                Some(_) => return None,
            }
        }
        color.map(str::to_string)
    }
}

/// 교차로 이름을 비교용으로 바꾼다. 괄호 설명과 공백을 빼고 JCT는 JC로 한다
pub fn normalize_node(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in name.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ' ' => {}
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.replace("JCT", "JC")
}

/// 유도선 표의 노선명 "경부선"이 표준노드링크 도로명 "경부고속도로"와 같은 노선인지. 끝의 "선"을 뺀 이름으로 시작하면 같다
pub fn route_matches(route_name: &str, road_name: &str) -> bool {
    let stem = route_name
        .trim()
        .strip_suffix('선')
        .unwrap_or(route_name.trim());
    !stem.is_empty() && road_name.starts_with(stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(node: &str, route: &str, left: &str, right: &str) -> GuideLine {
        GuideLine {
            node_name: node.to_string(),
            route_name: route.to_string(),
            direction: String::new(),
            shape: String::new(),
            left: left.to_string(),
            right: right.to_string(),
            note: "분기점".to_string(),
        }
    }

    #[test]
    fn normalize_and_route() {
        assert_eq!(normalize_node("신갈JC(북측)"), "신갈JC");
        assert_eq!(normalize_node("여주 JCT"), "여주JC");
        assert!(route_matches("경부선", "경부고속도로"));
        assert!(route_matches("중부내륙선", "중부내륙고속도로"));
        assert!(!route_matches("영동선", "경부고속도로"));
    }

    #[test]
    fn pick_color() {
        let table = GuideLineTable::from_rows(vec![
            row("신갈JC", "영동선", "녹색", "분홍색"),
            row("신갈JC", "경부선", "녹색", "분홍색"),
            row("동대구JC", "경부선", "분홍색", ""),
            row("금호JC", "경부선", "녹색", "분홍색"),
            row("금호JC", "경부선", "녹색", "녹색"),
        ]);
        // 경부고속도로에서 신갈JC 오른쪽 출구는 경부선 행의 오른쪽 색
        assert_eq!(
            table.color_for("신갈JC(북측)", "경부고속도로", Turn::ExitRight),
            Some("분홍색".to_string())
        );
        assert_eq!(
            table.color_for("신갈JC(북측)", "경부고속도로", Turn::ExitLeft),
            Some("녹색".to_string())
        );
        // 오른쪽 색이 없으면 하나뿐인 유도선인 왼쪽 색
        assert_eq!(
            table.color_for("동대구JC", "경부고속도로", Turn::ExitRight),
            Some("분홍색".to_string())
        );
        // 같은 노선의 행끼리 색이 다르면 없음, 출구가 아니면 없음, 모르는 교차로도 없음
        assert_eq!(
            table.color_for("금호JC", "경부고속도로", Turn::ExitRight),
            None
        );
        assert_eq!(table.color_for("신갈JC", "경부고속도로", Turn::Right), None);
        assert_eq!(
            table.color_for("반포IC", "경부고속도로", Turn::ExitRight),
            None
        );
    }
}

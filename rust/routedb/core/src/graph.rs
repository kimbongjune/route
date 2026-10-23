use std::collections::HashMap;
use std::ops::Range;

/// 그래프 생성에 사용하는 교차로 정보
#[derive(Clone, Debug, PartialEq)]
pub struct NodeInput {
    pub id: u64,
    pub lat: f64,
    pub lon: f64,
}

impl NodeInput {
    pub fn new(id: u64, lat: f64, lon: f64) -> Self {
        Self { id, lat, lon }
    }
}

/// 요금 지점. 링크와 그 링크를 지나면 통과하는 영업소코드이다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TollInput {
    pub link_id: u64,
    pub unit: u32,
}

impl TollInput {
    pub fn new(link_id: u64, unit: u32) -> Self {
        Self { link_id, unit }
    }
}

/// 그래프 생성에 사용하는 링크 정보
#[derive(Clone, Debug, PartialEq)]
pub struct LinkInput {
    pub id: u64,
    pub from: u64,
    pub to: u64,
    pub length: f64,
    /// F_NODE에서 T_NODE 방향의 (위도, 경도) 목록
    pub geometry: Vec<(f64, f64)>,
    /// 통행제한차량 REST_VEH. 0 제한 없음, 1 승용차, 2 승합차, 3 버스, 4 트럭, 5 이륜차, 6 기타
    pub rest_veh: u8,
    /// 통과제한하중 REST_W (kg). 0은 제한 없음
    pub rest_w: u16,
    /// 통과제한높이 REST_H (cm). 0은 제한 없음
    pub rest_h: u16,
    /// 최고제한속도 MAX_SPD (km/h)
    pub max_spd: u16,
    /// 도로등급 ROAD_RANK. 101 고속국도부터 108 기타
    pub road_rank: u8,
    /// 도로명 ROAD_NAME. 없으면 빈 문자열
    pub road_name: String,
}

impl LinkInput {
    /// 좌표와 통과제한 없이 LinkInput을 생성한다.
    pub fn new(id: u64, from: u64, to: u64, length: f64) -> Self {
        Self {
            id,
            from,
            to,
            length,
            geometry: Vec::new(),
            rest_veh: 0,
            rest_w: 0,
            rest_h: 0,
            max_spd: 60,
            road_rank: 107,
            road_name: String::new(),
        }
    }
}

/// 그래프에 넣을 경로 위 지점. 카메라와 방지턱이다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoiInput {
    pub link_id: u64,
    /// 1 카메라, 2 방지턱
    pub kind: u8,
    /// 카메라번호 또는 방지턱 지점번호
    pub id: u32,
    /// 링크 시작점부터의 길이 비율 × 10,000
    pub at: u16,
    /// 카메라 제한속도(km/h). 0은 없음
    pub limit: u8,
    /// 카메라 단속구분 비트. 방지턱은 1이 연속형이다
    pub enforce: u8,
}

/// CSR 그래프. 교차로와 간선(방향이 있는 링크)을 내부 번호로 저장한다.
/// 교차로 v에서 출발하는 간선은 first_out[v]번부터 first_out[v + 1]번 전까지이다.
pub struct Graph {
    /// 교차로 번호 -> NODE_ID
    pub node_id: Vec<u64>,
    /// 교차로마다 출발하는 첫 간선 번호
    pub first_out: Vec<u32>,
    /// 간선마다 도착 교차로 번호
    pub head: Vec<u32>,
    /// 간선마다 LINK_ID
    pub link_id: Vec<u64>,
    /// 간선마다 길이(m)
    pub length: Vec<f64>,
    /// 교차로마다 위도
    pub lat: Vec<f64>,
    /// 교차로마다 경도
    pub lon: Vec<f64>,
    /// 간선마다 좌표가 시작하는 번호. 간선 e의 좌표는 geom_first[e]번부터 geom_first[e + 1]번 전까지이다.
    pub geom_first: Vec<u32>,
    /// 링크 좌표 위도
    pub geom_lat: Vec<f64>,
    /// 링크 좌표 경도
    pub geom_lon: Vec<f64>,
    /// 간선마다 영업소코드가 시작하는 번호. 간선 e의 영업소코드는 toll_first[e]번부터 toll_first[e + 1]번 전까지이다.
    pub toll_first: Vec<u32>,
    /// 요금 지점 링크의 영업소코드
    pub toll_unit: Vec<u32>,
    /// 간선마다 통과제한하중(kg). 0은 제한 없음
    pub rest_w: Vec<u16>,
    /// 간선마다 통과제한높이(cm). 0은 제한 없음
    pub rest_h: Vec<u16>,
    /// 간선마다 통행제한차량 코드. 0은 제한 없음
    pub rest_veh: Vec<u8>,
    /// 간선마다 최고제한속도(km/h)
    pub max_spd: Vec<u16>,
    /// 간선마다 도로등급
    pub road_rank: Vec<u8>,
    /// 간선마다 지점이 시작하는 번호. 간선 e의 지점은 poi_first[e]번부터 poi_first[e + 1]번 전까지이며 링크 안 위치 순서이다.
    pub poi_first: Vec<u32>,
    pub poi_id: Vec<u32>,
    pub poi_at: Vec<u16>,
    pub poi_kind: Vec<u8>,
    pub poi_limit: Vec<u8>,
    pub poi_enforce: Vec<u8>,
    /// 간선마다 도로명 번호. road_names[0]은 빈 이름이다.
    pub road_name: Vec<u16>,
    /// 도로명 표. 간선 순서대로 처음 나온 이름에 번호를 붙인다
    pub road_names: Vec<String>,
}

impl Graph {
    /// 교차로 목록과 링크 목록으로 그래프를 만든다.
    /// 교차로 번호는 목록 순서대로 붙고, 교차로를 찾지 못해 건너뛴 링크 수를 같이 반환한다.
    pub fn build(nodes: &[NodeInput], links: &[LinkInput]) -> (Self, usize) {
        // 1. NODE_ID -> 교차로 번호
        let index: HashMap<u64, u32> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i as u32))
            .collect();

        // 2. 링크를 (출발 교차로 번호, 도착 교차로 번호, 링크)로 변환
        let mut edges: Vec<(u32, u32, &LinkInput)> = Vec::with_capacity(links.len());
        let mut skipped = 0;
        for l in links {
            match (index.get(&l.from), index.get(&l.to)) {
                (Some(&fi), Some(&ti)) => edges.push((fi, ti, l)),
                _ => skipped += 1,
            }
        }

        // 3. 출발 교차로 번호 순서로 정렬
        edges.sort_unstable_by_key(|&(f, t, l)| (f, t, l.id));

        // 4. 교차로마다 출발하는 간선 수를 세고 누적합으로 첫 간선 번호를 구한다
        let mut first_out = vec![0u32; nodes.len() + 1];
        for &(f, _, _) in &edges {
            first_out[f as usize + 1] += 1;
        }
        for v in 0..nodes.len() {
            first_out[v + 1] += first_out[v];
        }

        // 5. 정렬된 edges에서 도착 교차로 번호, LINK_ID, 길이, 좌표를 각각 배열로 만든다
        let mut geom_first = Vec::with_capacity(edges.len() + 1);
        let mut geom_lat = Vec::new();
        let mut geom_lon = Vec::new();
        geom_first.push(0);
        for (_, _, l) in &edges {
            for &(lat, lon) in &l.geometry {
                geom_lat.push(lat);
                geom_lon.push(lon);
            }
            geom_first.push(geom_lat.len() as u32);
        }
        // 도로명 표
        let mut road_names = vec![String::new()];
        let mut name_index: HashMap<&str, u16> = HashMap::from([("", 0u16)]);
        let mut road_name = Vec::with_capacity(edges.len());
        for (_, _, l) in &edges {
            let k = match name_index.get(l.road_name.as_str()) {
                Some(&k) => k,
                None => {
                    let k = road_names.len() as u16;
                    road_names.push(l.road_name.clone());
                    name_index.insert(&l.road_name, k);
                    k
                }
            };
            road_name.push(k);
        }
        let graph = Self {
            node_id: nodes.iter().map(|n| n.id).collect(),
            first_out,
            head: edges.iter().map(|e| e.1).collect(),
            link_id: edges.iter().map(|e| e.2.id).collect(),
            length: edges.iter().map(|e| e.2.length).collect(),
            lat: nodes.iter().map(|n| n.lat).collect(),
            lon: nodes.iter().map(|n| n.lon).collect(),
            geom_first,
            geom_lat,
            geom_lon,
            toll_first: vec![0; edges.len() + 1],
            toll_unit: Vec::new(),
            rest_w: edges.iter().map(|e| e.2.rest_w).collect(),
            rest_h: edges.iter().map(|e| e.2.rest_h).collect(),
            rest_veh: edges.iter().map(|e| e.2.rest_veh).collect(),
            max_spd: edges.iter().map(|e| e.2.max_spd).collect(),
            road_rank: edges.iter().map(|e| e.2.road_rank).collect(),
            poi_first: vec![0; edges.len() + 1],
            poi_id: Vec::new(),
            poi_at: Vec::new(),
            poi_kind: Vec::new(),
            poi_limit: Vec::new(),
            poi_enforce: Vec::new(),
            road_name,
            road_names,
        };
        (graph, skipped)
    }

    pub fn node_count(&self) -> usize {
        self.node_id.len()
    }

    pub fn edge_count(&self) -> usize {
        self.head.len()
    }

    pub fn geom_count(&self) -> usize {
        self.geom_lat.len()
    }

    /// 요금 지점 목록으로 간선마다 영업소코드를 저장한다.
    /// 같은 간선·같은 영업소코드는 한 번만 저장하고, 그래프에 없는 LINK_ID의 행 수를 반환한다.
    pub fn set_tolls(&mut self, tolls: &[TollInput]) -> usize {
        let index: HashMap<u64, usize> = self
            .link_id
            .iter()
            .enumerate()
            .map(|(e, &id)| (id, e))
            .collect();
        let mut per_edge: Vec<Vec<u32>> = vec![Vec::new(); self.edge_count()];
        let mut missing = 0;
        for toll in tolls {
            match index.get(&toll.link_id) {
                Some(&e) => {
                    if !per_edge[e].contains(&toll.unit) {
                        per_edge[e].push(toll.unit);
                    }
                }
                None => missing += 1,
            }
        }
        self.toll_first = Vec::with_capacity(self.edge_count() + 1);
        self.toll_unit = Vec::new();
        self.toll_first.push(0);
        for units in &mut per_edge {
            units.sort_unstable();
            self.toll_unit.extend_from_slice(units);
            self.toll_first.push(self.toll_unit.len() as u32);
        }
        missing
    }

    /// 간선 e의 영업소코드
    pub fn tolls_of(&self, e: usize) -> &[u32] {
        &self.toll_unit[self.toll_first[e] as usize..self.toll_first[e + 1] as usize]
    }

    /// 지점 목록으로 간선마다 지점을 링크 안 위치 순서로 저장한다. 그래프에 없는 LINK_ID의 지점 수를 반환한다.
    pub fn set_pois(&mut self, pois: &[PoiInput]) -> usize {
        let index: HashMap<u64, usize> = self
            .link_id
            .iter()
            .enumerate()
            .map(|(e, &id)| (id, e))
            .collect();
        let mut per_edge: Vec<Vec<&PoiInput>> = vec![Vec::new(); self.edge_count()];
        let mut missing = 0;
        for p in pois {
            match index.get(&p.link_id) {
                Some(&e) => per_edge[e].push(p),
                None => missing += 1,
            }
        }
        self.poi_first = vec![0];
        self.poi_id.clear();
        self.poi_at.clear();
        self.poi_kind.clear();
        self.poi_limit.clear();
        self.poi_enforce.clear();
        for list in &mut per_edge {
            list.sort_by_key(|p| (p.at, p.kind, p.id));
            for p in list.iter() {
                self.poi_id.push(p.id);
                self.poi_at.push(p.at);
                self.poi_kind.push(p.kind);
                self.poi_limit.push(p.limit);
                self.poi_enforce.push(p.enforce);
            }
            self.poi_first.push(self.poi_id.len() as u32);
        }
        missing
    }

    /// 간선 e의 지점 번호 범위
    pub fn poi_range(&self, e: usize) -> Range<usize> {
        self.poi_first[e] as usize..self.poi_first[e + 1] as usize
    }

    /// 간선 e의 도로명. 없으면 빈 문자열
    pub fn road_name_of(&self, e: usize) -> &str {
        &self.road_names[self.road_name[e] as usize]
    }

    /// 교차로 v에서 출발하는 간선 번호 범위
    pub fn out_edges(&self, v: u32) -> Range<usize> {
        self.first_out[v as usize] as usize..self.first_out[v as usize + 1] as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_demo_graph() {
        let nodes = [
            NodeInput::new(1000000101, 37.000, 127.000),
            NodeInput::new(1000000201, 37.000, 127.001),
            NodeInput::new(1000000301, 37.001, 127.001),
            NodeInput::new(1000000401, 37.001, 127.000),
            NodeInput::new(1000000501, 37.002, 127.001),
        ];
        let mut links = vec![
            LinkInput::new(1000001000, 1000000101, 1000000201, 89.0),
            LinkInput::new(1000001100, 1000000201, 1000000101, 89.0),
            LinkInput::new(1000002000, 1000000201, 1000000301, 111.0),
            LinkInput::new(1000002100, 1000000301, 1000000201, 111.0),
            LinkInput::new(1000003000, 1000000101, 1000000401, 111.0),
            LinkInput::new(1000004000, 1000000401, 1000000301, 89.0),
            LinkInput::new(1000005000, 1000000301, 1000000501, 111.0),
            LinkInput::new(1000009000, 1000000501, 1000009999, 50.0),
        ];
        // 가 -> 라 링크의 좌표 3개 설정
        links[4].geometry = vec![(37.000, 127.000), (37.0005, 126.9995), (37.001, 127.000)];
        let (g, skipped) = Graph::build(&nodes, &links);
        assert_eq!(skipped, 1);
        assert_eq!(g.first_out, vec![0, 2, 4, 6, 7, 7]);
        assert_eq!(g.head, vec![1, 3, 0, 2, 1, 4, 2]);
        assert_eq!(g.out_edges(1), 2..4);
        assert_eq!(g.out_edges(4), 7..7);
        // 정렬된 간선 순서대로 길이 확인
        assert_eq!(g.length, vec![89.0, 111.0, 89.0, 111.0, 111.0, 111.0, 89.0]);
        assert_eq!((g.lat[2], g.lon[2]), (37.001, 127.001));
        // 간선 1번 가 -> 라의 좌표 확인
        assert_eq!(g.geom_first, vec![0, 0, 3, 3, 3, 3, 3, 3]);
        assert_eq!(g.geom_lat[1], 37.0005);
        assert_eq!(g.geom_lon[1], 126.9995);
    }

    #[test]
    fn set_tolls_by_link_id() {
        let nodes = [
            NodeInput::new(1, 37.0, 127.0),
            NodeInput::new(2, 37.0, 127.001),
            NodeInput::new(3, 37.001, 127.001),
        ];
        let links = vec![
            LinkInput::new(10, 1, 2, 100.0),
            LinkInput::new(20, 2, 3, 100.0),
            LinkInput::new(30, 3, 1, 100.0),
        ];
        let (mut g, _) = Graph::build(&nodes, &links);
        // 링크 10에 영업소 246과 29, 링크 20에 29. 같은 쌍 중복과 그래프에 없는 링크 99
        let missing = g.set_tolls(&[
            TollInput::new(10, 246),
            TollInput::new(10, 29),
            TollInput::new(20, 29),
            TollInput::new(10, 246),
            TollInput::new(99, 1),
        ]);
        assert_eq!(missing, 1);
        let e = |id: u64| g.link_id.iter().position(|&x| x == id).unwrap();
        assert_eq!(g.tolls_of(e(10)), [29, 246]);
        assert_eq!(g.tolls_of(e(20)), [29]);
        assert_eq!(g.tolls_of(e(30)), [] as [u32; 0]);
        assert_eq!(g.toll_first.len(), 4);
    }

    #[test]
    fn road_name_table() {
        let nodes = [
            NodeInput::new(1, 37.0, 127.0),
            NodeInput::new(2, 37.0, 127.001),
            NodeInput::new(3, 37.001, 127.001),
        ];
        let mut links = vec![
            LinkInput::new(10, 1, 2, 100.0),
            LinkInput::new(20, 2, 3, 100.0),
            LinkInput::new(30, 3, 1, 100.0),
        ];
        links[0].road_name = "공원로".to_string();
        links[1].road_name = "공원로".to_string();
        // 링크 30은 이름이 없다
        let (g, _) = Graph::build(&nodes, &links);
        assert_eq!(g.road_names, ["", "공원로"]);
        let e = |id: u64| g.link_id.iter().position(|&x| x == id).unwrap();
        assert_eq!(
            (
                g.road_name_of(e(10)),
                g.road_name_of(e(20)),
                g.road_name_of(e(30))
            ),
            ("공원로", "공원로", "")
        );
        assert_eq!(g.road_name[e(10)], g.road_name[e(20)]);
    }

    #[test]
    fn set_pois_by_link_id() {
        let nodes = [
            NodeInput::new(1, 37.0, 127.0),
            NodeInput::new(2, 37.0, 127.001),
            NodeInput::new(3, 37.001, 127.001),
        ];
        let links = vec![
            LinkInput::new(10, 1, 2, 100.0),
            LinkInput::new(20, 2, 3, 100.0),
        ];
        let (mut g, _) = Graph::build(&nodes, &links);
        // 링크 10에 방지턱 0.8 지점과 카메라 0.3 지점, 링크 20에 카메라 0.5 지점, 그래프에 없는 링크 99
        let pois = [
            PoiInput {
                link_id: 10,
                kind: 2,
                id: 7,
                at: 8000,
                limit: 0,
                enforce: 1,
            },
            PoiInput {
                link_id: 10,
                kind: 1,
                id: 5,
                at: 3000,
                limit: 30,
                enforce: 3,
            },
            PoiInput {
                link_id: 20,
                kind: 1,
                id: 6,
                at: 5000,
                limit: 50,
                enforce: 1,
            },
            PoiInput {
                link_id: 99,
                kind: 1,
                id: 8,
                at: 0,
                limit: 0,
                enforce: 1,
            },
        ];
        assert_eq!(g.set_pois(&pois), 1);
        let e = |id: u64| g.link_id.iter().position(|&x| x == id).unwrap();
        let r = g.poi_range(e(10));
        assert_eq!(
            (
                &g.poi_id[r.clone()],
                &g.poi_at[r.clone()],
                &g.poi_kind[r.clone()],
                &g.poi_limit[r.clone()],
                &g.poi_enforce[r]
            ),
            (
                &[5, 7][..],
                &[3000, 8000][..],
                &[1, 2][..],
                &[30, 0][..],
                &[3, 1][..]
            )
        );
        assert_eq!(&g.poi_id[g.poi_range(e(20))], &[6]);
        assert_eq!(g.poi_first.len(), 3);
    }
}

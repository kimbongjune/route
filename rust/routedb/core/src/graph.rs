use std::collections::HashMap;

/// 그래프 생성에 사용하는 링크 정보
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
}

impl LinkInput {
    /// 좌표와 통과제한 없이 LinkInput을 생성한다.
    pub fn new(id: u64, from: u64, to: u64, length: f64) -> Self {
        Self { id, from, to, length, geometry: Vec::new(), rest_veh: 0, rest_w: 0, rest_h: 0 }
    }
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
}

impl Graph {
    /// (NODE_ID, 위도, 경도) 목록과 링크 목록으로 그래프를 만든다.
    /// 교차로 번호는 목록 순서대로 붙고, 교차로를 찾지 못해 건너뛴 링크 수를 같이 반환한다.
    pub fn build(nodes: &[(u64, f64, f64)], links: &[LinkInput]) -> (Self, usize) {
        // 1. NODE_ID -> 교차로 번호
        let index: HashMap<u64, u32> = nodes.iter().enumerate().map(|(i, &(id, _, _))| (id, i as u32)).collect();

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
        let graph = Self {
            node_id: nodes.iter().map(|n| n.0).collect(),
            first_out,
            head: edges.iter().map(|e| e.1).collect(),
            link_id: edges.iter().map(|e| e.2.id).collect(),
            length: edges.iter().map(|e| e.2.length).collect(),
            lat: nodes.iter().map(|n| n.1).collect(),
            lon: nodes.iter().map(|n| n.2).collect(),
            geom_first,
            geom_lat,
            geom_lon,
            toll_first: vec![0; edges.len() + 1],
            toll_unit: Vec::new(),
            rest_w: edges.iter().map(|e| e.2.rest_w).collect(),
            rest_h: edges.iter().map(|e| e.2.rest_h).collect(),
            rest_veh: edges.iter().map(|e| e.2.rest_veh).collect(),
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

    /// (LINK_ID, 영업소코드) 목록으로 간선마다 영업소코드를 저장한다.
    /// 같은 간선·같은 영업소코드는 한 번만 저장하고, 그래프에 없는 LINK_ID의 행 수를 반환한다.
    pub fn set_tolls(&mut self, tolls: &[(u64, u32)]) -> usize {
        let index: HashMap<u64, usize> = self.link_id.iter().enumerate().map(|(e, &id)| (id, e)).collect();
        let mut per_edge: Vec<Vec<u32>> = vec![Vec::new(); self.edge_count()];
        let mut missing = 0;
        for &(id, unit) in tolls {
            match index.get(&id) {
                Some(&e) => {
                    if !per_edge[e].contains(&unit) {
                        per_edge[e].push(unit);
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

    /// 교차로 v에서 출발하는 간선 번호 범위
    pub fn out_edges(&self, v: u32) -> std::ops::Range<usize> {
        self.first_out[v as usize] as usize..self.first_out[v as usize + 1] as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_demo_graph() {
        let nodes = [
            (1000000101, 37.000, 127.000),
            (1000000201, 37.000, 127.001),
            (1000000301, 37.001, 127.001),
            (1000000401, 37.001, 127.000),
            (1000000501, 37.002, 127.001),
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
        let nodes = [(1, 37.0, 127.0), (2, 37.0, 127.001), (3, 37.001, 127.001)];
        let links = vec![LinkInput::new(10, 1, 2, 100.0), LinkInput::new(20, 2, 3, 100.0), LinkInput::new(30, 3, 1, 100.0)];
        let (mut g, _) = Graph::build(&nodes, &links);
        // 링크 10에 영업소 246과 29, 링크 20에 29. 같은 쌍 중복과 그래프에 없는 링크 99
        let missing = g.set_tolls(&[(10, 246), (10, 29), (20, 29), (10, 246), (99, 1)]);
        assert_eq!(missing, 1);
        let e = |id: u64| g.link_id.iter().position(|&x| x == id).unwrap();
        assert_eq!(g.tolls_of(e(10)), [29, 246]);
        assert_eq!(g.tolls_of(e(20)), [29]);
        assert_eq!(g.tolls_of(e(30)), [] as [u32; 0]);
        assert_eq!(g.toll_first.len(), 4);
    }
}

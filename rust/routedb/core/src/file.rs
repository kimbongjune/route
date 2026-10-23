use std::fs::File;
use std::io::{BufWriter, Error, ErrorKind, Result, Write};
use std::ops::Range;
use std::path::Path;

use memmap2::Mmap;

use crate::graph::Graph;
use crate::linegraph::LineGraph;

/// 그래프 파일 맨 앞 8바이트
pub const MAGIC: &[u8; 8] = b"ROUTEDB\0";
pub const VERSION: u32 = 10;
/// MAGIC 8 + VERSION 4 + 빈칸 4 + 교차로 수 8 + 간선 수 8 + 링크 좌표 수 8 + 통행 수 8 + 경유 링크 수 8 + 영업소코드 수 8 + 지점 수 8
/// + 도로명 수 8 + 도로명 바이트 수 8
pub const HEADER_LEN: usize = 88;

impl Graph {
    /// 그래프와 line graph를 헤더, 8바이트 배열(node_id, link_id, length, lat, lon, geom_lat, geom_lon, turn_weight),
    /// 4바이트 배열(first_out, head, geom_first, turn_first_out, turn_head, via_first, via, toll_first, toll_unit, poi_first, poi_id),
    /// 2바이트 배열(rest_w, rest_h, max_spd, poi_at), 1바이트 배열(rest_veh, road_rank, ramp, poi_kind, poi_limit, poi_enforce, name_bytes) 순서로 파일에 저장한다.
    pub fn write(&self, line: &LineGraph, path: &Path) -> Result<()> {
        let mut w = BufWriter::new(File::create(path)?);

        // 헤더
        w.write_all(MAGIC)?;
        w.write_all(&VERSION.to_le_bytes())?;
        w.write_all(&0u32.to_le_bytes())?;
        w.write_all(&(self.node_count() as u64).to_le_bytes())?;
        w.write_all(&(self.edge_count() as u64).to_le_bytes())?;
        w.write_all(&(self.geom_count() as u64).to_le_bytes())?;
        w.write_all(&(line.edge_count() as u64).to_le_bytes())?;
        w.write_all(&(line.via.len() as u64).to_le_bytes())?;
        w.write_all(&(self.toll_unit.len() as u64).to_le_bytes())?;
        w.write_all(&(self.poi_id.len() as u64).to_le_bytes())?;
        // 도로명 표는 이름을 UTF-8로 이어 붙인 바이트 배열과 이름마다 시작 위치로 저장한다
        let name_bytes: Vec<u8> = self.road_names.concat().into_bytes();
        let mut name_first = Vec::with_capacity(self.road_names.len() + 1);
        name_first.push(0u32);
        for name in &self.road_names {
            name_first.push(name_first.last().unwrap() + name.len() as u32);
        }
        w.write_all(&(self.road_names.len() as u64).to_le_bytes())?;
        w.write_all(&(name_bytes.len() as u64).to_le_bytes())?;

        // 8바이트 배열
        w.write_all(bytemuck::cast_slice(&self.node_id))?;
        w.write_all(bytemuck::cast_slice(&self.link_id))?;
        w.write_all(bytemuck::cast_slice(&self.length))?;
        w.write_all(bytemuck::cast_slice(&self.lat))?;
        w.write_all(bytemuck::cast_slice(&self.lon))?;
        w.write_all(bytemuck::cast_slice(&self.geom_lat))?;
        w.write_all(bytemuck::cast_slice(&self.geom_lon))?;
        w.write_all(bytemuck::cast_slice(&line.weight))?;

        // 4바이트 배열
        w.write_all(bytemuck::cast_slice(&self.first_out))?;
        w.write_all(bytemuck::cast_slice(&self.head))?;
        w.write_all(bytemuck::cast_slice(&self.geom_first))?;
        w.write_all(bytemuck::cast_slice(&line.first_out))?;
        w.write_all(bytemuck::cast_slice(&line.head))?;
        w.write_all(bytemuck::cast_slice(&line.via_first))?;
        w.write_all(bytemuck::cast_slice(&line.via))?;
        w.write_all(bytemuck::cast_slice(&self.toll_first))?;
        w.write_all(bytemuck::cast_slice(&self.toll_unit))?;
        w.write_all(bytemuck::cast_slice(&self.poi_first))?;
        w.write_all(bytemuck::cast_slice(&self.poi_id))?;
        w.write_all(bytemuck::cast_slice(&name_first))?;

        // 2바이트 배열과 1바이트 배열
        w.write_all(bytemuck::cast_slice(&self.rest_w))?;
        w.write_all(bytemuck::cast_slice(&self.rest_h))?;
        w.write_all(bytemuck::cast_slice(&self.max_spd))?;
        w.write_all(bytemuck::cast_slice(&self.poi_at))?;
        w.write_all(bytemuck::cast_slice(&self.road_name))?;
        w.write_all(&self.rest_veh)?;
        w.write_all(&self.road_rank)?;
        w.write_all(&self.ramp)?;
        w.write_all(&self.poi_kind)?;
        w.write_all(&self.poi_limit)?;
        w.write_all(&self.poi_enforce)?;
        w.write_all(&name_bytes)?;

        w.flush()
    }
}

/// mmap으로 연 그래프 파일
pub struct GraphFile {
    mmap: Mmap,
    node_count: usize,
    edge_count: usize,
    geom_count: usize,
    turn_count: usize,
    via_count: usize,
    toll_count: usize,
    poi_count: usize,
    name_count: usize,
    name_byte_count: usize,
    /// 배열별 파일 내 위치
    ranges: [Range<usize>; 32],
}

impl GraphFile {
    /// 그래프 파일을 mmap으로 열고 헤더를 확인한다.
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        if mmap.len() < HEADER_LEN || &mmap[0..8] != MAGIC {
            return Err(Error::new(ErrorKind::InvalidData, "그래프 파일이 아님"));
        }
        let version = u32::from_le_bytes(mmap[8..12].try_into().unwrap());
        if version != VERSION {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("지원하지 않는 버전 {version}"),
            ));
        }
        let count = |at: usize| u64::from_le_bytes(mmap[at..at + 8].try_into().unwrap()) as usize;
        let node_count = count(16);
        let edge_count = count(24);
        let geom_count = count(32);
        let turn_count = count(40);
        let via_count = count(48);
        let toll_count = count(56);
        let poi_count = count(64);
        let name_count = count(72);
        let name_byte_count = count(80);

        // 배열별 파일 내 위치 계산
        let sizes = [
            8 * node_count,       // node_id
            8 * edge_count,       // link_id
            8 * edge_count,       // length
            8 * node_count,       // lat
            8 * node_count,       // lon
            8 * geom_count,       // geom_lat
            8 * geom_count,       // geom_lon
            8 * turn_count,       // turn_weight
            4 * (node_count + 1), // first_out
            4 * edge_count,       // head
            4 * (edge_count + 1), // geom_first
            4 * (edge_count + 1), // turn_first_out
            4 * turn_count,       // turn_head
            4 * (turn_count + 1), // via_first
            4 * via_count,        // via
            4 * (edge_count + 1), // toll_first
            4 * toll_count,       // toll_unit
            4 * (edge_count + 1), // poi_first
            4 * poi_count,        // poi_id
            4 * (name_count + 1), // name_first
            2 * edge_count,       // rest_w
            2 * edge_count,       // rest_h
            2 * edge_count,       // max_spd
            2 * poi_count,        // poi_at
            2 * edge_count,       // road_name
            edge_count,           // rest_veh
            edge_count,           // road_rank
            edge_count,           // ramp
            poi_count,            // poi_kind
            poi_count,            // poi_limit
            poi_count,            // poi_enforce
            name_byte_count,      // name_bytes
        ];
        let mut start = HEADER_LEN;
        let ranges = sizes.map(|size| {
            let range = start..start + size;
            start += size;
            range
        });
        if start != mmap.len() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!(
                    "파일 크기가 {}바이트여야 하는데 {}바이트",
                    start,
                    mmap.len()
                ),
            ));
        }
        Ok(Self {
            mmap,
            node_count,
            edge_count,
            geom_count,
            turn_count,
            via_count,
            toll_count,
            poi_count,
            name_count,
            name_byte_count,
            ranges,
        })
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    pub fn edge_count(&self) -> usize {
        self.edge_count
    }

    pub fn geom_count(&self) -> usize {
        self.geom_count
    }

    pub fn turn_count(&self) -> usize {
        self.turn_count
    }

    pub fn via_count(&self) -> usize {
        self.via_count
    }

    pub fn toll_count(&self) -> usize {
        self.toll_count
    }

    pub fn poi_count(&self) -> usize {
        self.poi_count
    }

    pub fn name_count(&self) -> usize {
        self.name_count
    }

    pub fn name_byte_count(&self) -> usize {
        self.name_byte_count
    }

    pub fn node_id(&self) -> &[u64] {
        bytemuck::cast_slice(&self.mmap[self.ranges[0].clone()])
    }

    pub fn link_id(&self) -> &[u64] {
        bytemuck::cast_slice(&self.mmap[self.ranges[1].clone()])
    }

    pub fn length(&self) -> &[f64] {
        bytemuck::cast_slice(&self.mmap[self.ranges[2].clone()])
    }

    pub fn lat(&self) -> &[f64] {
        bytemuck::cast_slice(&self.mmap[self.ranges[3].clone()])
    }

    pub fn lon(&self) -> &[f64] {
        bytemuck::cast_slice(&self.mmap[self.ranges[4].clone()])
    }

    pub fn geom_lat(&self) -> &[f64] {
        bytemuck::cast_slice(&self.mmap[self.ranges[5].clone()])
    }

    pub fn geom_lon(&self) -> &[f64] {
        bytemuck::cast_slice(&self.mmap[self.ranges[6].clone()])
    }

    /// 통행마다 비용(m)
    pub fn turn_weight(&self) -> &[f64] {
        bytemuck::cast_slice(&self.mmap[self.ranges[7].clone()])
    }

    pub fn first_out(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[8].clone()])
    }

    pub fn head(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[9].clone()])
    }

    pub fn geom_first(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[10].clone()])
    }

    /// 링크마다 이어지는 첫 통행 번호
    pub fn turn_first_out(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[11].clone()])
    }

    /// 통행마다 다음 링크 번호
    pub fn turn_head(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[12].clone()])
    }

    /// 통행마다 경유 내부 링크가 시작하는 번호
    pub fn via_first(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[13].clone()])
    }

    /// 경유 내부 링크 번호
    pub fn via(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[14].clone()])
    }

    /// 통행 t가 경유하는 내부 링크 번호
    pub fn via_of(&self, t: usize) -> &[u32] {
        &self.via()[self.via_first()[t] as usize..self.via_first()[t + 1] as usize]
    }

    /// 간선마다 영업소코드가 시작하는 번호
    pub fn toll_first(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[15].clone()])
    }

    /// 요금 지점 링크의 영업소코드
    pub fn toll_unit(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[16].clone()])
    }

    /// 간선 e의 영업소코드
    pub fn tolls_of(&self, e: usize) -> &[u32] {
        &self.toll_unit()[self.toll_first()[e] as usize..self.toll_first()[e + 1] as usize]
    }

    /// 간선마다 지점이 시작하는 번호
    pub fn poi_first(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[17].clone()])
    }

    /// 지점마다 카메라번호 또는 방지턱 지점번호
    pub fn poi_id(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[18].clone()])
    }

    /// 간선마다 통과제한하중(kg). 0은 제한 없음
    pub fn rest_w(&self) -> &[u16] {
        bytemuck::cast_slice(&self.mmap[self.ranges[20].clone()])
    }

    /// 간선마다 통과제한높이(cm). 0은 제한 없음
    pub fn rest_h(&self) -> &[u16] {
        bytemuck::cast_slice(&self.mmap[self.ranges[21].clone()])
    }

    /// 간선마다 최고제한속도(km/h)
    pub fn max_spd(&self) -> &[u16] {
        bytemuck::cast_slice(&self.mmap[self.ranges[22].clone()])
    }

    /// 지점마다 링크 안 위치 비율 × 10,000
    pub fn poi_at(&self) -> &[u16] {
        bytemuck::cast_slice(&self.mmap[self.ranges[23].clone()])
    }

    /// 간선마다 통행제한차량 코드. 0은 제한 없음
    pub fn rest_veh(&self) -> &[u8] {
        &self.mmap[self.ranges[25].clone()]
    }

    /// 간선마다 도로등급. 101 고속국도부터 108 기타
    pub fn road_rank(&self) -> &[u8] {
        &self.mmap[self.ranges[26].clone()]
    }

    /// 간선마다 연결로 여부. 1이 램프
    pub fn ramp(&self) -> &[u8] {
        &self.mmap[self.ranges[27].clone()]
    }

    /// 지점마다 종류. 1 카메라, 2 방지턱
    pub fn poi_kind(&self) -> &[u8] {
        &self.mmap[self.ranges[28].clone()]
    }

    /// 지점마다 카메라 제한속도(km/h). 0은 없음
    pub fn poi_limit(&self) -> &[u8] {
        &self.mmap[self.ranges[29].clone()]
    }

    /// 지점마다 카메라 단속구분 비트. 방지턱은 1이 연속형이다
    pub fn poi_enforce(&self) -> &[u8] {
        &self.mmap[self.ranges[30].clone()]
    }

    /// 간선 e의 지점 번호 범위
    pub fn poi_range(&self, e: usize) -> Range<usize> {
        self.poi_first()[e] as usize..self.poi_first()[e + 1] as usize
    }

    /// 도로명마다 name_bytes에서 시작하는 위치
    pub fn name_first(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[19].clone()])
    }

    /// 간선마다 도로명 번호. 0은 빈 이름이다
    pub fn road_name(&self) -> &[u16] {
        bytemuck::cast_slice(&self.mmap[self.ranges[24].clone()])
    }

    /// 도로명을 UTF-8로 이어 붙인 바이트
    pub fn name_bytes(&self) -> &[u8] {
        &self.mmap[self.ranges[31].clone()]
    }

    /// k번 도로명
    pub fn name(&self, k: usize) -> &str {
        let (s, t) = (
            self.name_first()[k] as usize,
            self.name_first()[k + 1] as usize,
        );
        std::str::from_utf8(&self.name_bytes()[s..t]).expect("도로명이 UTF-8이 아님")
    }

    /// 간선 e의 도로명. 없으면 빈 문자열
    pub fn road_name_of(&self, e: usize) -> &str {
        self.name(self.road_name()[e] as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{LinkInput, NodeInput, PoiInput, TollInput};

    fn demo() -> Graph {
        let nodes = [
            NodeInput::new(1000000101, 37.000, 127.000),
            NodeInput::new(1000000201, 37.000, 127.001),
            NodeInput::new(1000000301, 37.001, 127.001),
            NodeInput::new(1000000401, 37.001, 127.000),
            NodeInput::new(1000000501, 37.002, 127.001),
        ];
        let mut links = vec![
            LinkInput::new(1000001000, 1000000101, 1000000201, 100.0),
            LinkInput::new(1000001100, 1000000201, 1000000101, 100.0),
            LinkInput::new(1000002000, 1000000201, 1000000301, 100.0),
            LinkInput::new(1000002100, 1000000301, 1000000201, 100.0),
            LinkInput::new(1000003000, 1000000101, 1000000401, 50.0),
            LinkInput::new(1000004000, 1000000401, 1000000301, 80.0),
            LinkInput::new(1000005000, 1000000301, 1000000501, 100.0),
        ];
        links[4].geometry = vec![(37.000, 127.000), (37.0005, 126.9995), (37.001, 127.000)];
        links[0].road_name = "공원로".to_string();
        links[1].road_name = "공원로".to_string();
        links[4].road_name = "능안로".to_string();
        // 가 -> 라 링크는 높이 450cm, 하중 4,500kg 제한에 트럭 금지
        links[4].rest_h = 450;
        links[4].rest_w = 4500;
        links[4].rest_veh = 4;
        links[4].max_spd = 30;
        links[4].road_rank = 104;
        links[4].ramp = true;
        Graph::build(&nodes, &links).0
    }

    /// 간선 7개에 통행 3개. 0→3, 1→6은 경유 없음, 6→5는 간선 4를 경유
    fn demo_line() -> LineGraph {
        LineGraph {
            first_out: vec![0, 1, 2, 2, 2, 2, 2, 3],
            head: vec![3, 6, 5],
            weight: vec![100.0, 80.0, 200.0],
            via_first: vec![0, 0, 0, 1],
            via: vec![4],
        }
    }

    #[test]
    fn write_and_open_same_graph() {
        let mut g = demo();
        g.set_tolls(&[
            TollInput::new(1000002000, 246),
            TollInput::new(1000002000, 29),
            TollInput::new(1000005000, 29),
        ]);
        // 링크 1000002000에 카메라 0.3 지점, 링크 1000005000에 방지턱 0.6 지점
        g.set_pois(&[
            PoiInput {
                link_id: 1000002000,
                kind: 1,
                id: 5,
                at: 3000,
                limit: 30,
                enforce: 3,
            },
            PoiInput {
                link_id: 1000005000,
                kind: 2,
                id: 7,
                at: 6000,
                limit: 0,
                enforce: 1,
            },
        ]);
        let line = demo_line();
        let path = std::env::temp_dir().join(format!("routedb_test_{}.rdb", std::process::id()));
        g.write(&line, &path).unwrap();

        let f = GraphFile::open(&path).unwrap();
        assert_eq!(f.node_count(), g.node_count());
        assert_eq!(f.edge_count(), g.edge_count());
        assert_eq!(f.node_id(), g.node_id.as_slice());
        assert_eq!(f.link_id(), g.link_id.as_slice());
        assert_eq!(f.length(), g.length.as_slice());
        assert_eq!(f.lat(), g.lat.as_slice());
        assert_eq!(f.lon(), g.lon.as_slice());
        assert_eq!(f.first_out(), g.first_out.as_slice());
        assert_eq!(f.head(), g.head.as_slice());
        assert_eq!(f.geom_count(), g.geom_count());
        assert_eq!(f.geom_first(), g.geom_first.as_slice());
        assert_eq!(f.geom_lat(), g.geom_lat.as_slice());
        assert_eq!(f.geom_lon(), g.geom_lon.as_slice());
        assert_eq!(f.turn_count(), 3);
        assert_eq!(f.via_count(), 1);
        assert_eq!(f.turn_first_out(), line.first_out.as_slice());
        assert_eq!(f.turn_head(), line.head.as_slice());
        assert_eq!(f.turn_weight(), line.weight.as_slice());
        assert_eq!(f.via_first(), line.via_first.as_slice());
        assert_eq!(f.via(), line.via.as_slice());
        assert_eq!(f.via_of(2), [4]);
        assert_eq!(f.toll_count(), 3);
        assert_eq!(f.toll_first(), g.toll_first.as_slice());
        assert_eq!(f.toll_unit(), g.toll_unit.as_slice());
        let e = g.link_id.iter().position(|&id| id == 1000002000).unwrap();
        assert_eq!(f.tolls_of(e), [29, 246]);
        assert_eq!(f.rest_w(), g.rest_w.as_slice());
        assert_eq!(f.rest_h(), g.rest_h.as_slice());
        assert_eq!(f.rest_veh(), g.rest_veh.as_slice());
        assert_eq!(f.max_spd(), g.max_spd.as_slice());
        assert_eq!(f.road_rank(), g.road_rank.as_slice());
        assert_eq!(f.ramp(), g.ramp.as_slice());
        assert_eq!(f.ramp().iter().filter(|&&r| r == 1).count(), 1);
        assert_eq!(f.poi_count(), 2);
        assert_eq!(f.poi_first(), g.poi_first.as_slice());
        assert_eq!(
            (
                f.poi_id(),
                f.poi_at(),
                f.poi_kind(),
                f.poi_limit(),
                f.poi_enforce()
            ),
            (
                &[5, 7][..],
                &[3000, 6000][..],
                &[1, 2][..],
                &[30, 0][..],
                &[3, 1][..]
            )
        );
        let e = g.link_id.iter().position(|&id| id == 1000005000).unwrap();
        assert_eq!(f.poi_range(e), 1..2);
        assert_eq!(f.name_count(), 3);
        assert_eq!(f.road_name(), g.road_name.as_slice());
        assert_eq!((f.name(0), f.name(1), f.name(2)), ("", "공원로", "능안로"));
        let e = g.link_id.iter().position(|&id| id == 1000003000).unwrap();
        assert_eq!(f.road_name_of(e), "능안로");
        let e = g.link_id.iter().position(|&id| id == 1000003000).unwrap();
        assert_eq!(
            (
                f.rest_h()[e],
                f.rest_w()[e],
                f.rest_veh()[e],
                f.max_spd()[e],
                f.road_rank()[e]
            ),
            (450, 4500, 4, 30, 104)
        );
        assert_eq!(f.rest_h().iter().filter(|&&h| h > 0).count(), 1);

        drop(f);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn open_other_file() {
        let path =
            std::env::temp_dir().join(format!("routedb_test_other_{}.rdb", std::process::id()));
        std::fs::write(&path, b"not a routedb graph file, just some bytes").unwrap();
        assert!(GraphFile::open(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}

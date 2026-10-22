use std::fs::File;
use std::io::{BufWriter, Error, ErrorKind, Result, Write};
use std::ops::Range;
use std::path::Path;

use memmap2::Mmap;

use crate::graph::Graph;
use crate::linegraph::LineGraph;

/// 그래프 파일 맨 앞 8바이트
pub const MAGIC: &[u8; 8] = b"ROUTEDB\0";
pub const VERSION: u32 = 4;
/// MAGIC 8 + VERSION 4 + 빈칸 4 + 교차로 수 8 + 간선 수 8 + 링크 좌표 수 8 + 통행 수 8 + 경유 링크 수 8 + 영업소코드 수 8
pub const HEADER_LEN: usize = 64;

impl Graph {
    /// 그래프와 line graph를 헤더, 8바이트 배열(node_id, link_id, length, lat, lon, geom_lat, geom_lon, turn_weight),
    /// 4바이트 배열(first_out, head, geom_first, turn_first_out, turn_head, via_first, via, toll_first, toll_unit) 순서로 파일에 저장한다.
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
    /// 배열별 파일 내 위치
    ranges: [Range<usize>; 17],
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
            return Err(Error::new(ErrorKind::InvalidData, format!("지원하지 않는 버전 {version}")));
        }
        let count = |at: usize| u64::from_le_bytes(mmap[at..at + 8].try_into().unwrap()) as usize;
        let node_count = count(16);
        let edge_count = count(24);
        let geom_count = count(32);
        let turn_count = count(40);
        let via_count = count(48);
        let toll_count = count(56);

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
        ];
        let mut start = HEADER_LEN;
        let ranges = sizes.map(|size| {
            let range = start..start + size;
            start += size;
            range
        });
        if start != mmap.len() {
            return Err(Error::new(ErrorKind::InvalidData, format!("파일 크기가 {}바이트여야 하는데 {}바이트", start, mmap.len())));
        }
        Ok(Self { mmap, node_count, edge_count, geom_count, turn_count, via_count, toll_count, ranges })
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::LinkInput;

    fn demo() -> Graph {
        let nodes = [
            (1000000101, 37.000, 127.000),
            (1000000201, 37.000, 127.001),
            (1000000301, 37.001, 127.001),
            (1000000401, 37.001, 127.000),
            (1000000501, 37.002, 127.001),
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
        g.set_tolls(&[(1000002000, 246), (1000002000, 29), (1000005000, 29)]);
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

        drop(f);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn open_other_file() {
        let path = std::env::temp_dir().join(format!("routedb_test_other_{}.rdb", std::process::id()));
        std::fs::write(&path, b"not a routedb graph file, just some bytes").unwrap();
        assert!(GraphFile::open(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}

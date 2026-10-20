use std::fs::File;
use std::io::{BufWriter, Error, ErrorKind, Result, Write};
use std::ops::Range;
use std::path::Path;

use memmap2::Mmap;

use crate::graph::Graph;

/// 그래프 파일 맨 앞 8바이트
pub const MAGIC: &[u8; 8] = b"ROUTEDB\0";
pub const VERSION: u32 = 1;
/// MAGIC 8 + VERSION 4 + 빈칸 4 + 교차로 수 8 + 간선 수 8
pub const HEADER_LEN: usize = 32;

impl Graph {
    /// 그래프를 헤더, 8바이트 배열(node_id, link_id, length, lat, lon), 4바이트 배열(first_out, head) 순서로 파일에 저장한다.
    pub fn write(&self, path: &Path) -> Result<()> {
        let mut w = BufWriter::new(File::create(path)?);

        // 헤더
        w.write_all(MAGIC)?;
        w.write_all(&VERSION.to_le_bytes())?;
        w.write_all(&0u32.to_le_bytes())?;
        w.write_all(&(self.node_count() as u64).to_le_bytes())?;
        w.write_all(&(self.edge_count() as u64).to_le_bytes())?;

        // 8바이트 배열
        w.write_all(bytemuck::cast_slice(&self.node_id))?;
        w.write_all(bytemuck::cast_slice(&self.link_id))?;
        w.write_all(bytemuck::cast_slice(&self.length))?;
        w.write_all(bytemuck::cast_slice(&self.lat))?;
        w.write_all(bytemuck::cast_slice(&self.lon))?;

        // 4바이트 배열
        w.write_all(bytemuck::cast_slice(&self.first_out))?;
        w.write_all(bytemuck::cast_slice(&self.head))?;

        w.flush()
    }
}

/// mmap으로 연 그래프 파일
pub struct GraphFile {
    mmap: Mmap,
    node_count: usize,
    edge_count: usize,
    /// 배열별 파일 내 위치
    ranges: [Range<usize>; 7],
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
        let node_count = u64::from_le_bytes(mmap[16..24].try_into().unwrap()) as usize;
        let edge_count = u64::from_le_bytes(mmap[24..32].try_into().unwrap()) as usize;

        // 배열별 크기로 파일 내 위치를 계산한다
        let sizes = [
            8 * node_count,       // node_id
            8 * edge_count,       // link_id
            8 * edge_count,       // length
            8 * node_count,       // lat
            8 * node_count,       // lon
            4 * (node_count + 1), // first_out
            4 * edge_count,       // head
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
        Ok(Self { mmap, node_count, edge_count, ranges })
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    pub fn edge_count(&self) -> usize {
        self.edge_count
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

    pub fn first_out(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[5].clone()])
    }

    pub fn head(&self) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[6].clone()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> Graph {
        let nodes = [
            (1000000101, 37.000, 127.000),
            (1000000201, 37.000, 127.001),
            (1000000301, 37.001, 127.001),
            (1000000401, 37.001, 127.000),
            (1000000501, 37.002, 127.001),
        ];
        let links = [
            (1000001000, 1000000101, 1000000201, 100.0),
            (1000001100, 1000000201, 1000000101, 100.0),
            (1000002000, 1000000201, 1000000301, 100.0),
            (1000002100, 1000000301, 1000000201, 100.0),
            (1000003000, 1000000101, 1000000401, 50.0),
            (1000004000, 1000000401, 1000000301, 80.0),
            (1000005000, 1000000301, 1000000501, 100.0),
        ];
        Graph::build(&nodes, &links).0
    }

    #[test]
    fn write_and_open_same_graph() {
        let g = demo();
        let path = std::env::temp_dir().join(format!("routedb_test_{}.rdb", std::process::id()));
        g.write(&path).unwrap();

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

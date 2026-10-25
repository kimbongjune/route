//! 골목 파일(alley.rdb). 표준노드링크에 없는 골목을 큰길 그래프(CCH)와 따로 저장하고 mmap으로 연다.
//! 경로 탐색은 출발·도착 근처에서만 골목망을 다익스트라로 탐색하여 큰길 진입점(포털)을 찾고 큰길 구간은 CCH로 구한다

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Error, ErrorKind, Result, Write};
use std::ops::Range;
use std::path::Path;

use memmap2::Mmap;

const MAGIC: &[u8; 8] = b"ALLEY\0\0\0";
const VERSION: u32 = 1;
/// MAGIC 8바이트, VERSION 4바이트, 빈칸 4바이트, 개수 9개
const HEADER_LEN: usize = 16 + 9 * 8;
/// 좌표 정수 배율. 1e-7도 단위이다
const COORD_SCALE: f64 = 1e7;
/// 격자 칸 크기(도)와 원점. 위도 0.002도, 경도 0.0025도는 약 220m이다
const CELL_LAT: f64 = 0.002;
const CELL_LON: f64 = 0.0025;
const GRID_LAT0: f64 = 32.0;
const GRID_LON0: f64 = 123.0;
/// 역방향 간선 표시 비트
const REVERSED: u32 = 1 << 31;
/// 포털 방향. 출발 쪽에서 큰길로 나갈 수 있음
pub const PORTAL_OUT: u8 = 1;
/// 포털 방향. 도착 쪽에서 큰길에서 들어올 수 있음
pub const PORTAL_IN: u8 = 2;

const M_PER_LAT: f64 = 111_320.0;

fn m_per_lon(lat: f64) -> f64 {
    M_PER_LAT * lat.to_radians().cos()
}

/// 위경도의 격자 칸 번호 (위도 칸, 경도 칸)
pub fn cell_of(lat: f64, lon: f64) -> (i32, i32) {
    (
        ((lat - GRID_LAT0) / CELL_LAT).floor() as i32,
        ((lon - GRID_LON0) / CELL_LON).floor() as i32,
    )
}

fn cell_key(iy: i32, ix: i32) -> u32 {
    ((iy as u32) << 16) | (ix as u32 & 0xFFFF)
}

/// 골목 링크 하나. 양방향이다
pub struct AlleyLinkInput {
    pub id: u64,
    pub from: u32,
    pub to: u32,
    pub length: f32,
    pub name: u32,
    /// 좌표 (위도, 경도) 목록
    pub points: Vec<(f64, f64)>,
}

/// 골목 파일을 만드는 입력
#[derive(Default)]
pub struct AlleyBuilder {
    pub node_lat: Vec<f64>,
    pub node_lon: Vec<f64>,
    pub links: Vec<AlleyLinkInput>,
    /// 노드마다 (큰길 간선 번호, 간선 위 위치 비율, PORTAL_OUT·PORTAL_IN 비트)
    pub portals: Vec<Vec<(u32, f32, u8)>>,
    pub names: Vec<String>,
}

fn write_slice<T: bytemuck::Pod>(w: &mut impl Write, v: &[T]) -> Result<()> {
    w.write_all(bytemuck::cast_slice(v))
}

impl AlleyBuilder {
    pub fn write(&self, path: &Path) -> Result<()> {
        let node_count = self.node_lat.len();
        // 양방향 간선 CSR
        let mut out: Vec<Vec<(u32, u32)>> = vec![Vec::new(); node_count];
        for (k, l) in self.links.iter().enumerate() {
            out[l.from as usize].push((l.to, k as u32));
            out[l.to as usize].push((l.from, k as u32 | REVERSED));
        }
        let mut first_out = vec![0u32];
        let (mut head, mut edge_link) = (Vec::new(), Vec::new());
        for list in &out {
            for &(h, k) in list {
                head.push(h);
                edge_link.push(k);
            }
            first_out.push(head.len() as u32);
        }
        // 링크 좌표
        let mut geom_first = vec![0u32];
        let (mut point_lat, mut point_lon) = (Vec::new(), Vec::new());
        for l in &self.links {
            for &(lat, lon) in &l.points {
                point_lat.push((lat * COORD_SCALE).round() as i32);
                point_lon.push((lon * COORD_SCALE).round() as i32);
            }
            geom_first.push(point_lat.len() as u32);
        }
        // 포털
        let mut portal_first = vec![0u32];
        let (mut portal_edge, mut portal_frac, mut portal_dir) =
            (Vec::new(), Vec::new(), Vec::new());
        for v in 0..node_count {
            for &(e, f, d) in self.portals.get(v).map_or(&[][..], Vec::as_slice) {
                portal_edge.push(e);
                portal_frac.push(f);
                portal_dir.push(d);
            }
            portal_first.push(portal_edge.len() as u32);
        }
        // 격자. 링크 선분의 범위가 걸치는 칸마다 링크 번호를 넣는다
        let mut cells: HashMap<u32, Vec<u32>> = HashMap::new();
        for (k, l) in self.links.iter().enumerate() {
            let mut seen: HashSet<u32> = HashSet::new();
            for (i, p) in l.points.iter().enumerate() {
                let q = l.points.get(i + 1).unwrap_or(p);
                let (a, b) = (cell_of(p.0, p.1), cell_of(q.0, q.1));
                for iy in a.0.min(b.0)..=a.0.max(b.0) {
                    for ix in a.1.min(b.1)..=a.1.max(b.1) {
                        seen.insert(cell_key(iy, ix));
                    }
                }
            }
            for key in seen {
                cells.entry(key).or_default().push(k as u32);
            }
        }
        let mut keys: Vec<u32> = cells.keys().copied().collect();
        keys.sort_unstable();
        let mut cell_first = vec![0u32];
        let mut cell_link = Vec::new();
        for key in &keys {
            cell_link.extend_from_slice(&cells[key]);
            cell_first.push(cell_link.len() as u32);
        }
        // 이름
        let mut name_first = vec![0u32];
        let mut name_bytes = Vec::new();
        for n in &self.names {
            name_bytes.extend_from_slice(n.as_bytes());
            name_first.push(name_bytes.len() as u32);
        }

        let mut w = BufWriter::new(File::create(path)?);
        w.write_all(MAGIC)?;
        w.write_all(&VERSION.to_le_bytes())?;
        w.write_all(&[0u8; 4])?;
        for count in [
            node_count,
            head.len(),
            self.links.len(),
            point_lat.len(),
            portal_edge.len(),
            self.names.len(),
            name_bytes.len(),
            keys.len(),
            cell_link.len(),
        ] {
            w.write_all(&(count as u64).to_le_bytes())?;
        }
        let scale = |v: &f64| (v * COORD_SCALE).round() as i32;
        write_slice(
            &mut w,
            &self.links.iter().map(|l| l.id).collect::<Vec<u64>>(),
        )?;
        write_slice(
            &mut w,
            &self.node_lat.iter().map(scale).collect::<Vec<i32>>(),
        )?;
        write_slice(
            &mut w,
            &self.node_lon.iter().map(scale).collect::<Vec<i32>>(),
        )?;
        write_slice(&mut w, &first_out)?;
        write_slice(&mut w, &head)?;
        write_slice(&mut w, &edge_link)?;
        write_slice(
            &mut w,
            &self.links.iter().map(|l| l.from).collect::<Vec<u32>>(),
        )?;
        write_slice(
            &mut w,
            &self.links.iter().map(|l| l.to).collect::<Vec<u32>>(),
        )?;
        write_slice(
            &mut w,
            &self.links.iter().map(|l| l.length).collect::<Vec<f32>>(),
        )?;
        write_slice(&mut w, &geom_first)?;
        write_slice(
            &mut w,
            &self.links.iter().map(|l| l.name).collect::<Vec<u32>>(),
        )?;
        write_slice(&mut w, &point_lat)?;
        write_slice(&mut w, &point_lon)?;
        write_slice(&mut w, &portal_first)?;
        write_slice(&mut w, &portal_edge)?;
        write_slice(&mut w, &portal_frac)?;
        write_slice(&mut w, &keys)?;
        write_slice(&mut w, &cell_first)?;
        write_slice(&mut w, &cell_link)?;
        write_slice(&mut w, &name_first)?;
        w.write_all(&portal_dir)?;
        w.write_all(&name_bytes)?;
        w.flush()
    }
}

/// 골목 위 스냅 결과
#[derive(Clone, Debug, PartialEq)]
pub struct AlleySnap {
    pub link: u32,
    /// 입력 좌표에서 골목까지의 거리(m)
    pub distance_m: f64,
    /// 링크 시작점에서 투영 위치까지의 길이 비율(0~1)
    pub frac: f64,
    pub lat: f64,
    pub lon: f64,
    /// 투영한 선분의 링크 방향 방위각(도). 진북이 0이고 시계 방향이다
    pub bearing_deg: f64,
}

/// 골목 구간 조각. 링크 하나의 from_frac에서 to_frac까지이며 to_frac이 작으면 링크 반대 방향이다
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub link: u32,
    pub from_frac: f64,
    pub to_frac: f64,
}

/// 스냅 지점에서 골목망을 탐색한 결과
pub struct Search {
    dist: HashMap<u32, f64>,
    prev: HashMap<u32, (Option<u32>, Piece)>,
    /// 거리 순으로 닿은 포털 노드
    pub portal_nodes: Vec<u32>,
}

impl Search {
    /// 스냅 지점에서 노드까지의 골목 거리(m)
    pub fn dist(&self, node: u32) -> Option<f64> {
        self.dist.get(&node).copied()
    }

    /// 스냅 지점에서 노드까지의 조각 열
    pub fn pieces_to(&self, node: u32) -> Vec<Piece> {
        let mut pieces = Vec::new();
        let mut cur = Some(node);
        while let Some(v) = cur {
            let Some(&(p, piece)) = self.prev.get(&v) else {
                break;
            };
            pieces.push(piece);
            cur = p;
        }
        pieces.reverse();
        pieces
    }
}

/// 다익스트라 힙 항목. 거리가 작은 것이 먼저 나온다
#[derive(PartialEq)]
struct Item(f64, u32);
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Item {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0)
    }
}

/// mmap으로 연 골목 파일
pub struct AlleyFile {
    mmap: Mmap,
    node_count: usize,
    edge_count: usize,
    link_count: usize,
    portal_count: usize,
    ranges: [Range<usize>; 22],
}

macro_rules! slice {
    ($name:ident, $t:ty, $i:expr) => {
        pub fn $name(&self) -> &[$t] {
            bytemuck::cast_slice(&self.mmap[self.ranges[$i].clone()])
        }
    };
}

impl AlleyFile {
    /// 골목 파일을 mmap으로 열고 헤더와 크기를 확인한다
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        if mmap.len() < HEADER_LEN || &mmap[0..8] != MAGIC {
            return Err(Error::new(ErrorKind::InvalidData, "골목 파일이 아님"));
        }
        let version = u32::from_le_bytes(mmap[8..12].try_into().unwrap());
        if version != VERSION {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("지원하지 않는 골목 파일 버전 {version}"),
            ));
        }
        let count = |i: usize| {
            u64::from_le_bytes(mmap[16 + 8 * i..24 + 8 * i].try_into().unwrap()) as usize
        };
        let [
            node,
            edge,
            link,
            point,
            portal,
            name,
            name_bytes,
            cell,
            cell_entry,
        ] = std::array::from_fn(count);
        let sizes = [
            8 * link,       // link_id
            4 * node,       // node_lat
            4 * node,       // node_lon
            4 * (node + 1), // first_out
            4 * edge,       // head
            4 * edge,       // edge_link
            4 * link,       // link_from
            4 * link,       // link_to
            4 * link,       // link_len
            4 * (link + 1), // geom_first
            4 * link,       // link_name
            4 * point,      // point_lat
            4 * point,      // point_lon
            4 * (node + 1), // portal_first
            4 * portal,     // portal_edge
            4 * portal,     // portal_frac
            4 * cell,       // cell_key
            4 * (cell + 1), // cell_first
            4 * cell_entry, // cell_link
            4 * (name + 1), // name_first
            portal,         // portal_dir
            name_bytes,     // name_bytes
        ];
        let mut start = HEADER_LEN;
        let ranges = sizes.map(|size| {
            let r = start..start + size;
            start += size;
            r
        });
        if start != mmap.len() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!(
                    "골목 파일 크기가 {start}바이트여야 하는데 {}바이트",
                    mmap.len()
                ),
            ));
        }
        Ok(Self {
            mmap,
            node_count: node,
            edge_count: edge,
            link_count: link,
            portal_count: portal,
            ranges,
        })
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// 방향이 있는 간선 수. 링크 수의 두 배이다
    pub fn edge_count(&self) -> usize {
        self.edge_count
    }

    pub fn link_count(&self) -> usize {
        self.link_count
    }

    pub fn portal_count(&self) -> usize {
        self.portal_count
    }

    slice!(link_id, u64, 0);
    slice!(node_lat, i32, 1);
    slice!(node_lon, i32, 2);
    slice!(first_out, u32, 3);
    slice!(head, u32, 4);
    slice!(edge_link, u32, 5);
    slice!(link_from, u32, 6);
    slice!(link_to, u32, 7);
    slice!(link_len, f32, 8);
    slice!(geom_first, u32, 9);
    slice!(link_name, u32, 10);
    slice!(point_lat, i32, 11);
    slice!(point_lon, i32, 12);
    slice!(portal_first, u32, 13);
    slice!(portal_edge, u32, 14);
    slice!(portal_frac, f32, 15);
    slice!(cell_key, u32, 16);
    slice!(cell_first, u32, 17);
    slice!(cell_link, u32, 18);
    slice!(name_first, u32, 19);
    slice!(portal_dir, u8, 20);
    slice!(name_bytes, u8, 21);

    /// 노드 위경도
    pub fn node_point(&self, v: u32) -> (f64, f64) {
        (
            self.node_lat()[v as usize] as f64 / COORD_SCALE,
            self.node_lon()[v as usize] as f64 / COORD_SCALE,
        )
    }

    /// 링크 좌표 (위도, 경도) 목록
    pub fn link_points(&self, k: u32) -> Vec<(f64, f64)> {
        let first = self.geom_first();
        let (s, t) = (first[k as usize] as usize, first[k as usize + 1] as usize);
        let (lat, lon) = (self.point_lat(), self.point_lon());
        (s..t)
            .map(|i| (lat[i] as f64 / COORD_SCALE, lon[i] as f64 / COORD_SCALE))
            .collect()
    }

    /// 링크 이름. 없으면 빈 문자열
    pub fn name_of(&self, k: u32) -> &str {
        let n = self.link_name()[k as usize] as usize;
        let first = self.name_first();
        std::str::from_utf8(&self.name_bytes()[first[n] as usize..first[n + 1] as usize])
            .unwrap_or("")
    }

    /// 노드의 포털 (큰길 간선, 위치 비율, 방향 비트) 목록
    pub fn portals_of(&self, v: u32) -> impl Iterator<Item = (u32, f64, u8)> + '_ {
        let first = self.portal_first();
        (first[v as usize] as usize..first[v as usize + 1] as usize).map(|i| {
            (
                self.portal_edge()[i],
                self.portal_frac()[i] as f64,
                self.portal_dir()[i],
            )
        })
    }

    /// 위경도 사각형에 걸치는 격자 칸의 링크 번호. 같은 링크가 여러 번 나올 수 있다
    pub fn links_in(&self, lat0: f64, lon0: f64, lat1: f64, lon1: f64) -> Vec<u32> {
        let (y0, x0) = cell_of(lat0, lon0);
        let (y1, x1) = cell_of(lat1, lon1);
        let keys = self.cell_key();
        let mut out = Vec::new();
        for iy in y0..=y1 {
            let (lo, hi) = (cell_key(iy, x0), cell_key(iy, x1));
            let s = keys.partition_point(|&k| k < lo);
            let t = keys.partition_point(|&k| k <= hi);
            for c in s..t {
                let (a, b) = (
                    self.cell_first()[c] as usize,
                    self.cell_first()[c + 1] as usize,
                );
                out.extend_from_slice(&self.cell_link()[a..b]);
            }
        }
        out
    }

    /// 위경도 주변 3×3칸의 링크 중 가장 가까운 골목을 찾는다. max_m보다 멀면 None이다
    pub fn snap(&self, lat: f64, lon: f64, max_m: f64) -> Option<AlleySnap> {
        let (cy, cx) = cell_of(lat, lon);
        let keys = self.cell_key();
        let mpl = m_per_lon(lat);
        let mut best: Option<AlleySnap> = None;
        let mut seen: HashSet<u32> = HashSet::new();
        for iy in cy - 1..=cy + 1 {
            for ix in cx - 1..=cx + 1 {
                let Ok(c) = keys.binary_search(&cell_key(iy, ix)) else {
                    continue;
                };
                let (s, t) = (
                    self.cell_first()[c] as usize,
                    self.cell_first()[c + 1] as usize,
                );
                for &k in &self.cell_link()[s..t] {
                    if !seen.insert(k) {
                        continue;
                    }
                    let pts = self.link_points(k);
                    // 입력 좌표를 원점으로 한 m 단위 평면 좌표
                    let xy: Vec<(f64, f64)> = pts
                        .iter()
                        .map(|&(a, b)| ((b - lon) * mpl, (a - lat) * M_PER_LAT))
                        .collect();
                    let total: f64 = xy
                        .windows(2)
                        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
                        .sum();
                    let mut walked = 0.0;
                    for (i, w) in xy.windows(2).enumerate() {
                        let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
                        let seg = dx.hypot(dy);
                        let tt = if seg == 0.0 {
                            0.0
                        } else {
                            (-(w[0].0 * dx + w[0].1 * dy) / (seg * seg)).clamp(0.0, 1.0)
                        };
                        let d = (w[0].0 + tt * dx).hypot(w[0].1 + tt * dy);
                        if d <= max_m && best.as_ref().is_none_or(|b| d < b.distance_m) {
                            let (a, b) = (pts[i], pts[i + 1]);
                            best = Some(AlleySnap {
                                link: k,
                                distance_m: d,
                                frac: if total > 0.0 {
                                    (walked + tt * seg) / total
                                } else {
                                    0.0
                                },
                                lat: a.0 + tt * (b.0 - a.0),
                                lon: a.1 + tt * (b.1 - a.1),
                                bearing_deg: dx.atan2(dy).to_degrees().rem_euclid(360.0),
                            });
                        }
                        walked += seg;
                    }
                }
            }
        }
        best
    }

    /// 스냅 지점에서 골목망을 max_m까지 다익스트라로 탐색한다. dir 방향(PORTAL_OUT 또는 PORTAL_IN) 포털이 있는 노드를
    /// 거리 순으로 max_portals개까지 모은다. forward가 Some(true)이면 링크 방향으로만, Some(false)이면 반대 방향으로만 출발한다
    pub fn search(
        &self,
        snap: &AlleySnap,
        dir: u8,
        max_m: f64,
        max_portals: usize,
        forward: Option<bool>,
    ) -> Search {
        let (first_out, head, edge_link, link_len) = (
            self.first_out(),
            self.head(),
            self.edge_link(),
            self.link_len(),
        );
        let k = snap.link;
        let len = link_len[k as usize] as f64;
        let mut dist: HashMap<u32, f64> = HashMap::new();
        let mut prev: HashMap<u32, (Option<u32>, Piece)> = HashMap::new();
        let mut heap = BinaryHeap::new();
        for (node, cost, to_frac) in [
            (self.link_from()[k as usize], snap.frac * len, 0.0),
            (self.link_to()[k as usize], (1.0 - snap.frac) * len, 1.0),
        ] {
            if forward.is_some_and(|f| f != (to_frac == 1.0)) {
                continue;
            }
            if dist.get(&node).is_none_or(|&d| cost < d) {
                dist.insert(node, cost);
                prev.insert(
                    node,
                    (
                        None,
                        Piece {
                            link: k,
                            from_frac: snap.frac,
                            to_frac,
                        },
                    ),
                );
                heap.push(Item(cost, node));
            }
        }
        let mut portal_nodes = Vec::new();
        let mut done: HashSet<u32> = HashSet::new();
        while let Some(Item(d, u)) = heap.pop() {
            if d > max_m || portal_nodes.len() >= max_portals {
                break;
            }
            if !done.insert(u) {
                continue;
            }
            if self.portals_of(u).any(|(_, _, pd)| pd & dir != 0) {
                portal_nodes.push(u);
            }
            for e in first_out[u as usize] as usize..first_out[u as usize + 1] as usize {
                let (v, raw) = (head[e], edge_link[e]);
                let l = raw & !REVERSED;
                let nd = d + link_len[l as usize] as f64;
                if dist.get(&v).is_none_or(|&old| nd < old) {
                    dist.insert(v, nd);
                    let (from_frac, to_frac) = if raw & REVERSED != 0 {
                        (1.0, 0.0)
                    } else {
                        (0.0, 1.0)
                    };
                    prev.insert(
                        v,
                        (
                            Some(u),
                            Piece {
                                link: l,
                                from_frac,
                                to_frac,
                            },
                        ),
                    );
                    heap.push(Item(nd, v));
                }
            }
        }
        Search {
            dist,
            prev,
            portal_nodes,
        }
    }

    /// 조각의 좌표 열. 지나는 방향 순서이다
    pub fn piece_points(&self, p: &Piece) -> Vec<(f64, f64)> {
        let pts = self.link_points(p.link);
        let mut part = crate::route::sub_polyline(
            &pts,
            p.from_frac.min(p.to_frac),
            p.from_frac.max(p.to_frac),
        );
        if p.to_frac < p.from_frac {
            part.reverse();
        }
        part
    }

    /// 조각의 길이(m)
    pub fn piece_length(&self, p: &Piece) -> f64 {
        self.link_len()[p.link as usize] as f64 * (p.to_frac - p.from_frac).abs()
    }

    /// 조각의 LINK_ID. 링크 방향으로 지나면 LINK_ID, 반대 방향이면 LINK_ID + 1이다
    pub fn piece_link_id(&self, p: &Piece) -> u64 {
        let id = self.link_id()[p.link as usize];
        if p.to_frac < p.from_frac { id + 1 } else { id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 노드 0-1-2가 동서로 이어진 골목 두 링크. 노드 2에 큰길 간선 7의 포털이 있다
    fn sample(path: &Path) -> AlleyFile {
        let link = |id, from, to, name, points| AlleyLinkInput {
            id,
            from,
            to,
            length: 88.3,
            name,
            points,
        };
        AlleyBuilder {
            node_lat: vec![37.5, 37.5, 37.5],
            node_lon: vec![127.0, 127.001, 127.002],
            links: vec![
                link(
                    30_000_000_000,
                    0,
                    1,
                    0,
                    vec![(37.5, 127.0), (37.5, 127.001)],
                ),
                link(
                    30_000_000_002,
                    1,
                    2,
                    1,
                    vec![(37.5, 127.001), (37.5, 127.002)],
                ),
            ],
            portals: vec![vec![], vec![], vec![(7, 0.25, PORTAL_OUT | PORTAL_IN)]],
            names: vec![String::new(), "가길".into()],
        }
        .write(path)
        .unwrap();
        AlleyFile::open(path).unwrap()
    }

    #[test]
    fn write_open_snap_search() {
        let dir = std::env::temp_dir().join(format!("alley-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = sample(&dir.join("a.rdb"));
        assert_eq!((f.node_count(), f.edge_count(), f.link_count()), (3, 4, 2));
        assert_eq!(f.name_of(1), "가길");
        // 첫 링크 가운데에서 북쪽 10m
        let s = f.snap(37.5 + 10.0 / M_PER_LAT, 127.0005, 50.0).unwrap();
        assert_eq!(s.link, 0);
        assert!((s.distance_m - 10.0).abs() < 0.5, "{}", s.distance_m);
        assert!((s.frac - 0.5).abs() < 0.01);
        assert!(f.snap(37.6, 127.0, 50.0).is_none());
        assert!((s.bearing_deg - 90.0).abs() < 1.0, "{}", s.bearing_deg);
        // 반대 방향(서쪽)으로만 출발하면 노드 0에서 되돌아 포털에 닿는다
        let west = f.search(&s, PORTAL_OUT, 1000.0, 3, Some(false));
        assert!((west.dist(2).unwrap() - 88.3 * 2.5).abs() < 0.5);
        let search = f.search(&s, PORTAL_OUT, 1000.0, 3, Some(true));
        assert_eq!(search.portal_nodes, vec![2]);
        assert!((search.dist(2).unwrap() - 88.3 * 1.5).abs() < 0.5);
        let pieces = search.pieces_to(2);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].to_frac, 1.0);
        assert_eq!(f.piece_link_id(&pieces[1]), 30_000_000_002);
        let back = Piece {
            link: 1,
            from_frac: 1.0,
            to_frac: 0.0,
        };
        assert_eq!(f.piece_link_id(&back), 30_000_000_003);
        assert_eq!(f.piece_points(&back)[0], (37.5, 127.002));
        assert_eq!(f.portals_of(2).collect::<Vec<_>>(), vec![(7, 0.25, 3)]);
        std::fs::remove_dir_all(&dir).ok();
    }
}

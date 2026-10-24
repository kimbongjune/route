//! 색인 파일. 임포터가 `Builder`로 만들어 쓰고 서버가 `GeocodeIndex`로 mmap하여 읽는다

use std::fs::File;
use std::io::{BufWriter, Error, ErrorKind, Result, Write};
use std::ops::Range;
use std::path::Path;

use memmap2::Mmap;

use crate::text;

pub const MAGIC: &[u8; 8] = b"GEOCODE\0";
pub const VERSION: u32 = 1;
/// 헤더의 u64 개수 필드 수
const COUNTS: usize = 16;
/// MAGIC 8 + VERSION 4 + 빈칸 4 + 개수 16개 × 8
pub const HEADER_LEN: usize = 16 + COUNTS * 8;
/// 리가 없는 주소의 li 값
pub const NO_LI: u32 = u32::MAX;

/// 장소 하나. 국가관심지점정보 한 행
pub struct Poi {
    pub name: String,
    /// 도로명주소. 없으면 지번주소, 둘 다 없으면 빈 문자열
    pub address: String,
    pub lat: f64,
    pub lon: f64,
    /// 관심지점 분류코드 12자리
    pub class: u64,
    pub aliases: Vec<String>,
}

/// 행정구역 하나. 시도 2자리, 시군구 5자리, 읍면동 8자리, 리 10자리 코드
pub struct Region {
    pub code: u64,
    pub name: String,
}

/// 도로명 하나. 시군구코드와 도로명코드 7자리로 구분한다
pub struct Road {
    pub sig: u32,
    pub rn: u32,
    pub name: String,
}

/// 도로명주소 하나. 같은 주소의 건물은 하나로 묶는다
pub struct Address {
    /// `Builder::roads`의 번호
    pub road: u32,
    /// 1이면 지하
    pub under: u8,
    pub mnnm: u32,
    pub slno: u32,
    pub lat: f64,
    pub lon: f64,
    /// `Builder::emd`의 번호
    pub emd: u32,
    /// `Builder::li`의 번호. 없으면 `NO_LI`
    pub li: u32,
    /// 1이면 산 지번
    pub san: u8,
    pub lnbr_mnnm: u32,
    pub lnbr_slno: u32,
    /// 건축물대장 건물명. 없으면 빈 문자열
    pub name: String,
}

/// 색인을 만드는 입력. `write`가 정렬과 2-gram 색인 생성을 한다
#[derive(Default)]
pub struct Builder {
    pub pois: Vec<Poi>,
    pub sido: Vec<Region>,
    pub sig: Vec<Region>,
    pub emd: Vec<Region>,
    pub li: Vec<Region>,
    /// (시군구, 도로명코드) 순으로 정렬되어 있어야 한다
    pub roads: Vec<Road>,
    pub addresses: Vec<Address>,
}

/// `write`가 돌려주는 개수
#[derive(Debug, Default, PartialEq)]
pub struct Stats {
    pub pois: usize,
    pub aliases: usize,
    pub keys: usize,
    pub postings: usize,
    pub roads: usize,
    pub addresses: usize,
}

fn micro(deg: f64) -> i32 {
    (deg * 1e6).round() as i32
}

/// 문자열 목록을 이어 붙인 바이트와 시작 위치 배열로 만든다
fn pack<'a>(items: impl Iterator<Item = &'a str>) -> (Vec<u32>, Vec<u8>) {
    let mut first = vec![0u32];
    let mut bytes = Vec::new();
    for s in items {
        bytes.extend_from_slice(s.as_bytes());
        first.push(bytes.len() as u32);
    }
    (first, bytes)
}

impl Builder {
    /// 장소의 이름과 별칭에서 2-gram 키와 장소 번호 목록을 만든다
    fn bigram_index(&self) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
        let mut entries: Vec<u64> = Vec::new();
        let mut buf = Vec::new();
        for (i, poi) in self.pois.iter().enumerate() {
            for name in std::iter::once(&poi.name).chain(&poi.aliases) {
                text::normalize_into(name, &mut buf);
                for k in text::keys(&buf) {
                    entries.push(((k as u64) << 32) | i as u64);
                }
            }
        }
        entries.sort_unstable();
        entries.dedup();
        let mut keys = Vec::new();
        let mut key_first = vec![0u32];
        let mut postings = Vec::with_capacity(entries.len());
        for e in entries {
            let (k, doc) = ((e >> 32) as u32, e as u32);
            if keys.last() != Some(&k) {
                keys.push(k);
                key_first.push(postings.len() as u32);
            }
            postings.push(doc);
            *key_first.last_mut().unwrap() = postings.len() as u32;
        }
        (keys, key_first, postings)
    }

    /// 색인 파일을 쓴다. 주소는 (도로명, 지하, 본번, 부번) 순으로 정렬한다
    pub fn write(&mut self, path: &Path) -> Result<Stats> {
        for w in self.roads.windows(2) {
            if (w[0].sig, w[0].rn) > (w[1].sig, w[1].rn) {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    "도로명이 정렬되어 있지 않음",
                ));
            }
        }
        self.addresses
            .sort_by_key(|a| (a.road, a.under, a.mnnm, a.slno));
        let (keys, key_first, postings) = self.bigram_index();

        let mut road_by_name: Vec<u32> = (0..self.roads.len() as u32).collect();
        let road_norm: Vec<Vec<char>> = self
            .roads
            .iter()
            .map(|r| text::normalize(&r.name))
            .collect();
        road_by_name.sort_by(|&a, &b| road_norm[a as usize].cmp(&road_norm[b as usize]));

        let mut jibun: Vec<u32> = (0..self.addresses.len() as u32).collect();
        jibun.sort_by_key(|&i| {
            let a = &self.addresses[i as usize];
            (a.emd, a.san, a.lnbr_mnnm, a.lnbr_slno, a.li)
        });

        let (poi_name_first, poi_name_bytes) = pack(self.pois.iter().map(|p| p.name.as_str()));
        let (poi_addr_first, poi_addr_bytes) = pack(self.pois.iter().map(|p| p.address.as_str()));
        let mut poi_alias_first = vec![0u32];
        for p in &self.pois {
            poi_alias_first.push(poi_alias_first.last().unwrap() + p.aliases.len() as u32);
        }
        let (alias_first, alias_bytes) = pack(
            self.pois
                .iter()
                .flat_map(|p| p.aliases.iter().map(String::as_str)),
        );
        let regions = || {
            self.sido
                .iter()
                .chain(&self.sig)
                .chain(&self.emd)
                .chain(&self.li)
        };
        let region_code: Vec<u64> = regions().map(|r| r.code).collect();
        let (region_name_first, region_name_bytes) = pack(regions().map(|r| r.name.as_str()));
        let (road_name_first, road_name_bytes) = pack(self.roads.iter().map(|r| r.name.as_str()));
        let (addr_name_first, addr_name_bytes) =
            pack(self.addresses.iter().map(|a| a.name.as_str()));

        let mut w = BufWriter::new(File::create(path)?);
        w.write_all(MAGIC)?;
        w.write_all(&VERSION.to_le_bytes())?;
        w.write_all(&0u32.to_le_bytes())?;
        let counts = [
            self.pois.len(),
            poi_name_bytes.len(),
            poi_addr_bytes.len(),
            alias_first.len() - 1,
            alias_bytes.len(),
            keys.len(),
            postings.len(),
            self.sido.len(),
            self.sig.len(),
            self.emd.len(),
            self.li.len(),
            region_name_bytes.len(),
            self.roads.len(),
            road_name_bytes.len(),
            self.addresses.len(),
            addr_name_bytes.len(),
        ];
        for c in counts {
            w.write_all(&(c as u64).to_le_bytes())?;
        }

        // 8바이트 배열
        let poi_class: Vec<u64> = self.pois.iter().map(|p| p.class).collect();
        w.write_all(bytemuck::cast_slice(&poi_class))?;
        w.write_all(bytemuck::cast_slice(&region_code))?;

        // 4바이트 배열
        let col = |f: &dyn Fn(&Poi) -> i32| -> Vec<i32> { self.pois.iter().map(f).collect() };
        w.write_all(bytemuck::cast_slice(&col(&|p| micro(p.lat))))?;
        w.write_all(bytemuck::cast_slice(&col(&|p| micro(p.lon))))?;
        w.write_all(bytemuck::cast_slice(&poi_name_first))?;
        w.write_all(bytemuck::cast_slice(&poi_addr_first))?;
        w.write_all(bytemuck::cast_slice(&poi_alias_first))?;
        w.write_all(bytemuck::cast_slice(&alias_first))?;
        w.write_all(bytemuck::cast_slice(&keys))?;
        w.write_all(bytemuck::cast_slice(&key_first))?;
        w.write_all(bytemuck::cast_slice(&postings))?;
        w.write_all(bytemuck::cast_slice(&region_name_first))?;
        let rcol = |f: &dyn Fn(&Road) -> u32| -> Vec<u32> { self.roads.iter().map(f).collect() };
        w.write_all(bytemuck::cast_slice(&rcol(&|r| r.sig)))?;
        w.write_all(bytemuck::cast_slice(&rcol(&|r| r.rn)))?;
        w.write_all(bytemuck::cast_slice(&road_name_first))?;
        w.write_all(bytemuck::cast_slice(&road_by_name))?;
        let acol =
            |f: &dyn Fn(&Address) -> u32| -> Vec<u32> { self.addresses.iter().map(f).collect() };
        w.write_all(bytemuck::cast_slice(&acol(&|a| a.road)))?;
        w.write_all(bytemuck::cast_slice(&acol(&|a| micro(a.lat) as u32)))?;
        w.write_all(bytemuck::cast_slice(&acol(&|a| micro(a.lon) as u32)))?;
        w.write_all(bytemuck::cast_slice(&acol(&|a| a.emd)))?;
        w.write_all(bytemuck::cast_slice(&acol(&|a| a.li)))?;
        w.write_all(bytemuck::cast_slice(&acol(&|a| a.mnnm)))?;
        w.write_all(bytemuck::cast_slice(&acol(&|a| a.slno)))?;
        w.write_all(bytemuck::cast_slice(&acol(&|a| a.lnbr_mnnm)))?;
        w.write_all(bytemuck::cast_slice(&acol(&|a| a.lnbr_slno)))?;
        w.write_all(bytemuck::cast_slice(&addr_name_first))?;
        w.write_all(bytemuck::cast_slice(&jibun))?;

        // 1바이트 배열
        let bcol =
            |f: &dyn Fn(&Address) -> u8| -> Vec<u8> { self.addresses.iter().map(f).collect() };
        w.write_all(&bcol(&|a| a.under))?;
        w.write_all(&bcol(&|a| a.san))?;
        w.write_all(&poi_name_bytes)?;
        w.write_all(&poi_addr_bytes)?;
        w.write_all(&alias_bytes)?;
        w.write_all(&region_name_bytes)?;
        w.write_all(&road_name_bytes)?;
        w.write_all(&addr_name_bytes)?;
        w.flush()?;
        Ok(Stats {
            pois: self.pois.len(),
            aliases: alias_first.len() - 1,
            keys: keys.len(),
            postings: postings.len(),
            roads: self.roads.len(),
            addresses: self.addresses.len(),
        })
    }
}

/// mmap으로 연 색인 파일
pub struct GeocodeIndex {
    mmap: Mmap,
    counts: [usize; COUNTS],
    ranges: [Range<usize>; 35],
}

fn slice_str<'a>(first: &[u32], bytes: &'a [u8], k: usize) -> &'a str {
    std::str::from_utf8(&bytes[first[k] as usize..first[k + 1] as usize])
        .expect("색인 문자열이 UTF-8이 아님")
}

impl GeocodeIndex {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        if mmap.len() < HEADER_LEN || &mmap[0..8] != MAGIC {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "지오코더 색인 파일이 아님",
            ));
        }
        let version = u32::from_le_bytes(mmap[8..12].try_into().unwrap());
        if version != VERSION {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("지원하지 않는 버전 {version}"),
            ));
        }
        let mut counts = [0usize; COUNTS];
        for (i, c) in counts.iter_mut().enumerate() {
            let at = 16 + i * 8;
            *c = u64::from_le_bytes(mmap[at..at + 8].try_into().unwrap()) as usize;
        }
        let [
            p,
            p_name_b,
            p_addr_b,
            al,
            al_b,
            k,
            po,
            s,
            sg,
            e,
            l,
            reg_b,
            r,
            road_b,
            a,
            addr_b,
        ] = counts;
        let regions = s + sg + e + l;
        let sizes = [
            8 * p,             // poi_class
            8 * regions,       // region_code
            4 * p,             // poi_lat
            4 * p,             // poi_lon
            4 * (p + 1),       // poi_name_first
            4 * (p + 1),       // poi_addr_first
            4 * (p + 1),       // poi_alias_first
            4 * (al + 1),      // alias_first
            4 * k,             // key
            4 * (k + 1),       // key_first
            4 * po,            // posting
            4 * (regions + 1), // region_name_first
            4 * r,             // road_sig
            4 * r,             // road_rn
            4 * (r + 1),       // road_name_first
            4 * r,             // road_by_name
            4 * a,             // addr_road
            4 * a,             // addr_lat
            4 * a,             // addr_lon
            4 * a,             // addr_emd
            4 * a,             // addr_li
            4 * a,             // addr_mnnm
            4 * a,             // addr_slno
            4 * a,             // addr_lnbr_mnnm
            4 * a,             // addr_lnbr_slno
            4 * (a + 1),       // addr_name_first
            4 * a,             // jibun_sorted
            a,                 // addr_under
            a,                 // addr_san
            p_name_b,          // poi_name_bytes
            p_addr_b,          // poi_addr_bytes
            al_b,              // alias_bytes
            reg_b,             // region_name_bytes
            road_b,            // road_name_bytes
            addr_b,            // addr_name_bytes
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
            counts,
            ranges,
        })
    }

    fn u32s(&self, i: usize) -> &[u32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[i].clone()])
    }

    fn i32s(&self, i: usize) -> &[i32] {
        bytemuck::cast_slice(&self.mmap[self.ranges[i].clone()])
    }

    fn bytes(&self, i: usize) -> &[u8] {
        &self.mmap[self.ranges[i].clone()]
    }

    pub fn poi_count(&self) -> usize {
        self.counts[0]
    }

    pub fn alias_count(&self) -> usize {
        self.counts[3]
    }

    pub fn key_count(&self) -> usize {
        self.counts[5]
    }

    pub fn posting_count(&self) -> usize {
        self.counts[6]
    }

    pub fn sido_count(&self) -> usize {
        self.counts[7]
    }

    pub fn sig_count(&self) -> usize {
        self.counts[8]
    }

    pub fn emd_count(&self) -> usize {
        self.counts[9]
    }

    pub fn li_count(&self) -> usize {
        self.counts[10]
    }

    pub fn road_count(&self) -> usize {
        self.counts[12]
    }

    pub fn address_count(&self) -> usize {
        self.counts[14]
    }

    pub fn poi_class(&self) -> &[u64] {
        bytemuck::cast_slice(&self.mmap[self.ranges[0].clone()])
    }

    /// 장소 i의 (위도, 경도)
    pub fn poi_point(&self, i: usize) -> (f64, f64) {
        (self.i32s(2)[i] as f64 / 1e6, self.i32s(3)[i] as f64 / 1e6)
    }

    pub fn poi_name(&self, i: usize) -> &str {
        slice_str(self.u32s(4), self.bytes(29), i)
    }

    pub fn poi_address(&self, i: usize) -> &str {
        slice_str(self.u32s(5), self.bytes(30), i)
    }

    /// 장소 i의 별칭
    pub fn poi_aliases(&self, i: usize) -> impl Iterator<Item = &str> {
        let first = self.u32s(6);
        (first[i] as usize..first[i + 1] as usize)
            .map(|k| slice_str(self.u32s(7), self.bytes(31), k))
    }

    /// 2-gram 키의 장소 번호 목록. 없으면 None
    pub fn postings(&self, key: u32) -> Option<&[u32]> {
        let keys = self.u32s(8);
        let k = keys.binary_search(&key).ok()?;
        let first = self.u32s(9);
        Some(&self.u32s(10)[first[k] as usize..first[k + 1] as usize])
    }

    /// 행정구역 코드. 시도, 시군구, 읍면동, 리 순으로 이어져 있다
    pub fn region_code(&self) -> &[u64] {
        bytemuck::cast_slice(&self.mmap[self.ranges[1].clone()])
    }

    pub fn region_name(&self, k: usize) -> &str {
        slice_str(self.u32s(11), self.bytes(32), k)
    }

    /// 시도, 시군구, 읍면동, 리 각각의 번호 범위
    pub fn sido_range(&self) -> Range<usize> {
        0..self.sido_count()
    }

    pub fn sig_range(&self) -> Range<usize> {
        let s = self.sido_count();
        s..s + self.sig_count()
    }

    pub fn emd_range(&self) -> Range<usize> {
        let s = self.sido_count() + self.sig_count();
        s..s + self.emd_count()
    }

    pub fn li_range(&self) -> Range<usize> {
        let s = self.sido_count() + self.sig_count() + self.emd_count();
        s..s + self.li_count()
    }

    pub fn road_sig(&self) -> &[u32] {
        self.u32s(12)
    }

    pub fn road_rn(&self) -> &[u32] {
        self.u32s(13)
    }

    pub fn road_name(&self, r: usize) -> &str {
        slice_str(self.u32s(14), self.bytes(33), r)
    }

    /// 정규화한 이름 순으로 정렬한 도로명 번호
    pub fn road_by_name(&self) -> &[u32] {
        self.u32s(15)
    }

    pub fn addr_road(&self) -> &[u32] {
        self.u32s(16)
    }

    pub fn addr_point(&self, a: usize) -> (f64, f64) {
        (self.i32s(17)[a] as f64 / 1e6, self.i32s(18)[a] as f64 / 1e6)
    }

    /// 주소 a의 읍면동 번호(`emd_range` 안의 상대 번호)
    pub fn addr_emd(&self) -> &[u32] {
        self.u32s(19)
    }

    pub fn addr_li(&self) -> &[u32] {
        self.u32s(20)
    }

    pub fn addr_mnnm(&self) -> &[u32] {
        self.u32s(21)
    }

    pub fn addr_slno(&self) -> &[u32] {
        self.u32s(22)
    }

    pub fn addr_lnbr_mnnm(&self) -> &[u32] {
        self.u32s(23)
    }

    pub fn addr_lnbr_slno(&self) -> &[u32] {
        self.u32s(24)
    }

    pub fn addr_name(&self, a: usize) -> &str {
        slice_str(self.u32s(25), self.bytes(34), a)
    }

    /// (읍면동, 산, 지번 본번, 지번 부번, 리) 순으로 정렬한 주소 번호
    pub fn jibun_sorted(&self) -> &[u32] {
        self.u32s(26)
    }

    pub fn addr_under(&self) -> &[u8] {
        self.bytes(27)
    }

    pub fn addr_san(&self) -> &[u8] {
        self.bytes(28)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 서울 중구 세종대로 110 시청과 용산구 청파로 378 서울역, 장소 셋
    pub(crate) fn demo() -> Builder {
        let mut b = Builder::default();
        b.sido.push(Region {
            code: 11,
            name: "서울특별시".into(),
        });
        b.sig.push(Region {
            code: 11140,
            name: "중구".into(),
        });
        b.sig.push(Region {
            code: 11170,
            name: "용산구".into(),
        });
        b.emd.push(Region {
            code: 11140103,
            name: "태평로1가".into(),
        });
        b.emd.push(Region {
            code: 11170104,
            name: "동자동".into(),
        });
        b.roads.push(Road {
            sig: 11140,
            rn: 2001,
            name: "세종대로".into(),
        });
        b.roads.push(Road {
            sig: 11170,
            rn: 3002,
            name: "청파로".into(),
        });
        b.addresses.push(Address {
            road: 1,
            under: 0,
            mnnm: 378,
            slno: 0,
            lat: 37.5519,
            lon: 126.9708,
            emd: 1,
            li: NO_LI,
            san: 0,
            lnbr_mnnm: 43,
            lnbr_slno: 205,
            name: "서울역".into(),
        });
        b.addresses.push(Address {
            road: 0,
            under: 0,
            mnnm: 110,
            slno: 0,
            lat: 37.5666,
            lon: 126.9784,
            emd: 0,
            li: NO_LI,
            san: 0,
            lnbr_mnnm: 31,
            lnbr_slno: 0,
            name: "서울특별시청".into(),
        });
        b.addresses.push(Address {
            road: 0,
            under: 0,
            mnnm: 110,
            slno: 5,
            lat: 37.5660,
            lon: 126.9780,
            emd: 0,
            li: NO_LI,
            san: 0,
            lnbr_mnnm: 31,
            lnbr_slno: 1,
            name: String::new(),
        });
        b.pois.push(Poi {
            name: "서울역".into(),
            address: "서울특별시 용산구 한강대로 405".into(),
            lat: 37.5547,
            lon: 126.9707,
            class: 403003010900,
            aliases: vec!["서울역KTX".into()],
        });
        b.pois.push(Poi {
            name: "서울특별시청".into(),
            address: "서울특별시 중구 세종대로 110".into(),
            lat: 37.5666,
            lon: 126.9784,
            class: 103002000000,
            aliases: vec!["시청".into()],
        });
        b.pois.push(Poi {
            name: "CJ올리브영 서울역점".into(),
            address: String::new(),
            lat: 37.5540,
            lon: 126.9700,
            class: 203000000000,
            aliases: Vec::new(),
        });
        b
    }

    pub(crate) fn demo_file(name: &str) -> (GeocodeIndex, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("geocode_{name}_{}.idx", std::process::id()));
        demo().write(&path).unwrap();
        (GeocodeIndex::open(&path).unwrap(), path)
    }

    #[test]
    fn write_and_open() {
        let path = std::env::temp_dir().join(format!("geocode_test_{}.idx", std::process::id()));
        let stats = demo().write(&path).unwrap();
        assert_eq!(stats.pois, 3);
        assert_eq!(stats.aliases, 2);
        assert_eq!(stats.addresses, 3);
        let f = GeocodeIndex::open(&path).unwrap();
        assert_eq!(f.poi_count(), 3);
        assert_eq!(f.poi_name(2), "CJ올리브영 서울역점");
        assert_eq!(f.poi_aliases(0).collect::<Vec<_>>(), ["서울역KTX"]);
        assert_eq!(f.poi_point(1), (37.5666, 126.9784));
        assert_eq!(f.poi_class()[0], 403003010900);
        // "서울" 키는 세 장소 모두에 있고 "시청"은 시청과 별칭 둘이다
        assert_eq!(f.postings(text::key('서', '울')), Some(&[0, 1, 2][..]));
        assert_eq!(f.postings(text::key('시', '청')), Some(&[1][..]));
        assert_eq!(f.postings(text::key('없', '음')), None);
        // 주소는 (도로명, 지하, 본번, 부번) 순이라 세종대로 110이 먼저다
        assert_eq!(f.addr_mnnm(), [110, 110, 378]);
        assert_eq!(f.addr_slno(), [0, 5, 0]);
        assert_eq!(f.addr_name(2), "서울역");
        assert_eq!(f.region_name(f.sig_range().start + 1), "용산구");
        assert_eq!(f.road_name(f.road_by_name()[0] as usize), "세종대로");
        assert_eq!(f.jibun_sorted(), [0, 1, 2]);
        drop(f);
        std::fs::remove_file(&path).unwrap();
    }
}

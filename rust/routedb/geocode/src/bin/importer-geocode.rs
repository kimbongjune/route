//! 지오코더 색인 임포터. 도로명주소 전자지도(건물, 출입구, 도로구간, 행정경계)와
//! 국가관심지점정보(TN_POI, TN_ADRES, TN_ALIAS)를 읽어 색인 파일 geocode.idx를 만든다.
//!
//! cargo run --release -p routedb-geocode --bin importer-geocode
//! 데이터 폴더는 환경 변수 ROUTE_MAPS_DIR, 없으면 저장소의 maps이다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use calamine::{Data, Reader as _, open_workbook_auto};
use proj4rs::proj::Proj;
use routedb_geocode::index::NO_LI;
use routedb_geocode::{Address, Builder, Poi, Region, Road};
use shapefile::Shape;
use shapefile::dbase::encoding::EncodingRs;
use shapefile::dbase::encoding_rs::EUC_KR;
use shapefile::dbase::{FieldValue, Record};

const DEFAULT_MAPS_DIR: &str = r"C:\Users\nocdu\Desktop\dev\route\maps";
const EMAP_FOLDER: &str = "도로명주소 전자지도";
const POI_FOLDER: &str = "poi/DownloadMap(20261024)";
const EPSG_5179: &str = "+proj=tmerc +lat_0=38 +lon_0=127.5 +k=0.9996 +x_0=1000000 +y_0=2000000 +ellps=GRS80 +units=m +no_defs";
const EPSG_4326: &str = "+proj=longlat +datum=WGS84 +no_defs";

fn maps_dir() -> PathBuf {
    std::env::var_os("ROUTE_MAPS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MAPS_DIR))
}

/// UTM-K(EPSG:5179)를 위경도로 변경
struct UtmkToWgs84 {
    from: Proj,
    to: Proj,
}

impl UtmkToWgs84 {
    fn new() -> Self {
        Self {
            from: Proj::from_proj_string(EPSG_5179).expect("EPSG:5179 정의가 잘못됨"),
            to: Proj::from_proj_string(EPSG_4326).expect("EPSG:4326 정의가 잘못됨"),
        }
    }

    fn convert(&self, x: f64, y: f64) -> (f64, f64) {
        let mut p = (x, y, 0.0);
        proj4rs::transform::transform(&self.from, &self.to, &mut p).expect("좌표 변환 실패");
        (p.1.to_degrees(), p.0.to_degrees())
    }
}

fn text(record: &Record, name: &str) -> String {
    match record.get(name) {
        Some(FieldValue::Character(Some(s))) => s.trim_matches(['\0', ' ']).to_string(),
        Some(FieldValue::Numeric(Some(n))) => format!("{}", *n as i64),
        _ => String::new(),
    }
}

fn number(record: &Record, name: &str) -> u32 {
    match record.get(name) {
        Some(FieldValue::Numeric(Some(n))) => *n as u32,
        Some(FieldValue::Character(Some(s))) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// CP949 dbf를 연다. 전자지도에는 cpg 파일이 없어 인코딩을 지정한다
fn dbf(path: &Path) -> shapefile::dbase::Reader<std::io::BufReader<std::fs::File>> {
    shapefile::dbase::ReaderBuilder::new()
        .with_encoding(EncodingRs::from(EUC_KR))
        .open(path)
        .unwrap_or_else(|e| panic!("{}를 열 수 없음: {e}", path.display()))
}

/// shp와 CP949 dbf를 함께 연다
fn shp(
    path: &Path,
) -> shapefile::Reader<std::io::BufReader<std::fs::File>, std::io::BufReader<std::fs::File>> {
    let shape = shapefile::ShapeReader::from_path(path)
        .unwrap_or_else(|e| panic!("{}를 열 수 없음: {e}", path.display()));
    shapefile::Reader::new(shape, dbf(&path.with_extension("dbf")))
}

/// 시도 폴더 안의 숫자 이름 폴더(시도코드)
fn sido_folders(emap: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for sido in std::fs::read_dir(emap).expect("전자지도 폴더가 없음") {
        let sido = sido.unwrap().path();
        if !sido.is_dir() {
            continue;
        }
        for sub in std::fs::read_dir(&sido).unwrap() {
            let sub = sub.unwrap().path();
            let name = sub.file_name().unwrap().to_string_lossy();
            if sub.is_dir() && name.chars().all(|c| c.is_ascii_digit()) {
                out.push(sub);
            }
        }
    }
    out.sort();
    out
}

/// 폴리곤 바깥 고리의 무게중심. 넓이가 0이면 꼭짓점 평균
fn centroid(polygon: &shapefile::Polygon) -> (f64, f64) {
    let Some(ring) = polygon.rings().first() else {
        return (0.0, 0.0);
    };
    let pts = ring.points();
    let (mut area, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for w in pts.windows(2) {
        let cross = w[0].x * w[1].y - w[1].x * w[0].y;
        area += cross;
        cx += (w[0].x + w[1].x) * cross;
        cy += (w[0].y + w[1].y) * cross;
    }
    if area.abs() < 1e-9 {
        let n = pts.len() as f64;
        return (
            pts.iter().map(|p| p.x).sum::<f64>() / n,
            pts.iter().map(|p| p.y).sum::<f64>() / n,
        );
    }
    (cx / (3.0 * area), cy / (3.0 * area))
}

/// 같은 도로명주소의 건물 묶음
#[derive(Default)]
struct Group {
    /// 주건물의 주출입구, 주건물 중심, 종속건물 출입구, 종속건물 중심 순으로 좋은 점
    point: Option<(u8, (f64, f64))>,
    name: String,
    emd: u32,
    li: u32,
    san: u8,
    lnbr_mnnm: u32,
    lnbr_slno: u32,
}

fn read_regions(folders: &[PathBuf], b: &mut Builder) {
    for folder in folders {
        for (file, code, name, out) in [
            ("TL_SCCO_CTPRVN", "CTPRVN_CD", "CTP_KOR_NM", 0),
            ("TL_SCCO_SIG", "SIG_CD", "SIG_KOR_NM", 1),
            ("TL_SCCO_EMD", "EMD_CD", "EMD_KOR_NM", 2),
            ("TL_SCCO_LI", "LI_CD", "LI_KOR_NM", 3),
        ] {
            let mut reader = dbf(&folder.join(file).with_extension("dbf"));
            for record in reader.iter_records() {
                let record = record.expect("행정경계 레코드");
                let region = Region {
                    code: text(&record, code)
                        .parse()
                        .expect("행정구역 코드가 숫자가 아님"),
                    name: text(&record, name),
                };
                match out {
                    0 => b.sido.push(region),
                    1 => b.sig.push(region),
                    2 => b.emd.push(region),
                    _ => b.li.push(region),
                }
            }
        }
    }
    for list in [&mut b.sido, &mut b.sig, &mut b.emd, &mut b.li] {
        list.sort_by_key(|r| r.code);
        list.dedup_by_key(|r| r.code);
    }
}

fn read_roads(folders: &[PathBuf], b: &mut Builder) -> HashMap<(u32, u32), u32> {
    for folder in folders {
        let mut reader = dbf(&folder.join("TL_SPRD_MANAGE.dbf"));
        for record in reader.iter_records() {
            let record = record.expect("도로구간 레코드");
            b.roads.push(Road {
                sig: number(&record, "SIG_CD"),
                rn: number(&record, "RN_CD"),
                name: text(&record, "RN"),
            });
        }
    }
    b.roads.sort_by_key(|r| (r.sig, r.rn));
    b.roads.dedup_by_key(|r| (r.sig, r.rn));
    b.roads
        .iter()
        .enumerate()
        .map(|(i, r)| ((r.sig, r.rn), i as u32))
        .collect()
}

fn read_buildings(
    folders: &[PathBuf],
    b: &mut Builder,
    road_index: &HashMap<(u32, u32), u32>,
    tm: &UtmkToWgs84,
) -> (usize, usize) {
    let emd_index: HashMap<u64, u32> = b
        .emd
        .iter()
        .enumerate()
        .map(|(i, r)| (r.code, i as u32))
        .collect();
    let li_index: HashMap<u64, u32> =
        b.li.iter()
            .enumerate()
            .map(|(i, r)| (r.code, i as u32))
            .collect();
    let (mut buildings, mut no_road) = (0usize, 0usize);
    for folder in folders {
        // 주출입구. 건물마다 첫 출입구
        let mut entrances: HashMap<(u32, u32), (f64, f64)> = HashMap::new();
        let mut reader = shp(&folder.join("TL_SPBD_ENTRC.shp"));
        for item in reader.iter_shapes_and_records() {
            let (shape, record) = item.expect("출입구 레코드");
            let Shape::Point(p) = shape else { continue };
            entrances
                .entry((number(&record, "SIG_CD"), number(&record, "BUL_MAN_NO")))
                .or_insert((p.x, p.y));
        }
        let mut groups: HashMap<(u32, u8, u32, u32), Group> = HashMap::new();
        let mut reader = shp(&folder.join("TL_SPBD_BULD.shp"));
        for item in reader.iter_shapes_and_records() {
            let (shape, record) = item.expect("건물 레코드");
            buildings += 1;
            let sig = number(&record, "SIG_CD");
            let rds_sig = match number(&record, "RDS_SIG_CD") {
                0 => sig,
                s => s,
            };
            let Some(&road) = road_index.get(&(rds_sig, number(&record, "RN_CD"))) else {
                no_road += 1;
                continue;
            };
            let under = (text(&record, "BULD_SE_CD") == "1") as u8;
            let key = (
                road,
                under,
                number(&record, "BULD_MNNM"),
                number(&record, "BULD_SLNO"),
            );
            let main = text(&record, "BUL_DPN_SE") != "S";
            let entrance = entrances
                .get(&(sig, number(&record, "BUL_MAN_NO")))
                .copied();
            let center = match &shape {
                Shape::Polygon(p) => centroid(p),
                _ => continue,
            };
            let (rank, point) = match (main, entrance) {
                (true, Some(e)) => (0, e),
                (true, None) => (1, center),
                (false, Some(e)) => (2, e),
                (false, None) => (3, center),
            };
            let g = groups.entry(key).or_default();
            if g.point.is_none_or(|(r, _)| rank < r) {
                g.point = Some((rank, point));
                let sig_code = sig as u64;
                let emd_code =
                    sig_code * 1000 + text(&record, "EMD_CD").parse::<u64>().unwrap_or(0);
                g.emd = *emd_index.get(&emd_code).unwrap_or(&0);
                let li_code = emd_code * 100 + text(&record, "LI_CD").parse::<u64>().unwrap_or(0);
                g.li = match text(&record, "LI_CD").as_str() {
                    "" | "00" => NO_LI,
                    _ => *li_index.get(&li_code).unwrap_or(&NO_LI),
                };
                g.san = (text(&record, "MNTN_YN") == "1") as u8;
                g.lnbr_mnnm = number(&record, "LNBR_MNNM");
                g.lnbr_slno = number(&record, "LNBR_SLNO");
            }
            if g.name.is_empty() {
                g.name = text(&record, "BULD_NM");
            }
        }
        for ((road, under, mnnm, slno), g) in groups {
            let (x, y) = g.point.expect("점이 없는 묶음").1;
            let (lat, lon) = tm.convert(x, y);
            b.addresses.push(Address {
                road,
                under,
                mnnm,
                slno,
                lat,
                lon,
                emd: g.emd,
                li: g.li,
                san: g.san,
                lnbr_mnnm: g.lnbr_mnnm,
                lnbr_slno: g.lnbr_slno,
                name: g.name,
            });
        }
        println!(
            "  {} 건물 누적 {buildings} 주소 {}",
            folder.display(),
            b.addresses.len()
        );
    }
    (buildings, no_road)
}

/// TN_ALIAS xlsx. 열은 frstrgsde, lastupdde, updtcd, poiid, poi_nm, aliasnm1..20
fn read_aliases(path: &Path) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    let mut workbook =
        open_workbook_auto(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let range = workbook
        .worksheet_range_at(0)
        .expect("별칭 시트가 없음")
        .expect("별칭 시트를 읽을 수 없음");
    for row in range.rows().skip(1) {
        let Some(Data::String(id)) = row.get(3) else {
            continue;
        };
        let aliases: Vec<String> = row[5..]
            .iter()
            .filter_map(|c| match c {
                Data::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
                _ => None,
            })
            .collect();
        if !aliases.is_empty() {
            out.insert(id.clone(), aliases);
        }
    }
    out
}

fn read_pois(poi_dir: &Path, b: &mut Builder, tm: &UtmkToWgs84) {
    let mut folders: Vec<PathBuf> = std::fs::read_dir(poi_dir)
        .expect("POI 폴더가 없음")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir() && p.file_name().unwrap().to_string_lossy().ends_with("_POI"))
        .collect();
    folders.sort();
    for folder in folders {
        let t = Instant::now();
        let first = |sub: &str, ext: &str| -> PathBuf {
            std::fs::read_dir(folder.join(sub))
                .unwrap_or_else(|_| panic!("{}에 {sub}가 없음", folder.display()))
                .map(|e| e.unwrap().path())
                .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)))
                .unwrap_or_else(|| panic!("{}에 {sub} {ext} 파일이 없음", folder.display()))
        };
        let mut addresses: HashMap<String, String> = HashMap::new();
        let mut reader = dbf(&first("TN_ADRES", "dbf"));
        for record in reader.iter_records() {
            let record = record.expect("주소 레코드");
            let road = text(&record, "doro_nm");
            let addr = if road.is_empty() {
                text(&record, "jibun_nm")
            } else {
                road
            };
            addresses.entry(text(&record, "nf_id")).or_insert(addr);
        }
        let mut aliases = read_aliases(&first("TN_ALIAS", "xlsx"));
        let before = b.pois.len();
        let mut reader = shp(&first("TN_POI", "shp"));
        for item in reader.iter_shapes_and_records() {
            let (shape, record) = item.expect("POI 레코드");
            let Shape::Point(p) = shape else { continue };
            let id = text(&record, "nf_id");
            let (lat, lon) = tm.convert(p.x, p.y);
            b.pois.push(Poi {
                name: text(&record, "poi_nm"),
                address: addresses.remove(&id).unwrap_or_default(),
                lat,
                lon,
                class: text(&record, "poi_cl_dc").parse().unwrap_or(0),
                aliases: aliases.remove(&id).unwrap_or_default(),
            });
        }
        println!(
            "  {} 장소 {} 별칭 없는 ID {} {:.1}초",
            folder.file_name().unwrap().to_string_lossy(),
            b.pois.len() - before,
            aliases.len(),
            t.elapsed().as_secs_f64()
        );
    }
}

fn main() {
    let maps = maps_dir();
    let out = maps.join("routedb").join("geocode.idx");
    let tm = UtmkToWgs84::new();
    let mut b = Builder::default();
    let total = Instant::now();

    let t = Instant::now();
    let folders = sido_folders(&maps.join(EMAP_FOLDER));
    read_regions(&folders, &mut b);
    println!(
        "행정구역 시도 {} 시군구 {} 읍면동 {} 리 {} {:.1}초",
        b.sido.len(),
        b.sig.len(),
        b.emd.len(),
        b.li.len(),
        t.elapsed().as_secs_f64()
    );

    let t = Instant::now();
    let road_index = read_roads(&folders, &mut b);
    println!(
        "도로명 {} {:.1}초",
        b.roads.len(),
        t.elapsed().as_secs_f64()
    );

    let t = Instant::now();
    let (buildings, no_road) = read_buildings(&folders, &mut b, &road_index, &tm);
    println!(
        "건물 {buildings} 도로명 없는 건물 {no_road} 주소 {} {:.1}초",
        b.addresses.len(),
        t.elapsed().as_secs_f64()
    );

    let t = Instant::now();
    read_pois(&maps.join(POI_FOLDER), &mut b, &tm);
    println!("장소 {} {:.1}초", b.pois.len(), t.elapsed().as_secs_f64());

    let t = Instant::now();
    let stats = b.write(&out).expect("색인 파일을 쓸 수 없음");
    println!(
        "색인 저장 {} 장소 {} 별칭 {} 키 {} 목록 {} 도로명 {} 주소 {} {:.1}초 ({:.1}초 전체)",
        out.display(),
        stats.pois,
        stats.aliases,
        stats.keys,
        stats.postings,
        stats.roads,
        stats.addresses,
        t.elapsed().as_secs_f64(),
        total.elapsed().as_secs_f64()
    );
}

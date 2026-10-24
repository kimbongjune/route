//! 검색. 검색어를 주소로 해석해 건물 주소를 찾고, 2-gram 색인으로 장소 이름과 별칭을 찾는다

use crate::index::{GeocodeIndex, NO_LI};
use crate::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Address,
    Place,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub kind: Kind,
    /// 장소명. 주소면 도로명주소
    pub title: String,
    /// 장소의 도로명주소. 주소면 지번주소와 건물명
    pub address: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Default, PartialEq)]
pub struct Found {
    /// 검색어에 맞는 전체 건수
    pub total: usize,
    pub hits: Vec<Hit>,
}

/// 검색어 토큰에서 찾은 번호. 본번과 부번
struct Number {
    san: bool,
    mnnm: u32,
    slno: u32,
}

/// "110", "110-5", "산12-3", "656번지"를 번호로 읽는다
fn parse_number(token: &str) -> Option<Number> {
    let (san, rest) = match token.strip_prefix('산') {
        Some(r) => (true, r),
        None => (false, token),
    };
    let rest = rest.strip_suffix("번지").unwrap_or(rest);
    let (m, s) = match rest.split_once('-') {
        Some((m, s)) => (m, s),
        None => (rest, "0"),
    };
    if m.is_empty()
        || !m.chars().all(|c| c.is_ascii_digit())
        || !s.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(Number {
        san,
        mnnm: m.parse().ok()?,
        slno: s.parse().ok()?,
    })
}

/// "세종대로110"처럼 이름 뒤에 바로 붙은 번호를 나누고 "산 12"처럼 떨어진 산은 번호에 붙인다
fn split_tokens(q: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in q.split_whitespace() {
        if out.last().is_some_and(|t| t == "산") && parse_number(token).is_some() {
            *out.last_mut().unwrap() = format!("산{token}");
            continue;
        }
        let digits_at = token
            .char_indices()
            .find(|(_, c)| c.is_ascii_digit())
            .map(|(i, _)| i);
        match digits_at {
            Some(i) if i > 0 && parse_number(&token[i..]).is_some() => {
                let name = &token[..i];
                if name.ends_with(['로', '길', '동', '리', '가', '읍', '면']) {
                    out.push(name.to_string());
                    out.push(token[i..].to_string());
                    continue;
                }
                out.push(token.to_string());
            }
            _ => out.push(token.to_string()),
        }
    }
    out
}

/// 검색어 토큰이 가리키는 시도·시군구. 토큰마다 맞는 코드를 모으고 토큰끼리는 교집합이다. 비어 있으면 거르지 않는다
#[derive(Default)]
struct RegionFilter {
    sido: Vec<u64>,
    sig: Vec<u64>,
}

impl RegionFilter {
    fn allows_sig(&self, sig: u64) -> bool {
        (self.sig.is_empty() || self.sig.contains(&sig))
            && (self.sido.is_empty() || self.sido.contains(&(sig / 1000)))
    }
}

/// 행정구역 이름이 토큰으로 시작하거나 "창원시 의창구"처럼 띄어쓴 이름의 한 부분이 토큰으로 시작하면 맞는 것으로 본다
fn region_matches(name: &str, token: &str) -> bool {
    name.starts_with(token) || name.split_whitespace().any(|part| part.starts_with(token))
}

/// 시도는 "경남"처럼 첫 글자와 셋째 글자를 딴 줄임말과 "광주"처럼 이름 안에 든 토큰도 맞는 것으로 본다
fn sido_matches(name: &str, token: &str) -> bool {
    if region_matches(name, token) || name.contains(token) {
        return true;
    }
    let chars: Vec<char> = name.chars().collect();
    let short: String = chars.iter().step_by(2).take(2).collect();
    token.chars().count() == 2 && short == token
}

/// 토큰마다 맞는 코드를 교집합으로 모은다
fn narrow(current: &mut Vec<u64>, seen: &mut bool, matched: Vec<u64>) {
    if matched.is_empty() {
        return;
    }
    if *seen {
        current.retain(|c| matched.contains(c));
    } else {
        *current = matched;
        *seen = true;
    }
}

/// 빈 부분과 바로 앞과 같은 부분(세종특별자치시의 시도명과 시군구명)을 빼고 띄어쓰기로 잇는다
fn join_address(parts: &[&str]) -> String {
    let mut out = String::new();
    let mut last = "";
    for part in parts {
        if part.is_empty() || *part == last {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(part);
        last = part;
    }
    out
}

/// 같은 점수의 장소를 앞세우는 순서. 지명, 철도·공항과 기관, 관광·문화재, 건물, 그 밖의 시설, 상호, 진출입시설 순이다
fn class_rank(code: u64) -> u8 {
    match (
        code / 100_000_000_000,
        code / 1_000_000_000,
        code / 1_000_000,
    ) {
        (5, _, _) => 0,
        (_, _, 403001 | 403003) | (1, _, _) => 1,
        (_, 402 | 406 | 407 | 408 | 409, _) => 2,
        (_, 401, _) => 3,
        (_, _, 405003) => 6,
        (4, _, _) => 4,
        _ => 5,
    }
}

impl GeocodeIndex {
    /// 시군구 코드의 시도명과 시군구명
    fn sig_names(&self, sig: u64) -> (&str, &str) {
        let codes = self.region_code();
        let sido = self
            .sido_range()
            .find(|&k| codes[k] == sig / 1000)
            .map_or("", |k| self.region_name(k));
        let name = self
            .sig_range()
            .find(|&k| codes[k] == sig)
            .map_or("", |k| self.region_name(k));
        (sido, name)
    }

    fn region_filter(&self, tokens: &[String]) -> RegionFilter {
        let mut f = RegionFilter::default();
        let (mut sido_seen, mut sig_seen) = (false, false);
        let codes = self.region_code();
        for t in tokens {
            if t.chars().count() < 2 {
                continue;
            }
            let sido: Vec<u64> = self
                .sido_range()
                .filter(|&k| sido_matches(self.region_name(k), t))
                .map(|k| codes[k])
                .collect();
            let sig: Vec<u64> = self
                .sig_range()
                .filter(|&k| region_matches(self.region_name(k), t))
                .map(|k| codes[k])
                .collect();
            // 시도에 맞는 토큰은 시군구 필터에 넣지 않는다. "세종"은 시도이지 시군구가 아니다
            if sido.is_empty() {
                narrow(&mut f.sig, &mut sig_seen, sig);
            } else {
                narrow(&mut f.sido, &mut sido_seen, sido);
            }
        }
        f
    }

    /// 정규화한 이름이 같은 도로명 번호
    fn roads_named(&self, name: &[char]) -> Vec<u32> {
        let by_name = self.road_by_name();
        let norm = |r: u32| text::normalize(self.road_name(r as usize));
        let lo = by_name.partition_point(|&r| norm(r).as_slice() < name);
        let hi = by_name.partition_point(|&r| norm(r).as_slice() <= name);
        by_name[lo..hi].to_vec()
    }

    /// 도로명 r의 (본번, 부번) 주소 번호
    fn addresses_on(&self, road: u32, under: u8, n: &Number) -> Vec<usize> {
        let key = (road, under, n.mnnm, n.slno);
        let at = |a: usize| {
            (
                self.addr_road()[a],
                self.addr_under()[a],
                self.addr_mnnm()[a],
                self.addr_slno()[a],
            )
        };
        let count = self.address_count();
        let (mut lo, mut hi) = (0, count);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if at(mid) < key {
                lo = mid + 1
            } else {
                hi = mid
            }
        }
        (lo..count).take_while(|&a| at(a) == key).collect()
    }

    /// 읍면동(과 리)의 지번 주소 번호
    fn addresses_at(&self, emd: u32, li: u32, n: &Number) -> Vec<usize> {
        let sorted = self.jibun_sorted();
        let san = n.san as u8;
        let at = |a: usize| {
            (
                self.addr_emd()[a],
                self.addr_san()[a],
                self.addr_lnbr_mnnm()[a],
                self.addr_lnbr_slno()[a],
            )
        };
        let key = (emd, san, n.mnnm, n.slno);
        let lo = sorted.partition_point(|&a| at(a as usize) < key);
        sorted[lo..]
            .iter()
            .map(|&a| a as usize)
            .take_while(|&a| at(a) == key)
            .filter(|&a| li == NO_LI || self.addr_li()[a] == li)
            .collect()
    }

    /// 주소 a의 시군구 코드. 도로명의 관리 시군구와 다를 수 있으므로 읍면동 코드에서 얻는다
    fn addr_sig(&self, a: usize) -> u64 {
        self.region_code()[self.emd_range().start + self.addr_emd()[a] as usize] / 1000
    }

    /// 주소 a의 도로명주소 문자열. 읍·면은 넣고 동은 넣지 않는다
    pub fn road_address(&self, a: usize) -> String {
        let road = self.addr_road()[a] as usize;
        let (sido, sig) = self.sig_names(self.addr_sig(a));
        let emd = self.region_name(self.emd_range().start + self.addr_emd()[a] as usize);
        let town = if emd.ends_with(['읍', '면']) {
            emd
        } else {
            ""
        };
        let under = if self.addr_under()[a] == 1 {
            "지하 "
        } else {
            ""
        };
        let mut number = format!("{under}{}", self.addr_mnnm()[a]);
        if self.addr_slno()[a] > 0 {
            number.push_str(&format!("-{}", self.addr_slno()[a]));
        }
        join_address(&[sido, sig, town, self.road_name(road), &number])
    }

    /// 주소 a의 지번주소 문자열. 건물명이 있으면 괄호 안에 더한다
    pub fn lot_address(&self, a: usize) -> String {
        let emd_k = self.emd_range().start + self.addr_emd()[a] as usize;
        let emd_code = self.region_code()[emd_k];
        let (sido, sig) = self.sig_names(emd_code / 1000);
        let li = if self.addr_li()[a] != NO_LI {
            self.region_name(self.li_range().start + self.addr_li()[a] as usize)
        } else {
            ""
        };
        let san = if self.addr_san()[a] == 1 { "산" } else { "" };
        let mut number = self.addr_lnbr_mnnm()[a].to_string();
        if self.addr_lnbr_slno()[a] > 0 {
            number.push_str(&format!("-{}", self.addr_lnbr_slno()[a]));
        }
        let mut s = join_address(&[sido, sig, self.region_name(emd_k), li, san, &number]);
        let name = self.addr_name(a);
        if !name.is_empty() {
            s.push_str(&format!(" ({name})"));
        }
        s
    }

    fn address_hit(&self, a: usize) -> Hit {
        let (lat, lon) = self.addr_point(a);
        Hit {
            kind: Kind::Address,
            title: self.road_address(a),
            address: self.lot_address(a),
            lat,
            lon,
        }
    }

    /// 검색어를 "[시도] [시군구] 도로명 번호" 또는 "[시도] [시군구] 읍면동 [리] 지번"으로 해석한다
    fn search_address(&self, tokens: &[String]) -> Vec<usize> {
        let mut found = Vec::new();
        for i in 0..tokens.len().saturating_sub(1) {
            let Some(number) = parse_number(&tokens[i + 1]) else {
                continue;
            };
            let filter = self.region_filter(&tokens[..i]);
            let name = text::normalize(&tokens[i]);
            if !number.san {
                for road in self.roads_named(&name) {
                    for under in [0u8, 1] {
                        found.extend(
                            self.addresses_on(road, under, &number)
                                .into_iter()
                                .filter(|&a| filter.allows_sig(self.addr_sig(a))),
                        );
                    }
                }
            }
            let codes = self.region_code();
            // 리 이름이 맞으면 그 리의 지번, 읍면동 이름이 맞으면 읍면동 전체의 지번
            for k in self.li_range() {
                if self.region_name(k) == tokens[i] && filter.allows_sig(codes[k] / 100_000) {
                    let emd_code = codes[k] / 100;
                    if let Some(e) = self.emd_range().position(|m| codes[m] == emd_code) {
                        let li = (k - self.li_range().start) as u32;
                        found.extend(self.addresses_at(e as u32, li, &number));
                    }
                }
            }
            for k in self.emd_range() {
                if self.region_name(k) == tokens[i] && filter.allows_sig(codes[k] / 1000) {
                    let e = (k - self.emd_range().start) as u32;
                    found.extend(self.addresses_at(e, NO_LI, &number));
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }

    /// 2-gram 색인으로 장소 후보를 찾고 이름 또는 별칭에 모든 토큰이 들어 있는지 확인한다.
    /// (점수, 분류 순위, 거리 또는 이름 길이, 장소 번호)를 돌려준다
    fn search_place(
        &self,
        tokens: &[String],
        center: Option<(f64, f64)>,
    ) -> Vec<(u8, u8, f64, usize)> {
        let norm_tokens: Vec<Vec<char>> = tokens.iter().map(|t| text::normalize(t)).collect();
        let joined: Vec<char> = norm_tokens.concat();
        let mut keys: Vec<u32> = norm_tokens.iter().flat_map(|t| text::keys(t)).collect();
        keys.sort_unstable();
        keys.dedup();
        if keys.is_empty() {
            return Vec::new();
        }
        let mut lists = Vec::with_capacity(keys.len());
        for k in keys {
            match self.postings(k) {
                Some(list) => lists.push(list),
                None => return Vec::new(),
            }
        }
        lists.sort_by_key(|l| l.len());
        let mut candidates: Vec<u32> = lists[0].to_vec();
        for list in &lists[1..] {
            candidates.retain(|d| list.binary_search(d).is_ok());
        }

        let mut buf = Vec::new();
        let mut out = Vec::new();
        for doc in candidates {
            let doc = doc as usize;
            text::normalize_into(self.poi_name(doc), &mut buf);
            let mut score = if buf == joined {
                0
            } else if buf.starts_with(&joined) {
                1
            } else if text::contains(&buf, &joined) {
                2
            } else if norm_tokens.iter().all(|t| text::contains(&buf, t)) {
                3
            } else {
                4
            };
            if score == 4 {
                for alias in self.poi_aliases(doc) {
                    text::normalize_into(alias, &mut buf);
                    let s = if buf == joined {
                        1
                    } else if norm_tokens.iter().all(|t| text::contains(&buf, t)) {
                        3
                    } else {
                        4
                    };
                    score = score.min(s);
                }
                if score == 4 {
                    continue;
                }
            }
            let second = match center {
                Some(c) => text::distance_m(c, self.poi_point(doc)),
                None => self.poi_name(doc).chars().count() as f64,
            };
            out.push((score, class_rank(self.poi_class()[doc]), second, doc));
        }
        out
    }

    /// 검색. 주소 결과를 앞에, 장소 결과를 뒤에 두고 limit개까지 돌려준다.
    /// center가 있으면 같은 점수의 장소는 가까운 순이다
    pub fn search(&self, q: &str, center: Option<(f64, f64)>, limit: usize) -> Found {
        let tokens = split_tokens(q);
        if tokens.is_empty() {
            return Found::default();
        }
        let addresses = self.search_address(&tokens);
        let mut places = self.search_place(&tokens, center);
        places.sort_by(|a, b| (a.0, a.1, a.2).partial_cmp(&(b.0, b.1, b.2)).unwrap());
        let total = addresses.len() + places.len();
        let mut hits: Vec<Hit> = addresses
            .iter()
            .take(limit)
            .map(|&a| self.address_hit(a))
            .collect();
        for &(_, _, _, doc) in places.iter().take(limit.saturating_sub(hits.len())) {
            let (lat, lon) = self.poi_point(doc);
            hits.push(Hit {
                kind: Kind::Place,
                title: self.poi_name(doc).to_string(),
                address: self.poi_address(doc).to_string(),
                lat,
                lon,
            });
        }
        Found { total, hits }
    }

    /// 역지오코딩. 좌표에서 가장 가까운 주소(반경 radius_m 안)를 찾는다
    pub fn nearest_address(&self, lat: f64, lon: f64, radius_m: f64) -> Option<Hit> {
        let count = self.address_count();
        let dlat = radius_m / 111_000.0;
        let dlon = dlat / lat.to_radians().cos();
        let mut best: Option<(f64, usize)> = None;
        for a in 0..count {
            let (alat, alon) = self.addr_point(a);
            if (alat - lat).abs() > dlat || (alon - lon).abs() > dlon {
                continue;
            }
            let d = text::distance_m((lat, lon), (alat, alon));
            if d <= radius_m && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, a));
            }
        }
        best.map(|(_, a)| self.address_hit(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::tests::demo_file;

    #[test]
    fn numbers_and_tokens() {
        let n = parse_number("110-5").unwrap();
        assert_eq!((n.san, n.mnnm, n.slno), (false, 110, 5));
        let n = parse_number("산12").unwrap();
        assert_eq!((n.san, n.mnnm, n.slno), (true, 12, 0));
        assert_eq!(parse_number("656번지").unwrap().mnnm, 656);
        assert!(parse_number("12층").is_none());
        assert!(parse_number("-5").is_none());
        assert_eq!(
            split_tokens("중구 세종대로110-5"),
            ["중구", "세종대로", "110-5"]
        );
        assert_eq!(split_tokens("역삼동123"), ["역삼동", "123"]);
        assert_eq!(split_tokens("3호선 강남역"), ["3호선", "강남역"]);
        assert_eq!(split_tokens("효자동 산 1-2"), ["효자동", "산1-2"]);
    }

    #[test]
    fn address_search() {
        let (f, path) = demo_file("addr");
        let found = f.search("서울 중구 세종대로 110", None, 10);
        // 110-5는 다른 주소라 110 하나이고, 장소 서울특별시청은 이름에 토큰이 없어 빠진다
        assert_eq!(found.total, 1);
        assert_eq!(found.hits[0].title, "서울특별시 중구 세종대로 110");
        assert_eq!(
            found.hits[0].address,
            "서울특별시 중구 태평로1가 31 (서울특별시청)"
        );
        assert_eq!(found.hits[0].kind, Kind::Address);
        // 시군구가 틀리면 없다
        assert_eq!(f.search("용산구 세종대로 110", None, 10).total, 0);
        // 번호를 붙여 써도 되고 부번도 찾는다
        let found = f.search("세종대로110-5", None, 10);
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.hits[0].title, "서울특별시 중구 세종대로 110-5");
        // 지번
        let found = f.search("동자동 43-205", None, 10);
        assert_eq!(found.hits[0].title, "서울특별시 용산구 청파로 378");
        assert_eq!(
            found.hits[0].address,
            "서울특별시 용산구 동자동 43-205 (서울역)"
        );
        drop(f);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn place_search() {
        let (f, path) = demo_file("place");
        // 정확 일치가 앞, 포함이 뒤
        let found = f.search("서울역", None, 10);
        let titles: Vec<&str> = found.hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, ["서울역", "CJ올리브영 서울역점"]);
        assert_eq!(found.hits[0].kind, Kind::Place);
        assert_eq!(found.hits[0].address, "서울특별시 용산구 한강대로 405");
        // 토큰이 떨어져 있어도 모두 들어 있으면 찾는다
        let found = f.search("올리브영 서울역", None, 10);
        assert_eq!(found.hits[0].title, "CJ올리브영 서울역점");
        // 별칭으로 찾는다
        let found = f.search("시청", None, 10);
        assert_eq!(found.hits[0].title, "서울특별시청");
        // 지도 중심이 있으면 같은 점수에서 가까운 순. 시청 근처에서 "서울"은 시청이 먼저다
        let found = f.search("서울", Some((37.5666, 126.9784)), 10);
        assert_eq!(found.hits[0].title, "서울특별시청");
        let found = f.search("서울", Some((37.5547, 126.9707)), 10);
        assert_eq!(found.hits[0].title, "서울역");
        // 없는 2-gram이면 비어 있고 limit을 지킨다
        assert_eq!(f.search("없는곳", None, 10), Found::default());
        assert_eq!(f.search("서울", None, 1).hits.len(), 1);
        assert_eq!(f.search("서울", None, 1).total, 3);
        drop(f);
        std::fs::remove_file(path).unwrap();
    }

    /// 세종(시도명과 시군구명이 같음), 홍성군 홍북읍, 창원시 의창구·성산구의 중앙대로, 경복궁 문화재와 식당
    fn demo_wide(name: &str) -> (GeocodeIndex, std::path::PathBuf) {
        use crate::index::{Address, Builder, Poi, Region, Road};
        let mut b = Builder::default();
        for (code, name) in [(36, "세종특별자치시"), (44, "충청남도"), (48, "경상남도")]
        {
            b.sido.push(Region {
                code,
                name: name.into(),
            });
        }
        for (code, name) in [
            (36110, "세종특별자치시"),
            (44800, "홍성군"),
            (48121, "창원시 의창구"),
            (48123, "창원시 성산구"),
        ] {
            b.sig.push(Region {
                code,
                name: name.into(),
            });
        }
        for (code, name) in [
            (36110101, "보람동"),
            (44800253, "홍북읍"),
            (48121101, "사림동"),
            (48123101, "중앙동"),
        ] {
            b.emd.push(Region {
                code,
                name: name.into(),
            });
        }
        b.li.push(Region {
            code: 4480025321,
            name: "신경리".into(),
        });
        for (sig, rn, name) in [
            (36110, 5001, "한누리대로"),
            (44800, 6001, "충남대로"),
            (48123, 4001, "중앙대로"),
        ] {
            b.roads.push(Road {
                sig,
                rn,
                name: name.into(),
            });
        }
        for (road, mnnm, emd, li, lat, lon) in [
            (0, 2130, 0, NO_LI, 36.48, 127.29),
            (1, 21, 1, 0, 36.66, 126.67),
            (2, 300, 2, NO_LI, 35.23, 128.68),
            (2, 300, 3, NO_LI, 35.22, 128.68),
        ] {
            b.addresses.push(Address {
                road,
                under: 0,
                mnnm,
                slno: 0,
                lat,
                lon,
                emd,
                li,
                san: 0,
                lnbr_mnnm: 1,
                lnbr_slno: 0,
                name: String::new(),
            });
        }
        // 식당 경복궁이 광화문 중심에 더 가깝지만 문화재가 먼저다
        b.pois.push(Poi {
            name: "경복궁".into(),
            address: String::new(),
            lat: 37.5796,
            lon: 126.9770,
            class: 407001000000,
            aliases: Vec::new(),
        });
        b.pois.push(Poi {
            name: "경복궁".into(),
            address: String::new(),
            lat: 37.5720,
            lon: 126.9760,
            class: 203073021100,
            aliases: Vec::new(),
        });
        let path = std::env::temp_dir().join(format!("geocode_{name}_{}.idx", std::process::id()));
        b.write(&path).unwrap();
        (GeocodeIndex::open(&path).unwrap(), path)
    }

    #[test]
    fn region_parts_and_display() {
        let (f, path) = demo_wide("wide");
        // 시군구 이름의 한 부분 "의창구"로 거른다. 도로는 성산구가 관리하지만 건물이 있는 의창구로 거르고 표시한다
        let found = f.search("경남 창원시 의창구 중앙대로 300", None, 10);
        assert_eq!(found.total, 1);
        assert_eq!(found.hits[0].title, "경상남도 창원시 의창구 중앙대로 300");
        assert_eq!(f.search("창원시 중앙대로 300", None, 10).total, 2);
        // 세종은 시도명을 한 번만, 읍·면은 도로명 앞에 넣는다
        assert_eq!(
            f.search("한누리대로 2130", None, 10).hits[0].title,
            "세종특별자치시 한누리대로 2130"
        );
        let found = f.search("홍북읍 충남대로 21", None, 10);
        assert_eq!(found.hits[0].title, "충청남도 홍성군 홍북읍 충남대로 21");
        assert_eq!(found.hits[0].address, "충청남도 홍성군 홍북읍 신경리 1");
        // 같은 이름이면 분류 순위가 거리보다 먼저다
        let found = f.search("경복궁", Some((37.5715, 126.9765)), 10);
        assert_eq!(found.hits[0].lat, 37.5796);
        assert_eq!(class_rank(510001010300), 0);
        assert_eq!(class_rank(403003010900), 1);
        assert_eq!(class_rank(103002000000), 1);
        assert_eq!(class_rank(401001030000), 3);
        assert_eq!(class_rank(403002010600), 4);
        assert_eq!(class_rank(203073021100), 5);
        assert_eq!(class_rank(405003000000), 6);
        assert!(region_matches("창원시 의창구", "의창구") && region_matches("서울특별시", "서울"));
        assert!(!region_matches("창원시 의창구", "창구"));
        assert!(sido_matches("경상남도", "경남") && sido_matches("전남광주통합특별시", "광주"));
        assert!(!sido_matches("경상남도", "경북"));
        // 시도 줄임말로도 거른다
        assert_eq!(f.search("경남 중앙대로 300", None, 10).total, 2);
        assert_eq!(f.search("충남 중앙대로 300", None, 10).total, 0);
        assert_eq!(
            join_address(&["세종특별자치시", "세종특별자치시", "", "한누리대로"]),
            "세종특별자치시 한누리대로"
        );
        drop(f);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reverse() {
        let (f, path) = demo_file("reverse");
        let hit = f.nearest_address(37.5665, 126.9785, 100.0).unwrap();
        assert_eq!(hit.title, "서울특별시 중구 세종대로 110");
        assert!(f.nearest_address(35.0, 129.0, 100.0).is_none());
        drop(f);
        std::fs::remove_file(path).unwrap();
    }
}

"""무인교통단속카메라를 표준노드링크 링크에 매칭한다.

입력
  전국무인교통단속카메라표준데이터.csv
  도로명주소 전자지도 도로구간(TL_SPRD_MANAGE), 표준노드링크 MOCT_LINK
규칙(07 문서 단속 방향의 처리 기준)
  - 카메라가 있는 전자지도 도로를 과속방지턱과 같은 방법으로 정하고, 그 도로가 표준노드링크에 없으면 제외한다.
    도로노선명에서 "~로", "~길"까지 남긴 이름과 같은 도로가 50m 안에 있으면 그 도로이고, 없으면 30m 안에서 가장 가까운 도로이다.
  - 100m 안에 단속구분과 제한속도가 같고 설치장소에 방향 표기가 있는 카메라가 있으면, 방향 표기가 없는 카메라는 중복 등록으로 보고 제외한다.
  - 구간단속은 구간길이가 있는 시점 카메라마다 직선거리가 구간길이의 0.5~1.1배인 종점 카메라 중 구간길이에 가장 가까운 것을 짝으로 정한다.
    시점 카메라는 종점 쪽, 종점 카메라는 시점에서 종점으로 가는 쪽 링크에 매칭한다.
  - 설치장소에 "A→B"가 있으면 B, "B 방면", "B방향"이 있으면 B를 브이월드 장소 검색(약 5km 안)과 행정구역 검색(약 30km 안)으로 찾아
    카메라에서 B 쪽으로 가는 링크에 매칭한다. 검색 결과는 maps/전국무인교통단속카메라표준데이터/방향지명_좌표.json에 저장하여 다시 쓴다.
  - 방향이 정해진 카메라는 30m 안 가장 가까운 링크와 그 반대 방향 링크 중 정해진 방향과 나란한(cos > 0.5) 링크 하나에 매칭한다.
    나란한 링크가 없으면 방향을 모르는 카메라와 같이 처리한다.
  - 그 밖의 카메라는 30m 안 가장 가까운 링크와, 30m 안에 반대 방향(cos < -0.8) 링크가 있으면 그 링크에도 매칭한다.
  - 링크 시작점에서 카메라를 링크에 투영한 위치까지의 거리(m)와 그 거리를 링크 좌표 길이로 나눈 비율을 기록한다.
출력
  전국무인교통단속카메라표준데이터/카메라_링크.csv   카메라와 매칭한 링크를 한 행에 하나씩 기록한다.
    관리번호는 시청·구청이 매긴 "14" 같은 번호가 지자체끼리 겹치므로 좌표 정상 행의 순번을 카메라번호로 기록한다.

키는 .env의 VWORLD_API_KEY.
python -X utf8 scripts/camera/match_links.py
"""
import json
import re
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

import geopandas as gpd
import numpy as np
import pandas as pd
from shapely.geometry import Point

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "nodelink"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "speedbump"))
from alley_data_check import address_roads, local_dir, moct_links
from common import ROOT, env, read_csv, write_csv
from match_links import covered_roads, place_roads, road_name
from nodelink import TM, WGS84, points

DIR = ROOT / "maps" / "전국무인교통단속카메라표준데이터"
SRC = DIR / "전국무인교통단속카메라표준데이터.csv"
OUT = DIR / "카메라_링크.csv"
CACHE = DIR / "방향지명_좌표.json"
MAX_DIST = 30
DUP_DIST = 100
COLS = ["카메라번호", "관리번호", "위도", "경도", "도로노선명", "설치장소", "단속구분", "제한속도", "구간위치", "구간짝", "구간길이m", "방향근거",
        "LINK_ID", "링크시작거리", "링크비율", "링크까지거리"]


def load():
    cam = read_csv(SRC)
    cam["위도"] = pd.to_numeric(cam.위도, errors="coerce")
    cam["경도"] = pd.to_numeric(cam.경도, errors="coerce")
    cam = cam[cam.위도.between(33, 39) & cam.경도.between(124, 132)].reset_index(drop=True)
    cam["kind"] = cam.단속구분.str.replace("0", "", regex=False)
    cam["spd"] = pd.to_numeric(cam.제한속도, errors="coerce")
    cam["pos"] = cam.단속구간위치구분.fillna("").str.lstrip("0")
    cam["L"] = pd.to_numeric(cam.과속단속구간길이, errors="coerce") * 1000
    t = cam.설치장소.fillna("")
    arrow = t.str.extract(r"(?:→|->)\s*([^()→]+)")[0].str.strip()
    toward = t.str.extract(r"([가-힣A-Za-z0-9]+?)\s*(?:방면|방향)")[0]
    cam["B"] = arrow.where(arrow.notna() & ~t.str.contains("↔"), toward)
    cam["both_text"] = t.str.contains("↔|양방향|양면")
    cam["has_text"] = cam.B.notna() | cam.both_text
    return cam


def duplicates(cam, g):
    """방향 표기가 없고 100m 안에 같은 단속구분·제한속도의 방향 표기 카메라가 있는 카메라"""
    a = g[["geometry"]].assign(kind=cam.kind, spd=cam.spd)
    no_text = a[~cam.has_text]
    j = gpd.sjoin(no_text.assign(geometry=no_text.buffer(DUP_DIST)), a[cam.has_text], predicate="intersects")
    j = j[(j.kind_left == j.kind_right) & (j.spd_left == j.spd_right)]
    return pd.Index(j.index.unique())


def section_pairs(cam, g):
    """구간단속 시점 번호 -> 종점 번호"""
    st = cam[(cam.pos == "1") & (cam.L > 0)]
    ed = cam[cam.pos == "2"]
    pairs = {}
    for i, r in st.iterrows():
        d = g.geometry.loc[ed.index].distance(g.geometry.loc[i])
        ok = d[(d >= 0.5 * r.L) & (d <= 1.1 * r.L)]
        if len(ok):
            pairs[i] = (ok - r.L).abs().idxmin()
    return pairs


class Geocoder:
    def __init__(self):
        self.key = env("VWORLD_API_KEY")
        self.cache = json.loads(CACHE.read_text(encoding="utf-8")) if CACHE.exists() else {}
        self.calls = 0

    def find(self, name, lon, lat):
        """name을 (lon, lat) 근처에서 찾은 WGS84 좌표. 같은 이름·약 5km 칸은 한 번만 찾는다"""
        k = f"{name}|{round(lon / 0.05)}|{round(lat / 0.05)}"
        if k not in self.cache:
            self.cache[k] = self._search(name, lon, lat)
            self.calls += 1
            if self.calls % 500 == 0:
                self.save()
        return self.cache[k]

    def _search(self, q, lon, lat):
        for typ, r in (("place", 0.05), ("district", 0.3)):
            params = {"service": "search", "request": "search", "version": "2.0", "crs": "EPSG:4326", "size": 1, "page": 1, "query": q,
                      "type": typ, "format": "json", "errorformat": "json", "key": self.key, "bbox": f"{lon - r},{lat - r},{lon + r},{lat + r}"}
            if typ == "district":
                params["category"] = "L4"
            for _ in range(3):
                try:
                    res = json.load(urllib.request.urlopen("https://api.vworld.kr/req/search?" + urllib.parse.urlencode(params), timeout=20))["response"]
                    break
                except Exception:
                    time.sleep(1)
            else:
                continue
            time.sleep(0.05)
            if res["status"] == "OK" and res["result"]["items"] and "point" in res["result"]["items"][0]:
                p = res["result"]["items"][0]["point"]
                return [float(p["x"]), float(p["y"])]
        return None

    def save(self):
        CACHE.write_text(json.dumps(self.cache, ensure_ascii=False), encoding="utf-8")


def to_tm(lon, lat):
    return gpd.GeoSeries([Point(lon, lat)], crs=WGS84).to_crs(TM).iloc[0]


def main():
    cam = load()
    g = points(cam, "위도", "경도")
    links = moct_links()
    roads = address_roads()
    road, _ = place_roads(g, cam.도로노선명.map(road_name), roads)
    ok = covered_roads(road.unique(), roads, links)
    on = road.index[road.isin(ok)]
    dup = duplicates(cam, g)
    keep = on.difference(dup)
    pairs = section_pairs(cam, g)
    start_of = {e: s for s, e in pairs.items()}
    print(f"카메라 {len(cam):,}대 | 있는 도로가 표준노드링크에 있음 {len(on):,}대 | 중복 등록으로 제외 {len(on.intersection(dup)):,}대"
          f" | 매칭 대상 {len(keep):,}대 | 구간길이가 있는 구간단속 시점 {((cam.pos == '1') & (cam.L > 0)).sum():,}대 중 짝을 정함 {len(pairs):,}개")

    geo = Geocoder()
    rows = []
    count = {"구간단속": 0, "방향 지명": 0, "양방향 표기": 0, "방향 모름": 0, "지명 못 찾음": 0, "방향이 도로와 나란하지 않음": 0, "30m 안 링크 없음": 0}
    for n, i in enumerate(keep):
        r = cam.loc[i]
        p = g.geometry.loc[i]
        near = links.iloc[links.sindex.query(p.buffer(MAX_DIST), predicate="intersects")]
        near = near.assign(d=near.distance(p))
        near = near[near.d <= MAX_DIST].sort_values("d")
        if near.empty:
            count["30m 안 링크 없음"] += 1
            continue
        first = near.iloc[0]
        v = local_dir(first.geometry, p)
        opp = None
        for _, row in near.iloc[1:].iterrows():
            if float(local_dir(row.geometry, p) @ v) < -0.8:
                opp = row
                break

        target, basis = None, None
        if i in pairs:
            q = g.geometry.loc[pairs[i]]
            target, basis = np.array([q.x - p.x, q.y - p.y]), "구간단속"
        elif i in start_of:
            s, e = g.geometry.loc[start_of[i]], p
            target, basis = np.array([e.x - s.x, e.y - s.y]), "구간단속"
        elif isinstance(r.B, str) and r.B:
            b = geo.find(r.B, r.경도, r.위도)
            if b is None:
                count["지명 못 찾음"] += 1
            else:
                q = to_tm(*b)
                target, basis = np.array([q.x - p.x, q.y - p.y]), "방향 지명"
        picked = None
        if target is not None and np.hypot(*target) > 0:
            t = target / np.hypot(*target)
            for row in [first] + ([opp] if opp is not None else []):
                if float(local_dir(row.geometry, p) @ t) > 0.5:
                    picked = [row]
                    break
            if picked is None:
                count["방향이 도로와 나란하지 않음"] += 1
                basis = None
        if picked is None:
            picked = [first] + ([opp] if opp is not None else [])
            basis = "양방향 표기" if r.both_text else "방향 모름"
        count[basis] += 1
        sec_pos = {"1": "시점", "2": "종점"}.get(r.pos, "") if (i in pairs or i in start_of) else ""
        mate = cam.무인교통단속카메라관리번호.loc[pairs[i]] if i in pairs else (cam.무인교통단속카메라관리번호.loc[start_of[i]] if i in start_of else "")
        length = r.L if i in pairs else (cam.L.loc[start_of[i]] if i in start_of else np.nan)
        for row in picked:
            at = row.geometry.project(p)
            rows.append([i, r.무인교통단속카메라관리번호, r.위도, r.경도, r.도로노선명, r.설치장소, r.kind, r.제한속도, sec_pos, mate, length, basis,
                         row.LINK_ID, round(at, 2), round(at / row.geometry.length, 6) if row.geometry.length else 0.0, round(row.d, 2)])
        if (n + 1) % 5000 == 0:
            print(f"  {n + 1:,}대 처리 | 지명 검색 {geo.calls:,}번", flush=True)
    geo.save()
    out = pd.DataFrame(rows, columns=COLS)
    write_csv(OUT, out, COLS)
    print(f"매칭한 카메라 {out.카메라번호.nunique():,}대 | 저장한 행 {len(out):,} | 이번 실행의 지명 검색 {geo.calls:,}번"
          f" | 저장된 지명 {len(geo.cache):,}건 중 좌표를 찾음 {sum(v is not None for v in geo.cache.values()):,}건")
    print("  방향 근거별 카메라", {k: f"{v:,}" for k, v in count.items()})
    per = out.groupby("카메라번호").size()
    print(f"  한 방향 링크에 매칭 {(per == 1).sum():,}대 | 양방향 링크에 매칭 {(per == 2).sum():,}대")
    print("  단속구분별 방향 근거", out.drop_duplicates("카메라번호").pivot_table(index="단속구분", columns="방향근거", values="카메라번호", aggfunc="count", fill_value=0).to_dict("index"))
    print(f"저장 {OUT}")


if __name__ == "__main__":
    main()

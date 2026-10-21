"""무인교통단속카메라의 단속 방향을 정할 근거를 확인한다.

- 설치장소 방향 표기(화살표, 방면, 방향, ↔) 건수와 도로노선방향 값의 관계
- 상행·하행이 표준 정의(상행 = 노선 종점→기점)대로 입력되었는지: 서해안·경부고속도로, 시내 도로 도로명주소 기초번호
- 설치장소 화살표 지명을 브이월드 검색으로 찾아 단속 방향 링크를 고르는 표본 시험과 OSM 단속카메라 방향 대조
- 좌표 위치(링크 좌우, 교차로와의 관계)로 방향을 가릴 수 있는지
- 같은 자리 상행·하행 짝, 붙은 관리번호 짝, 관리번호 앞 글자

키는 .env의 VWORLD_API_KEY. 표준노드링크, 도로명주소 전자지도, OSM 파일이 maps에 있어야 한다.
python -X utf8 scripts/camera/direction_check.py
"""
import glob
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
import pyogrio
from shapely.geometry import Point

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "nodelink"))
from alley_data_check import local_dir, moct_links
from common import ROOT, env, read_csv
from nodelink import TM, WGS84, points, read_links, read_nodes

SRC = ROOT / "maps" / "전국무인교통단속카메라표준데이터" / "전국무인교통단속카메라표준데이터.csv"
ARROW = r"→|->"
ARROW_AB = r"([^()→]+?)\s*(?:→|->)\s*([^()→]+)"
KEY = None


def load():
    cam = read_csv(SRC)
    cam["위도"] = pd.to_numeric(cam.위도, errors="coerce")
    cam["경도"] = pd.to_numeric(cam.경도, errors="coerce")
    cam = cam[cam.위도.between(33, 39) & cam.경도.between(124, 132)].reset_index(drop=True)
    cam["dir"] = cam.도로노선방향.str.lstrip("0")
    cam["kind"] = cam.단속구분.str.replace("0", "", regex=False)
    cam["spd"] = pd.to_numeric(cam.제한속도, errors="coerce")
    ab = cam.설치장소.fillna("").str.extract(ARROW_AB)
    cam["A"], cam["B"] = ab[0].str.strip(), ab[1].str.strip()
    return cam


def search(q, lon, lat):
    """브이월드 장소 검색(약 5km 안), 없으면 행정구역 검색(약 30km 안)의 첫 결과 좌표"""
    for typ, r in (("place", 0.05), ("district", 0.3)):
        params = {"service": "search", "request": "search", "version": "2.0", "crs": "EPSG:4326", "size": 1, "page": 1, "query": q,
                  "type": typ, "format": "json", "errorformat": "json", "key": KEY, "bbox": f"{lon - r},{lat - r},{lon + r},{lat + r}"}
        if typ == "district":
            params["category"] = "L4"
        try:
            res = json.load(urllib.request.urlopen("https://api.vworld.kr/req/search?" + urllib.parse.urlencode(params), timeout=20))["response"]
        except Exception:
            continue
        time.sleep(0.05)
        if res["status"] == "OK" and res["result"]["items"] and "point" in res["result"]["items"][0]:
            p = res["result"]["items"][0]["point"]
            return gpd.GeoSeries([Point(float(p["x"]), float(p["y"]))], crs=WGS84).to_crs(TM).iloc[0]
    return None


def arrow_travel(links, c, b_name, lon, lat):
    """카메라 c에서 화살표 뒤 지명 쪽으로 가는 링크 방향. 가장 가까운 링크 방향 v와 함께 반환"""
    near = links.iloc[links.sindex.query(c.buffer(30), predicate="intersects")]
    if near.empty:
        return None
    near = near.assign(d=near.distance(c)).sort_values("d")
    v = local_dir(near.iloc[0].geometry, c)
    b = search(b_name, lon, lat)
    if b is None:
        return None
    vb = np.array([b.x - c.x, b.y - c.y])
    vb /= np.hypot(*vb)
    if abs(vb @ v) < 0.5:
        return None
    return (v if vb @ v > 0 else -v), v, near


def text_counts(cam):
    t = cam.설치장소.fillna("")
    arrow = t.str.contains(ARROW)
    print(f"== 카메라 {len(cam):,} | 도로노선방향", cam.dir.value_counts().to_dict())
    print(f"  설치장소 화살표 {arrow.sum():,} | 방면 {(t.str.contains('방면') & ~arrow).sum():,} | 방향 {(t.str.contains('방향') & ~arrow & ~t.str.contains('방면')).sum():,}"
          f" | ↔ {t.str.contains('↔').sum():,}")
    one = arrow & ~t.str.contains("↔")
    both = t.str.contains("↔|양방향|양면")
    print("  한 방향 화살표의 도로노선방향", cam[one].dir.value_counts().to_dict())
    print("  양방향 표기(↔·양방향·양면)의 도로노선방향", cam[both].dir.value_counts().to_dict())


def updown_expressway(cam):
    links = read_links(["LINK_ID", "ROAD_NAME", "ROAD_RANK", "ROAD_USE"])
    links = links[(links.ROAD_USE != "1") & (links.ROAD_RANK == "101")].reset_index(drop=True).set_crs(TM, allow_override=True)
    print("== 고속국도 상행·하행과 가장 가까운 링크 방향(종점 서울)")
    for name in ["서해안", "경부"]:
        c = cam[(cam.도로종류 == "고속국도") & cam.도로노선명.str.contains(name, na=False) & cam.dir.isin(["1", "2"])].reset_index(drop=True)
        g = points(c, "위도", "경도")
        road = links[links.ROAD_NAME.str.contains(name, na=False)]
        j = gpd.sjoin_nearest(g, road[["geometry"]], distance_col="d", max_distance=40)
        j = j[~j.index.duplicated()]
        j["north"] = [local_dir(road.geometry.loc[k], g.geometry.loc[i])[1] > 0 for i, k in zip(j.index, j.index_right)]
        j["dir"] = c.dir.loc[j.index].map({"1": "상행", "2": "하행"})
        print(f"  {name}고속도로 {len(j)}")
        print(pd.crosstab(j.dir, j.north.map({True: "서울 쪽(북쪽)", False: "남쪽"})).to_string())


def arrow_sample(cam, links):
    s = cam.dropna(subset=["A", "B"]).sample(200, random_state=7).reset_index(drop=True)
    g = points(s, "위도", "경도")
    stat = {"링크 없음": 0, "B 못 찾음": 0}
    rows = []
    for i, r in s.iterrows():
        c = g.geometry.loc[i]
        near = links.iloc[links.sindex.query(c.buffer(30), predicate="intersects")]
        if near.empty:
            stat["링크 없음"] += 1
            continue
        near = near.assign(d=near.distance(c)).sort_values("d")
        v1 = local_dir(near.iloc[0].geometry, c)
        opp = any(float(local_dir(row.geometry, c) @ v1) < -0.8 for _, row in near.iloc[1:].iterrows())
        b, a = search(r.B, r.경도, r.위도), search(r.A, r.경도, r.위도)
        if b is None:
            stat["B 못 찾음"] += 1
            continue
        vb = np.array([b.x - c.x, b.y - c.y])
        vb /= np.hypot(*vb)
        ab_first = None
        if a is not None:
            vab = np.array([b.x - a.x, b.y - a.y])
            if np.hypot(*vab) > 0:
                ab_first = float((vab / np.hypot(*vab)) @ v1) > 0
        rows.append((abs(float(vb @ v1)), float(vb @ v1) > 0, opp, ab_first))
    d = pd.DataFrame(rows, columns=["abscos", "first", "opp", "ab_first"])
    print(f"== 화살표 표본 200 | {stat} | B 찾음 {len(d)} | 카메라→B가 도로와 나란함 {(d.abscos > 0.5).sum()}")
    print(f"  30m 안 반대 방향 링크 있음 {d.opp.sum()} | 그중 B 쪽이 가장 가까운 링크가 아님 {(d.opp & ~d['first']).sum()}")
    both = d[d.ab_first.notna() & (d.abscos > 0.5)]
    print(f"  A·B 둘 다 찾고 나란함 {len(both)} | 카메라→B와 A→B가 같은 링크 방향 {(both['first'] == both.ab_first).sum()}")


def osm_compare(cam, links):
    pt = pyogrio.read_dataframe(ROOT / "maps" / "osm" / "south-korea-261020.osm.pbf", layer="points", where="highway = 'speed_camera'", columns=["other_tags"])
    print(f"== OSM 단속카메라 {len(pt)} | direction 태그 {pt.other_tags.fillna('').str.contains(chr(34) + 'direction' + chr(34)).sum()}")
    pt["deg"] = pd.to_numeric(pt.other_tags.fillna("").str.extract(r'"direction"=>"([^"]*)"')[0], errors="coerce")
    pt = pt.dropna(subset=["deg"]).to_crs(TM).reset_index(drop=True)
    c = cam.dropna(subset=["B"]).reset_index(drop=True)
    g = points(c, "위도", "경도")
    j = gpd.sjoin_nearest(pt, g[["B", "위도", "경도", "geometry"]], max_distance=30, distance_col="d")
    j = j[~j.index.duplicated()]
    out = []
    for _, r in j.iterrows():
        res = arrow_travel(links, g.geometry.loc[r.index_right], r.B, r.경도, r.위도)
        if res is None:
            continue
        travel, v, _ = res
        bear = lambda u: (np.degrees(np.arctan2(u[0], u[1])) + 360) % 360
        diff = lambda u: abs((r.deg - bear(u) + 180) % 360 - 180)
        out.append((diff(travel), diff(v)))
    o = np.array(out)
    print(f"  각도 있는 OSM 카메라 30m 안 화살표 카메라와 비교 {len(o)}")
    for k, name in [(0, "화살표로 고른 링크"), (1, "가장 가까운 링크")]:
        d = o[:, k]
        print(f"    {name}: OSM 각도와 정반대 {(d >= 135).sum()} | 같은 방향 {(d <= 45).sum()} | 그 사이 {((d > 45) & (d < 135)).sum()}")


def position_rules(cam, links):
    nodes = read_nodes(["NODE_ID", "NODE_TYPE"])
    nodes = nodes[nodes.NODE_TYPE == "101"].reset_index(drop=True).set_crs(TM, allow_override=True)
    s = cam.dropna(subset=["A", "B"]).sample(400, random_state=11).reset_index(drop=True)
    g = points(s, "위도", "경도")
    side = []
    for i, r in s.iterrows():
        c = g.geometry.loc[i]
        res = arrow_travel(links, c, r.B, r.경도, r.위도)
        if res is None:
            continue
        travel, v, near = res
        opp = [k for k, row in near.iloc[1:].iterrows() if float(local_dir(row.geometry, c) @ v) < -0.8]
        if not opp:
            continue
        tl, other = (near.iloc[0], near.loc[opp[0]]) if travel @ v > 0 else (near.loc[opp[0]], near.iloc[0])
        q = tl.geometry.interpolate(tl.geometry.project(c))
        w = np.array([c.x - q.x, c.y - q.y])
        side.append((travel[0] * w[1] - travel[1] * w[0] < 0, c.distance(tl.geometry) < c.distance(other.geometry)))
    sd = pd.DataFrame(side, columns=["right", "nearer"])
    print(f"== 왕복 도로 화살표 카메라 {len(sd)} | 단속 방향 링크 오른쪽 {sd.right.sum()} 왼쪽 {(~sd.right).sum()} | 단속 방향 링크가 더 가까움 {sd.nearer.sum()}")
    for kname, kinds in [("신호", ["2", "1+2"]), ("속도", ["1"])]:
        t = cam.dropna(subset=["B"])
        t = t[t.kind.isin(kinds) & (t.도로종류 != "고속국도")].sample(500, random_state=8).reset_index(drop=True)
        g = points(t, "위도", "경도")
        rows = []
        for i, r in t.iterrows():
            c = g.geometry.loc[i]
            res = arrow_travel(links, c, r.B, r.경도, r.위도)
            if res is None:
                continue
            travel, _, near = res
            nn = nodes.iloc[nodes.sindex.query(c.buffer(150), predicate="intersects")]
            nn = nn[nn.distance(c) > 5]
            if nn.empty:
                continue
            n = nn.geometry.loc[nn.distance(c).idxmin()]
            vn = np.array([n.x - c.x, n.y - c.y])
            dn = np.hypot(*vn)
            rows.append((float(travel @ (vn / dn)), dn, near.d.min()))
        d = pd.DataFrame(rows, columns=["cos", "dist", "dl"])
        d = d[d.cos.abs() > 0.5]
        e = d[d.dist <= 30]
        print(f"  {kname}: 교차로 30m 안 {len(e)} | 교차로에서 멀어지는 방향 {(e.cos < 0).mean():.0%}"
              f" | 그중 도로 8m 안 {(e.dl <= 8).sum()} 멀어지는 방향 {(e[e.dl <= 8].cos < 0).mean():.0%}")


def updown_urban(cam, links):
    iv = pd.concat([pyogrio.read_dataframe(f, encoding="cp949", columns=["RDS_MAN_NO", "SIG_CD", "ODD_BSI_MN", "EVE_BSI_MN"])
                    for f in glob.glob(str(ROOT / "maps" / "도로명주소 전자지도" / "*" / "*" / "TL_SPRD_INTRVL.shp"))], ignore_index=True)
    iv = gpd.GeoDataFrame(iv, geometry="geometry", crs="EPSG:5179").to_crs(TM)
    iv["no"] = pd.to_numeric(iv.ODD_BSI_MN, errors="coerce").fillna(0).clip(lower=0) + pd.to_numeric(iv.EVE_BSI_MN, errors="coerce").fillna(0).clip(lower=0)
    iv["road"] = iv.SIG_CD.astype(str) + "_" + iv.RDS_MAN_NO.astype(str)
    iv["mid"] = iv.geometry.interpolate(0.5, normalized=True)
    s = cam.dropna(subset=["B"])
    s = s[s.dir.isin(["1", "2"]) & (s.도로종류 != "고속국도")].sample(400, random_state=21).reset_index(drop=True)
    g = points(s, "위도", "경도")
    rows = []
    for i, r in s.iterrows():
        c = g.geometry.loc[i]
        res = arrow_travel(links, c, r.B, r.경도, r.위도)
        if res is None:
            continue
        travel, v, _ = res
        cand = iv.iloc[iv.sindex.query(c.buffer(40), predicate="intersects")]
        cand = cand[[abs(float(local_dir(gg, c) @ v)) > 0.7 for gg in cand.geometry]]
        if cand.empty:
            continue
        road = cand.iloc[cand.geometry.distance(c).argmin()].road
        same = iv.iloc[iv.sindex.query(c.buffer(200), predicate="intersects")]
        same = same[(same.road == road) & (same.no > 0)]
        if len(same) < 2:
            continue
        lo, hi = same.loc[same.no.idxmin()].mid, same.loc[same.no.idxmax()].mid
        up = np.array([hi.x - lo.x, hi.y - lo.y])
        if np.hypot(*up):
            rows.append((r.dir, float(travel @ (up / np.hypot(*up)))))
    d = pd.DataFrame(rows, columns=["dir", "cos"])
    d = d[d.cos.abs() > 0.5]
    d["단속"] = np.where(d.cos > 0, "기점→종점(번호 커짐)", "종점→기점(번호 작아짐)")
    print(f"== 시내 도로 상행·하행과 도로명주소 기초번호 방향 {len(d)}")
    print(pd.crosstab(d.dir.map({"1": "상행", "2": "하행"}), d.단속).to_string())


def pairs_and_ids(cam):
    g = points(cam, "위도", "경도")
    j = gpd.sjoin(g[["dir", "kind", "geometry"]].assign(geometry=g.buffer(30)), g[["dir", "kind", "geometry"]], predicate="intersects")
    j = j[(j.index < j.index_right) & (j.kind_left == j.kind_right)]
    p = j[((j.dir_left == "1") & (j.dir_right == "2")) | ((j.dir_left == "2") & (j.dir_right == "1"))]
    print(f"== 30m 안 같은 단속구분 상행·하행 짝 {len(p):,} | 카메라 {len(set(p.index) | set(p.index_right)):,}")
    cam["pre"] = cam.무인교통단속카메라관리번호.fillna("").str.extract(r"^([A-Za-z]+)")[0].fillna("(숫자)")
    cam["yr"] = pd.to_numeric(cam.설치연도, errors="coerce")
    print("== 관리번호 앞 글자")
    for pre in ["F", "G", "H", "J", "(숫자)", "P", "PE", "ICHEON"]:
        s = cam[cam.pre == pre]
        print(f"  {pre}: {len(s):,} | 시도 {s.시도명.nunique()} | 설치연도 중앙값 {s.yr.median():.0f} | 단속구분", s.kind.value_counts().head(3).to_dict(),
              "| 관리기관", s.관리기관명.value_counts().head(2).to_dict())
    m = cam.무인교통단속카메라관리번호.fillna("").str.extract(r"^([FGHJ])(\d{4})$")
    cam["p1"], cam["num"] = m[0], pd.to_numeric(m[1], errors="coerce")
    pos = {(a, b): i for i, a, b in zip(cam.index, cam.p1, cam.num) if isinstance(a, str)}
    near = []
    for i, a, b in zip(cam.index, cam.p1, cam.num):
        if isinstance(a, str) and (a, b + 1) in pos:
            k = pos[(a, b + 1)]
            if g.geometry.loc[i].distance(g.geometry.loc[k]) <= 100:
                near.append((i, k))
    rev = same = other = 0
    for i, k in near:
        x, y = (cam.A[i], cam.B[i]), (cam.A[k], cam.B[k])
        if pd.isna(x[0]) or pd.isna(y[0]):
            continue
        if x[0] == y[1] and x[1] == y[0]:
            rev += 1
        elif x == y:
            same += 1
        else:
            other += 1
    print(f"== 100m 안 붙은 관리번호 짝 {len(near):,} | 둘 다 화살표: 서로 뒤집힘 {rev} 같은 방향 {same} 지명 다름 {other}")


def main():
    global KEY
    KEY = env("VWORLD_API_KEY")
    cam = load()
    links = moct_links()
    text_counts(cam)
    updown_expressway(cam)
    arrow_sample(cam, links)
    osm_compare(cam, links)
    position_rules(cam, links)
    updown_urban(cam, links)
    pairs_and_ids(cam)


if __name__ == "__main__":
    main()

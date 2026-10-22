"""표준노드링크에 없는 골목과 골목 일방통행 방향을 다른 데이터로 확인한다.

- 방지턱 안내 지점이 놓인 도로명주소 전자지도 도로가 표준노드링크에 있는지
- 연속수치지형도 도로중심선 일방통행(ONSD)의 도로구분별 건수와 선을 그린 방향이 표준노드링크 링크 방향과 같은 비율
- 전국일방통행도로표준데이터의 규모와 시작점→종료점 방향이 표준노드링크 일방통행 링크 방향과 같은 수
- OpenStreetMap 일방통행 방향이 표준노드링크 일방통행 링크 방향과 같은 비율, 도로중심선 일방통행 도로에 OSM 일방통행이 있는 수

python -X utf8 scripts/nodelink/alley_data_check.py
"""
import glob
import sys
from pathlib import Path

import geopandas as gpd
import numpy as np
import pandas as pd
import pyogrio
import shapely
from shapely.geometry import LineString

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
from common import ROOT, read_csv
from nodelink import TM, WGS84, points, read_links

CENTERLINE = sorted(glob.glob(str(ROOT / "maps" / "도로중심선" / "*" / "*.shp")))
ADDRESS_ROADS = sorted(glob.glob(str(ROOT / "maps" / "도로명주소 전자지도" / "*" / "*" / "TL_SPRD_MANAGE.shp")))
OSM = ROOT / "maps" / "osm" / "south-korea-261020.osm.pbf"
RDDV = {"RDD000": "미분류", "RDD001": "고속국도", "RDD002": "일반국도", "RDD003": "지방도", "RDD008": "면리간도로", "RDD009": "소로"}


def moct_links():
    links = read_links(["LINK_ID", "ROAD_USE"])
    return links[links.ROAD_USE != "1"].reset_index(drop=True).set_crs(TM, allow_override=True)


def one_way_links(links):
    """LINK_ID ±100 대응링크가 없는 링크"""
    ids = set(links.LINK_ID.astype(np.int64))
    keep = [(int(i) + 100 not in ids) and (int(i) - 100 not in ids) for i in links.LINK_ID]
    return links[keep].reset_index(drop=True)


def local_dir(line, p):
    """선 위 p 근처 6m 구간의 진행 방향 단위 벡터"""
    at = line.project(p)
    a, b = line.interpolate(max(at - 3, 0)), line.interpolate(min(at + 3, line.length))
    v = np.array([b.x - a.x, b.y - a.y])
    n = np.hypot(*v)
    return v / n if n else v


def pair_direction(a, b, max_dist, min_len=20):
    """a 선마다 가운데 점에서 max_dist 안의 가장 가까운 b 선과 나란한(|cos| > 0.8) 쌍의 방향 cos"""
    a = a[a.geometry.length >= min_len]
    mid = gpd.GeoDataFrame(geometry=a.geometry.interpolate(0.5, normalized=True), crs=TM, index=a.index)
    j = gpd.sjoin_nearest(mid, b[["geometry"]], max_distance=max_dist, distance_col="d")
    j = j[~j.index.duplicated()]
    j["cos"] = [float(local_dir(a.geometry.loc[i], mid.geometry.loc[i]) @ local_dir(b.geometry.loc[k], mid.geometry.loc[i]))
                for i, k in zip(j.index, j.index_right)]
    return j[j.cos.abs() > 0.8]


def address_roads():
    """도로명주소 전자지도 도로구간(TM)"""
    roads = pd.concat([gpd.read_file(f, encoding="cp949", columns=["RN", "ROA_CLS_SE", "ROAD_BT"]) for f in ADDRESS_ROADS], ignore_index=True)
    return gpd.GeoDataFrame(roads, geometry="geometry", crs="EPSG:5179").to_crs(TM)


def guide_bumps():
    """방지턱 안내 지점(안내제외 N)"""
    bump = pd.read_csv(ROOT / "maps" / "과속방지턱정보" / "행안부" / "과속방지턱_지점.csv", dtype=str)
    bump = bump[bump.안내제외 == "N"].reset_index(drop=True)
    bump["위도"], bump["경도"] = bump.위도.astype(float), bump.경도.astype(float)
    return bump


def on_moct_road(g, links, roads):
    """점마다 30m 안 가장 가까운 전자지도 도로와, 그 도로 길이의 절반 이상이 표준노드링크 링크 15m 안에 있는지(in_moct)"""
    jr = gpd.sjoin_nearest(g, roads, distance_col="dr", max_distance=30)
    jr = jr[~jr.index.duplicated()]
    seg = roads.loc[jr.index_right.unique()]
    cand = gpd.sjoin(seg.assign(geometry=seg.buffer(15))[["geometry"]], links[["geometry"]], predicate="intersects")
    cover = {}
    for i, grp in cand.groupby(level=0):
        u = shapely.union_all(links.geometry.loc[grp.index_right].buffer(15).values)
        cover[i] = roads.geometry.loc[i].intersection(u).length / roads.geometry.loc[i].length
    jr["in_moct"] = jr.index_right.map(lambda i: cover.get(i, 0.0)) >= 0.5
    return jr


def bumps_on_moct_roads(links):
    roads = address_roads()
    bump = guide_bumps()
    g = points(bump, "위도", "경도")
    jl = gpd.sjoin_nearest(g, links[["geometry"]], distance_col="dl")
    dl = jl[~jl.index.duplicated()].dl
    jr = on_moct_road(g, links, roads)
    jr["dl"] = dl.loc[jr.index]
    cls = {"1": "고속도로", "2": "대로", "3": "로", "4": "길"}
    print(f"== 방지턱 안내 지점 {len(g):,} | 전자지도 도로구간 {len(roads):,}개 중 30m 안 도로가 있는 지점 {len(jr):,}")
    print(f"  놓인 도로가 표준노드링크에 있음(15m 안에 50% 이상) {jr.in_moct.sum():,} | 없음 {(~jr.in_moct).sum():,}")
    print("  도로위계별 (있음, 없음)", {cls.get(k, k): (int(v.in_moct.sum()), int((~v.in_moct).sum())) for k, v in jr.groupby("ROA_CLS_SE")})
    print(f"  도로폭 중앙값 있음 {jr[jr.in_moct].ROAD_BT.median()}m 없음 {jr[~jr.in_moct].ROAD_BT.median()}m")
    far, near = jr.dl > 30, jr.dl <= 10
    print(f"  표준노드링크에서 30m 넘는 지점 {far.sum():,} 중 도로가 표준노드링크에 없음 {(far & ~jr.in_moct).sum():,}")
    print(f"  표준노드링크 10m 안 지점 {near.sum():,} 중 도로가 표준노드링크에 없음 {(near & ~jr.in_moct).sum():,}")


def centerline_oneway(links):
    d = pd.concat([pyogrio.read_dataframe(f, columns=["RDDV", "ONSD"], read_geometry=False) for f in CENTERLINE], ignore_index=True)
    print(f"== 연속수치지형도 도로중심선 {len(d):,} | ONSD", d.ONSD.value_counts().to_dict())
    print(pd.crosstab(d.RDDV.map(RDDV), d.ONSD).to_string())
    c = pd.concat([pyogrio.read_dataframe(f, columns=["RDDV", "ONSD"], where="ONSD = 'ITH001'") for f in CENTERLINE], ignore_index=True)
    c = gpd.GeoDataFrame(c, geometry="geometry", crs="EPSG:5179").to_crs(TM).reset_index(drop=True)
    j = pair_direction(c, links, 5)
    j["RDDV"] = c.RDDV.loc[j.index]
    print(f"  일방통행 중심선 20m 이상 {(c.geometry.length >= 20).sum():,} | 5m 안 나란한 표준노드링크 링크와 짝 {len(j):,}")
    for k, g in j.groupby("RDDV"):
        if len(g) >= 50:
            print(f"    {RDDV[k]}: {len(g):,} | 그린 방향이 링크 방향과 같음 {(g.cos > 0).mean():.1%}")
    return c


def oneway_standard(links):
    d = read_csv(ROOT / "maps" / "일방도로" / "전국일방통행도로표준데이터.csv")
    print(f"== 전국일방통행도로표준데이터 {len(d):,}행 | 시도 {d.시도명.nunique()} | 시군구 {d.groupby(['시도명', '시군구명']).ngroups}")
    for c in ["시작점위도", "시작점경도", "종료점위도", "종료점경도"]:
        d[c] = pd.to_numeric(d[c], errors="coerce")
    same = (d.시작점위도 == d.종료점위도) & (d.시작점경도 == d.종료점경도)
    print(f"  시작점과 종료점이 같음 {same.sum()}")
    d = d[d.시작점위도.between(33, 39) & d.종료점위도.between(33, 39) & ~same]
    seg = gpd.GeoSeries([LineString([(a, b), (c, e)]) for a, b, c, e in zip(d.시작점경도, d.시작점위도, d.종료점경도, d.종료점위도)],
                        crs=WGS84, index=d.index).to_crs(TM)
    d = gpd.GeoDataFrame(d, geometry=seg, crs=TM)
    d = d[d.geometry.length >= 20]
    ow = set(one_way_links(links).LINK_ID)
    links = links.assign(one_way=links.LINK_ID.isin(ow))
    cand = gpd.sjoin(d[["geometry"]].assign(geometry=d.geometry.buffer(15)), links[["one_way", "geometry"]], predicate="intersects")
    same_dir = opposite = mixed = two_way = 0
    for i, g in cand.groupby(level=0):
        s = d.geometry.loc[i]
        sv = np.array(s.coords[-1]) - np.array(s.coords[0])
        sv /= np.hypot(*sv)
        hits = []
        for k in g.index_right:
            lg = links.geometry.iloc[k]
            if lg.intersection(s.buffer(15)).length < 0.5 * min(lg.length, s.length):
                continue
            lv = np.array(lg.coords[-1]) - np.array(lg.coords[0])
            n = np.hypot(*lv)
            if n and abs(sv @ (lv / n)) > 0.8:
                hits.append((float(sv @ (lv / n)), links.one_way.iloc[k]))
        if not hits:
            continue
        ow_hits = [h for h in hits if h[1]]
        if not ow_hits:
            two_way += 1
        elif all(h[0] > 0 for h in ow_hits):
            same_dir += 1
        elif all(h[0] < 0 for h in ow_hits):
            opposite += 1
        else:
            mixed += 1
    print(f"  나란한 표준노드링크 링크가 있는 도로 {same_dir + opposite + mixed + two_way}")
    print(f"    표준노드링크도 일방통행: 시작→종료가 링크 방향과 같음 {same_dir} 반대 {opposite} 섞임 {mixed} | 표준노드링크는 양방향 {two_way}")


def osm_oneway(links, centerline_oneway_lines):
    o = pyogrio.read_dataframe(OSM, layer="lines", where="highway IS NOT NULL", columns=["highway", "other_tags"])
    o["oneway"] = o.other_tags.fillna("").str.extract(r'"oneway"=>"([^"]*)"')[0]
    o = o[o.geometry.notna()].to_crs(TM).reset_index(drop=True)
    print(f"== OpenStreetMap 도로 {len(o):,}")
    kinds = ["motorway", "trunk", "primary", "secondary", "tertiary", "unclassified", "residential", "living_street", "service"]
    print(pd.crosstab(o.highway, o.oneway.fillna("(없음)")).loc[kinds].to_string())
    w = o[o.oneway.isin(["yes", "-1"])].copy()
    w.loc[w.oneway == "-1", "geometry"] = w.loc[w.oneway == "-1"].geometry.reverse()
    j = pair_direction(w, one_way_links(links), 5)
    j["highway"] = w.highway.loc[j.index]
    print(f"  OSM 일방통행과 5m 안 나란한 표준노드링크 일방통행 링크 짝 {len(j):,}")
    for k, g in j.groupby("highway"):
        if len(g) >= 150:
            print(f"    {k}: {len(g):,} | 방향 같음 {(g.cos > 0).mean():.1%}")
    c = centerline_oneway_lines
    c = c[c.RDDV.isin(["RDD000", "RDD008", "RDD009"])].reset_index(drop=True)
    j2 = pair_direction(c, o, 6)
    j2["oneway"] = o.oneway.loc[j2.index_right].values
    print(f"  도로중심선 일방통행(미분류·면리간도로·소로) 20m 이상 {(c.geometry.length >= 20).sum():,}"
          f" | 6m 안 나란한 OSM 도로 {len(j2):,} | 그중 OSM 일방통행 {j2.oneway.isin(['yes', '-1']).sum():,}")


def main():
    links = moct_links()
    bumps_on_moct_roads(links)
    c = centerline_oneway(links)
    oneway_standard(links)
    osm_oneway(links, c)


if __name__ == "__main__":
    main()

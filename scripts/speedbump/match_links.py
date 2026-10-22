"""과속방지턱 안내 지점을 표준노드링크 링크에 매칭한다.

입력
  행안부/과속방지턱_지점.csv           build_points.py가 만든 지점 파일 중 안내제외가 N인 지점
  도로명주소 전자지도 도로구간(TL_SPRD_MANAGE), 연속수치지형도 도로중심선, 표준노드링크 MOCT_LINK
규칙
  - 방지턱이 있는 전자지도 도로를 정한다. 방지턱 도로명에서 "~로", "~길"까지 남긴 이름과 같은 전자지도 도로가 50m 안에 있으면 그 도로이고,
    없으면 30m 안에서 가장 가까운 도로이다. 골목 안 주소 좌표에서 가장 가까운 도로가 옆 큰길인 경우가 있기 때문이다.
  - 같은 이름의 전자지도 도로가 5km 안에 있으나 좌표에서 200m보다 멀면 좌표가 도로명과 맞지 않으므로 제외한다.
    5km 안에 같은 이름의 도로가 없으면 판단하지 않는다.
    수영구에서 서로 다른 골목 이름의 방지턱 34곳이 큰길 링크 하나 위에 4m 간격으로 입력되어 있었다.
  - 그 도로 길이의 절반 이상이 표준노드링크 링크 15m 안에 있을 때만 매칭한다.
    표준노드링크에 없는 골목의 방지턱은 경로에 나오지 않으므로 제외한다(06 문서 구축대상 도로와 골목).
  - 30m 안에서 가장 가까운 링크에 매칭하고, 30m 안에 그 링크와 반대 방향(cos < -0.8)인 링크가 있으면 연장에 따라 그 링크에도 매칭한다.
    연장(과속방지턱연장)은 30 이하는 m, 100 이상은 cm로 보고, 그 사이 값과 0 이하는 알 수 없음으로 둔다. 지점에 행이 여러 개이면 가장 긴 값이다.
    도로폭은 연속수치지형도 도로중심선의 RVWD(도로경계석 외측 끝 기준)이며, 지점 30m 안에서 매칭한 링크와 나란한 중심선 중 가장 가까운 것의 값이다.
    왕복 2차로(가장 가까운 링크가 방향별 1차로) 도로는 반대 방향 링크에도 매칭한다. RVWD에는 갓길과 길가 주차 공간이 포함되어
    두 주행 차로를 모두 가로지르는 5m 정도의 방지턱도 도로폭보다 짧게 계산되기 때문이다.
    방향별 2차로 이상 도로는 연장이 도로폭의 60% 이상이거나 연장이나 도로폭을 알 수 없으면 반대 방향 링크에도 매칭하고,
    60%보다 짧으면 한쪽 방향만 막는 방지턱으로 보고 가장 가까운 링크에만 매칭한다.
  - 링크 시작점에서 지점을 링크에 투영한 위치까지의 거리(m)와, 그 거리를 링크 좌표 길이로 나눈 비율을 기록한다.
출력
  행안부/과속방지턱_링크.csv   방지턱 지점과 매칭한 링크를 한 행에 하나씩 기록한다. 지점번호는 과속방지턱_지점.csv 안내 지점의 순번이다.

python -X utf8 scripts/speedbump/match_links.py
"""
import glob
import re
import sys
from pathlib import Path

import geopandas as gpd
import numpy as np
import pandas as pd
import pyogrio
import shapely

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "nodelink"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
from alley_data_check import address_roads, guide_bumps, local_dir, moct_links
from common import SPEEDBUMP_DIR, read_csv, write_csv
from nodelink import points, read_links

OUT = SPEEDBUMP_DIR / "행안부" / "과속방지턱_링크.csv"
MAX_DIST = 30
COLS = ["지점번호", "위도", "경도", "시도명", "시군구명", "도로명", "연속형", "연장m", "도로폭m", "LINK_ID", "링크시작거리", "링크비율", "링크까지거리", "반대방향"]
FULL_RATIO = 0.6
CENTERLINE = sorted(glob.glob(str(Path(__file__).resolve().parents[2] / "maps" / "도로중심선" / "*" / "*.shp")))


def bump_lengths(bump):
    """지점마다 연장(m). 30 이하는 m, 100 이상은 cm로 보고 지점의 행 중 가장 긴 값. 알 수 없으면 NaN"""
    raw = read_csv(SPEEDBUMP_DIR / "행안부" / "과속방지턱정보.csv")
    v = pd.to_numeric(raw.과속방지턱연장, errors="coerce")
    m = np.where((v > 0) & (v <= 30), v, np.where(v >= 100, v / 100, np.nan))
    by_id = dict(zip(raw.관리번호, m))
    out = []
    for ids in bump.관리번호목록.fillna(""):
        vals = [by_id.get(k, np.nan) for k in ids.split(";")]
        vals = [x for x in vals if not np.isnan(x)]
        out.append(max(vals) if vals else np.nan)
    return pd.Series(out, index=bump.index)


def centerlines(g):
    """지점들이 있는 범위의 연속수치지형도 도로중심선(TM)과 도로폭 RVWD"""
    b = g.to_crs("EPSG:5179").total_bounds
    box = (b[0] - 50, b[1] - 50, b[2] + 50, b[3] + 50)
    c = pd.concat([pyogrio.read_dataframe(f, columns=["RVWD"], bbox=box) for f in CENTERLINE], ignore_index=True)
    return gpd.GeoDataFrame(c, geometry="geometry", crs="EPSG:5179").to_crs(g.crs)


def road_width(cl, p, v):
    """점 p 30m 안에서 방향 v와 나란한(|cos| > 0.7) 도로중심선 중 가장 가까운 것의 RVWD. 없으면 NaN"""
    near = cl.iloc[cl.sindex.query(p.buffer(MAX_DIST), predicate="intersects")]
    if near.empty:
        return np.nan
    near = near.assign(d=near.distance(p)).sort_values("d")
    for _, row in near.iterrows():
        if row.RVWD > 0 and abs(float(local_dir(row.geometry, p) @ v)) > 0.7:
            return float(row.RVWD)
    return np.nan


def road_name(x):
    """도로명 칸에서 "~로", "~로N길", "~로N번길"까지만 남긴다. 없으면 빈 문자열"""
    m = re.match(r"^(.*?(?:로|길)(?:\d+(?:번)?(?:가|나|다|라|마|바|사|아)?길)?)", re.sub(r"\s", "", str(x)))
    return m.group(1) if m else ""


def far_from_name(g, names, roads, limit=200, window=5000):
    """window m 안에 같은 이름의 전자지도 도로가 있지만 모두 limit m 넘게 떨어진 지점"""
    rn = roads.RN.fillna("").str.replace(r"\s", "", regex=True)
    by_name = {n: grp.geometry.values for n, grp in roads.assign(n=rn).groupby("n")}
    far = []
    for i, n in names.items():
        if n and n in by_name:
            d = shapely.distance(g.geometry.loc[i], by_name[n]).min()
            if limit < d <= window:
                far.append(i)
    return pd.Index(far)


def place_roads(g, names, roads):
    """지점마다 놓인 전자지도 도로 번호. 같은 이름 도로가 50m 안에 있으면 그 도로, 없으면 30m 안 가장 가까운 도로"""
    rn = roads.RN.fillna("").str.replace(r"\s", "", regex=True)
    near = gpd.sjoin(g[["geometry"]].assign(geometry=g.buffer(50)), roads[["geometry"]], predicate="intersects")
    near["d"] = shapely.distance(g.geometry.loc[near.index].values, roads.geometry.loc[near.index_right].values)
    near["same"] = rn.loc[near.index_right].values == names.loc[near.index].values
    near = near.sort_values("d")
    same = near[near.same & (names.loc[near.index] != "").values]
    by_name = same[~same.index.duplicated()].index_right
    nearest = near[near.d <= 30]
    nearest = nearest[~nearest.index.duplicated()].index_right
    return by_name.combine_first(nearest), by_name.index


def covered_roads(road_ids, roads, links):
    """도로 길이의 절반 이상이 표준노드링크 링크 15m 안에 있는 전자지도 도로 번호"""
    seg = roads.loc[road_ids]
    cand = gpd.sjoin(seg.assign(geometry=seg.buffer(15))[["geometry"]], links[["geometry"]], predicate="intersects")
    ok = set()
    for i, grp in cand.groupby(level=0):
        u = shapely.union_all(links.geometry.loc[grp.index_right].buffer(15).values)
        if roads.geometry.loc[i].intersection(u).length >= 0.5 * roads.geometry.loc[i].length:
            ok.add(i)
    return ok


def main():
    links = moct_links()
    lanes = read_links(["LINK_ID", "LANES"], geometry=False).set_index("LINK_ID").LANES
    roads = address_roads()
    bump = guide_bumps()
    bump["연장m"] = bump_lengths(bump)
    g = points(bump, "위도", "경도")
    names = bump.도로명.map(road_name)
    far = far_from_name(g, names, roads)
    road, named = place_roads(g, names, roads)
    road = road.drop(far, errors="ignore")
    ok = covered_roads(road.unique(), roads, links)
    keep = road.index[road.isin(ok)]
    print(f"방지턱 안내 지점 {len(bump):,}곳 | 좌표가 도로명과 맞지 않아 제외 {len(far):,}곳"
          f" | 있는 도로를 전자지도에서 찾음 {len(road):,}곳 (도로명으로 찾음 {len(named):,}곳) | 그 도로가 표준노드링크에 있음 {len(keep):,}곳")

    cl = centerlines(g.loc[keep])
    rows = []
    widths = {}
    no_link = one_side = 0
    for i in keep:
        p = g.geometry.loc[i]
        near = links.iloc[links.sindex.query(p.buffer(MAX_DIST), predicate="intersects")]
        near = near.assign(d=near.distance(p))
        near = near[near.d <= MAX_DIST].sort_values("d")
        if near.empty:
            no_link += 1
            continue
        first = near.iloc[0]
        v = local_dir(first.geometry, p)
        picked = [(first, False)]
        for _, row in near.iloc[1:].iterrows():
            if float(local_dir(row.geometry, p) @ v) < -0.8:
                width = road_width(cl, p, v)
                widths[i] = width
                length = bump.연장m.loc[i]
                two_lane = lanes.get(first.LINK_ID, 1) <= 1
                if two_lane or np.isnan(length) or np.isnan(width) or length >= FULL_RATIO * width:
                    picked.append((row, True))
                else:
                    one_side += 1
                break
        b = bump.loc[i]
        for row, opposite in picked:
            at = row.geometry.project(p)
            rows.append([i, b.위도, b.경도, b.시도명, b.시군구명, b.도로명, b.연속형, b.연장m, widths.get(i, np.nan), row.LINK_ID,
                         round(at, 2), round(at / row.geometry.length, 6) if row.geometry.length else 0.0, round(row.d, 2), "Y" if opposite else "N"])
    out = pd.DataFrame(rows, columns=COLS)
    write_csv(OUT, out, COLS)
    per = out.groupby("지점번호").size()
    u = out.drop_duplicates("지점번호")
    has_opp = u.지점번호.isin(widths.keys())
    print(f"매칭한 방지턱 {out.지점번호.nunique():,}곳 | 30m 안에 링크가 없어 제외 {no_link:,}곳 | 저장한 행 {len(out):,}")
    print(f"  양방향 링크에 매칭 {(per == 2).sum():,}곳 | 한 방향 링크에만 매칭 {(per == 1).sum():,}곳")
    print(f"    한 방향만 매칭한 이유: 30m 안에 반대 방향 링크가 없음 {(per == 1).sum() - one_side:,}곳"
          f" | 방향별 2차로 이상 도로에서 연장이 도로폭의 60%보다 짧음 {one_side:,}곳")
    print(f"  반대 방향 링크가 있는 방지턱 {has_opp.sum():,}곳 중 연장을 알 수 없음 {u[has_opp].연장m.isna().sum():,}곳"
          f" | 도로폭을 알 수 없음 {u[has_opp].도로폭m.isna().sum():,}곳")
    print("  방지턱에서 가장 가까운 링크까지 거리 분위 50% 90% 99%", np.percentile(out[out.반대방향 == "N"].링크까지거리, [50, 90, 99]).round(1))
    print(f"  방지턱이 매칭된 링크 {out.LINK_ID.nunique():,}개 | 링크 하나에 매칭된 방지턱 최대 {out.groupby('LINK_ID').size().max()}곳")
    w = u[has_opp].dropna(subset=["연장m", "도로폭m"]).copy()
    w["비"] = w.연장m / w.도로폭m
    w["방향별차로"] = lanes.reindex(w.LINK_ID).values
    print(f"  반대 방향 링크가 있고 연장·도로폭을 아는 방지턱 {len(w):,}곳의 연장 ÷ 도로폭")
    for lo, hi in [(0, 6), (6, 9), (9, 15), (15, 100)]:
        x = w[(w.도로폭m > lo) & (w.도로폭m <= hi)]
        print(f"    도로폭 {lo}~{hi}m {len(x):,}곳: 중앙값 {x.비.median():.2f} | 25% {x.비.quantile(.25):.2f} 75% {x.비.quantile(.75):.2f}")
    short = w[w.비 < FULL_RATIO]
    print(f"    60%보다 짧은 {len(short):,}곳 중 방향별 1차로 {(short.방향별차로 <= 1).sum():,}곳"
          f" | 방향별 1차로 도로폭 중앙값 {short[short.방향별차로 <= 1].도로폭m.median()}m 연장 중앙값 {short[short.방향별차로 <= 1].연장m.median()}m")
    print(f"저장 {OUT}")


if __name__ == "__main__":
    main()

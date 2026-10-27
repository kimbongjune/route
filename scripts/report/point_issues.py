"""단속카메라(경찰청 표준데이터)와 과속방지턱(행정안전부) 중 표준노드링크 링크에 매칭하지 못한 지점의 목록을 만든다.

매칭 규칙은 scripts/camera/match_links.py, scripts/speedbump/match_links.py와 같다.
- 과속방지턱_좌표도로명불일치: 5km 안에 같은 이름의 전자지도 도로가 있지만 좌표에서 200m보다 먼 방지턱(도로명과 좌표가 맞지 않음).
  좌표 15m 안 가장 가까운 도로가 그 도로명의 번길(은여울로 → 은여울로7번길)이면 "도로명에 번길이 빠짐", 아니면 "좌표가 같은 이름 도로에서 멂"
- 과속방지턱_30m링크없음, 단속카메라_30m링크없음: 있는 도로의 절반 이상이 표준노드링크 링크 15m 안에 있는데 지점 30m 안에 링크가 없는 것
각 행에 가장 가까운 표준노드링크 링크와 거리, 가장 가까운 전자지도 도로명과 거리를 함께 적는다.
출력은 docs/15-데이터-문제/<제공처>/ 아래 CSV(UTF-8 BOM)이다

python -X utf8 scripts/report/point_issues.py
"""
import importlib.util
import sys
from pathlib import Path

import geopandas as gpd
import pandas as pd
import shapely

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "nodelink"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
from alley_data_check import address_roads, guide_bumps, moct_links
from common import ROOT
from nodelink import WGS84, points

OUT = ROOT / "docs" / "15-데이터-문제"
MAX_DIST = 30


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


bump_mod = load_module("bump_match", Path(__file__).resolve().parents[1] / "speedbump" / "match_links.py")
sys.modules["match_links"] = bump_mod
cam_mod = load_module("camera_match", Path(__file__).resolve().parents[1] / "camera" / "match_links.py")


def save(df, folder, name):
    d = OUT / folder
    d.mkdir(parents=True, exist_ok=True)
    df.to_csv(d / f"{name}.csv", index=False, encoding="utf-8-sig")
    print(f"{folder}/{name}: {len(df):,}행")


def nearest_info(g, idx, links, roads):
    """지점마다 가장 가까운 링크ID·거리, 가장 가까운 전자지도 도로명·거리"""
    pts = g.loc[idx, ["geometry"]]
    jl = gpd.sjoin_nearest(pts, links[["LINK_ID", "geometry"]], distance_col="d")
    jl = jl[~jl.index.duplicated()]
    jr = gpd.sjoin_nearest(pts, roads[["RN", "geometry"]], distance_col="d")
    jr = jr[~jr.index.duplicated()]
    ll = pts.to_crs(WGS84).geometry
    return pd.DataFrame({"가장가까운_LINK_ID": jl.LINK_ID, "링크까지_m": jl.d.round(1),
                         "가장가까운_전자지도도로": jr.RN, "전자지도도로까지_m": jr.d.round(1),
                         "위도": ll.y.round(7), "경도": ll.x.round(7)}, index=pts.index)


def no_link_within(g, idx, links):
    out = []
    for i in idx:
        p = g.geometry.loc[i]
        near = links.iloc[links.sindex.query(p.buffer(MAX_DIST), predicate="intersects")]
        if near.empty or near.distance(p).min() > MAX_DIST:
            out.append(i)
    return pd.Index(out)


def main():
    links = moct_links()
    roads = address_roads()

    bump = guide_bumps()
    g = points(bump, "위도", "경도")
    names = bump.도로명.map(bump_mod.road_name)
    far = bump_mod.far_from_name(g, names, roads)
    rn = roads.RN.fillna("").str.replace(r"\s", "", regex=True)
    same_d = []
    by_name = {n: grp.geometry.values for n, grp in roads.assign(n=rn).groupby("n")}
    for i in far:
        same_d.append(round(float(shapely.distance(g.geometry.loc[i], by_name[names.loc[i]]).min()), 1))
    info = nearest_info(g, far, links, roads)
    # 가장 가까운 전자지도 도로(15m 안)가 도로명으로 시작하는 번길이면 도로명 칸에 번길이 빠진 것으로 본다
    nearest_rn = info["가장가까운_전자지도도로"].fillna("").str.replace(r"\s", "", regex=True)
    kind = ["도로명에 번길이 빠짐" if names.loc[i] and r.startswith(names.loc[i]) and r != names.loc[i] and d <= 15 else "좌표가 같은 이름 도로에서 멂"
            for i, r, d in zip(far, nearest_rn, info["전자지도도로까지_m"])]
    out = pd.concat([bump.loc[far, ["시도명", "시군구명", "도로명"]], pd.Series(kind, index=far, name="구분"),
                     pd.Series(same_d, index=far, name="같은이름도로까지_m"), info], axis=1)
    save(out, "행정안전부_과속방지턱", "좌표도로명불일치")
    print("  ", out["구분"].value_counts().to_dict())

    road, _ = bump_mod.place_roads(g, names, roads)
    road = road.drop(far, errors="ignore")
    ok = bump_mod.covered_roads(road.unique(), roads, links)
    keep = road.index[road.isin(ok)]
    miss = no_link_within(g, keep, links)
    save(pd.concat([bump.loc[miss, ["시도명", "시군구명", "도로명"]], nearest_info(g, miss, links, roads)], axis=1),
         "행정안전부_과속방지턱", "30m링크없음")

    cam = cam_mod.load()
    cg = points(cam, "위도", "경도")
    croad, _ = cam_mod.place_roads(cg, cam.도로노선명.map(cam_mod.road_name), roads)
    cok = cam_mod.covered_roads(croad.unique(), roads, links)
    on = croad.index[croad.isin(cok)]
    keep = on.difference(cam_mod.duplicates(cam, cg))
    miss = no_link_within(cg, keep, links)
    cols = ["무인교통단속카메라관리번호", "시도명", "시군구명", "관리기관명", "도로노선명", "설치장소", "단속구분", "제한속도"]
    cols = [c for c in cols if c in cam.columns]
    save(pd.concat([pd.Series(miss, index=miss, name="카메라번호"), cam.loc[miss, cols], nearest_info(cg, miss, links, roads)], axis=1),
         "경찰청_단속카메라", "30m링크없음")


if __name__ == "__main__":
    main()

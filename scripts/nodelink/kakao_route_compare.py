"""route_cli 경로를 카카오모빌리티 길찾기(거리 우선)와 비교한다.

- 구간: 서울역→강남역, 서울역→부산역, 고정 시드로 고른 일반도로 링크 가운데 점 쌍(직선 3~40km) 20개
- 거리, routedb 경로 중 카카오 경로 30m 안에 있는 길이 비율, 카카오 경로 중 routedb 경로 30m 안에 있는 길이 비율
- 카카오 경로 중 표준노드링크 링크로 덮이는 길이 비율. 카카오 경로 20m 안에 완전히 들어가는 링크를 15m 두께로 덮어 잰다
- routedb가 5% 넘게 긴 구간은 routedb 출발·도착 스냅 링크와 표준노드링크로 덮이지 않는 카카오 도로(100m 넘게)
- routedb가 5% 넘게 짧은 구간은 카카오 경로 30m 밖에 있는 routedb 링크의 도로명별 길이와 도로등급, 1차로 길이

먼저 cargo build --release -p importer-knl --example route_cli
python -X utf8 scripts/nodelink/kakao_route_compare.py
"""
import json
import subprocess
import sys
import urllib.request
from pathlib import Path

import geopandas as gpd
import numpy as np
from shapely.geometry import LineString
from shapely.ops import linemerge, unary_union

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import env
from nodelink import TM, WGS84, read_links, tm_to_wgs84

ROOT = Path(__file__).resolve().parents[2]
ROUTE_CLI = ROOT / "rust" / "target" / "release" / "examples" / "route_cli.exe"
BUFFER_M = 30
PAIRS = 20


def kakao(key, a, b):
    """카카오 거리 우선 경로의 (거리 m, TM 선, 도로별 (이름, TM 선))"""
    url = (f"https://apis-navi.kakaomobility.com/v1/directions?origin={a[1]},{a[0]}&destination={b[1]},{b[0]}"
           "&priority=DISTANCE&summary=false")
    req = urllib.request.Request(url, headers={"Authorization": "KakaoAK " + key})
    route = json.load(urllib.request.urlopen(req, timeout=30))["routes"][0]
    if route["result_code"] != 0:
        return None
    roads = [rd for sec in route["sections"] for rd in sec["roads"]]
    to_tm = lambda xy: gpd.GeoSeries([LineString(xy)], crs=WGS84).to_crs(TM).iloc[0]
    line = to_tm([v for rd in roads for v in zip(rd["vertexes"][0::2], rd["vertexes"][1::2])])
    parts = [(rd["name"] or "-", to_tm(list(zip(rd["vertexes"][0::2], rd["vertexes"][1::2])))) for rd in roads if len(rd["vertexes"]) >= 4]
    return route["summary"]["distance"], line, parts


def routedb(links, a, b):
    """route_cli 경로의 (거리 m, LINK_ID 목록, TM 선, 출발·도착 스냅 줄)"""
    out = subprocess.run([str(ROUTE_CLI), *map(str, (*a, *b))], capture_output=True, text=True, encoding="utf-8").stdout.splitlines()
    if len(out) < 4:
        return None
    dist = float(out[2].split()[1].rstrip("m"))
    ids = out[3].split()
    line = unary_union(list(links.loc[ids].geometry))
    return dist, ids, line, out[1]


def covered(links, line):
    """line 20m 안에 완전히 들어가는 링크를 15m 두께로 덮었을 때 line 중 덮이는 부분"""
    near = links[links.within(line.buffer(20))]
    return unary_union(list(near.geometry.buffer(15))) if len(near) else None


def pairs(links):
    rng = np.random.default_rng(7)
    road = links[links.ROAD_RANK.isin(["103", "104", "105", "106"])]
    mid = road.geometry.interpolate(0.5, normalized=True)
    out = []
    while len(out) < PAIRS:
        i, j = rng.integers(len(road), size=2)
        if 3000 <= mid.iloc[i].distance(mid.iloc[j]) <= 40000:
            out.append((tm_to_wgs84(mid.iloc[i].x, mid.iloc[i].y), tm_to_wgs84(mid.iloc[j].x, mid.iloc[j].y)))
    return out


def main():
    key = env("KAKAO_REST_API_KEY")
    links = read_links(["LINK_ID", "ROAD_NAME", "ROAD_RANK", "LANES", "LENGTH", "ROAD_USE"])
    links = links[links.ROAD_USE != "1"]
    cases = [("서울역→강남역", (37.5547, 126.9707), (37.4979, 127.0276)),
             ("서울역→부산역", (37.5547, 126.9707), (35.1152, 129.0422))]
    cases += [(f"무작위 {k + 1}", a, b) for k, (a, b) in enumerate(pairs(links))]
    by_id = links.set_index("LINK_ID")

    rows = []
    for name, a, b in cases:
        k, o = kakao(key, a, b), routedb(by_id, a, b)
        if k is None or o is None:
            print(name, "카카오 경로 없음" if k is None else "routedb 경로 없음")
            continue
        (kd, kl, kparts), (od, ids, ol, snap) = k, o
        in_k = ol.intersection(kl.buffer(BUFFER_M)).length / ol.length
        in_o = kl.intersection(ol.buffer(BUFFER_M)).length / kl.length
        cover = covered(links, kl)
        in_moct = kl.intersection(cover).length / kl.length if cover is not None else 0
        diff = (od - kd) / kd
        rows.append(diff)
        print(f"{name} ({a[0]:.5f}, {a[1]:.5f}) → ({b[0]:.5f}, {b[1]:.5f}) | 카카오 {kd:,}m routedb {od:,.0f}m 차이 {diff:+.1%}"
              f" | routedb 경로 중 카카오 30m 안 {in_k:.0%} 카카오 경로 중 routedb 30m 안 {in_o:.0%} | 카카오 경로 중 표준노드링크로 덮임 {in_moct:.0%}")
        if diff > 0.05:
            print("    routedb", snap)
            gaps = []
            for rname, rl in kparts:
                c = covered(links, rl)
                miss = rl.length - (rl.intersection(c).length if c is not None else 0)
                if miss > 100:
                    gaps.append(f"{rname} {rl.length:,.0f}m 중 {miss:,.0f}m")
            print("    표준노드링크로 덮이지 않는 카카오 도로", gaps)
        if diff < -0.05:
            used = by_id.loc[ids]
            off = used[~used.geometry.within(kl.buffer(BUFFER_M))]
            print(f"    카카오 경로 30m 밖 routedb 링크 {off.LENGTH.sum():,.0f}m | 도로등급별", off.groupby("ROAD_RANK").LENGTH.sum().round().to_dict(),
                  f"| 1차로 {off[off.LANES <= 1].LENGTH.sum():,.0f}m | 도로명별", off.groupby("ROAD_NAME", sort=False).LENGTH.sum().round().to_dict())
    r = np.array(rows)
    print(f"{len(r)}구간 거리 차이 중앙값 {np.median(r):+.1%} 평균 {r.mean():+.1%} | 5% 넘게 김 {(r > 0.05).sum()} 5% 넘게 짧음 {(r < -0.05).sum()}")


if __name__ == "__main__":
    main()

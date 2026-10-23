"""route_cli 안내 지점을 카카오모빌리티 길찾기(거리 우선)의 안내(guides)와 비교한다. 카카오 응답은 저장하지 않는다.

- 구간: 서울역→강남역, 서울역→부산역, kakao_route_compare.py와 같은 고정 시드의 무작위 20쌍
- 두 경로가 같이 지나는 지점만 비교한다. 안내 지점 앞뒤 80m의 카카오 경로가 routedb 경로 30m 안에 있어야 한다
- 카카오 회전 안내(좌·우회전, 좌·우측 방향, 시 방향, 고속도로 출·입구)마다 40m 안에 routedb 안내가 있는지, 좌·우가 같은지
- routedb 회전 안내(직진 제외)마다 40m 안에 카카오 안내가 있는지
- 12시 방향, 톨게이트, 고가·지하차도 진입 같은 카카오 정보 안내는 회전 안내로 세지 않는다

먼저 cargo build --release -p importer-knl --example route_cli
python -X utf8 scripts/nodelink/kakao_guide_compare.py
"""
import json
import subprocess
import sys
import urllib.request
from pathlib import Path

import geopandas as gpd
from shapely.geometry import LineString, Point
from shapely.ops import substring, unary_union

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import env
from nodelink import TM, WGS84, read_links, wgs84_to_tm
import kakao_route_compare as krc

ROOT = Path(__file__).resolve().parents[2]
ROUTE_CLI = ROOT / "rust" / "target" / "release" / "examples" / "route_cli.exe"
NEAR = 30.0
MATCH = 40.0
# 카카오 안내 종류. 왼쪽·오른쪽으로 나뉘는 것과 회전 안내로 세지 않는 정보 안내
LEFT = {1, 3, 5, 8, 11, 82, 43, 46, 48} | set(range(24, 29)) | set(range(36, 41)) | set(range(77, 82))
RIGHT = {2, 6, 9, 12, 83, 44, 47, 49} | set(range(18, 23)) | set(range(30, 35)) | set(range(70, 75))
INFO = {0, 29, 84, 85, 86, 100, 101, 14, 15, 16, 17}
OUR_LEFT = {"Left", "SlightLeft", "ExitLeft", "UTurn"}
OUR_RIGHT = {"Right", "SlightRight", "ExitRight"}


def kakao(key, a, b):
    """카카오 거리 우선 경로의 (거리 m, TM 선, 안내 (TM 점, 종류, 문구, 이름) 목록)"""
    url = (f"https://apis-navi.kakaomobility.com/v1/directions?origin={a[1]},{a[0]}&destination={b[1]},{b[0]}"
           "&priority=DISTANCE&summary=false")
    req = urllib.request.Request(url, headers={"Authorization": "KakaoAK " + key})
    route = json.load(urllib.request.urlopen(req, timeout=30))["routes"][0]
    if route["result_code"] != 0:
        return None
    roads = [rd for sec in route["sections"] for rd in sec["roads"]]
    xy = [v for rd in roads for v in zip(rd["vertexes"][0::2], rd["vertexes"][1::2])]
    line = gpd.GeoSeries([LineString(xy)], crs=WGS84).to_crs(TM).iloc[0]
    guides = []
    for sec in route["sections"]:
        for g in sec["guides"]:
            if g["type"] in (100, 101):
                continue
            guides.append((Point(*wgs84_to_tm(g["y"], g["x"])), g["type"], g["guidance"], g["name"]))
    return route["summary"]["distance"], line, guides


def routedb(links, a, b):
    """route_cli 거리 경로의 (거리 m, TM 선, 안내 지점 (TM 점, 회전, 출력 줄) 목록)"""
    out = subprocess.run([str(ROUTE_CLI), *map(str, (*a, *b)), "distance"], capture_output=True, text=True,
                         encoding="utf-8").stdout.splitlines()
    ids = next((l for l in out if len(l.split()) > 3 and all(t.isdigit() for t in l.split())), None)
    if ids is None:
        return None
    dist = float(next(l for l in out if l.startswith("거리")).split()[1].rstrip("m"))
    line = unary_union(list(links.loc[ids.split()].geometry))
    steps = []
    on = False
    for l in out:
        if l.startswith("안내 지점"):
            on = True
            continue
        if l.startswith("도로명 구간"):
            break
        if on:
            t = l.strip()
            lat, lon = t[t.rfind("(") + 1:-1].split(", ")
            steps.append((Point(*wgs84_to_tm(float(lat), float(lon))), t.split()[1], t[: t.rfind("(")].strip()))
    return dist, line, steps


def side(turn):
    return "L" if turn in OUR_LEFT else "R" if turn in OUR_RIGHT else "S"


def kakao_side(t):
    return "L" if t in LEFT else "R" if t in RIGHT else "S"


def main():
    key = env("KAKAO_REST_API_KEY")
    links = read_links(["LINK_ID", "ROAD_RANK"]).set_index("LINK_ID")
    pairs = [("서울역→강남역", (37.5547, 126.9707), (37.4979, 127.0276)),
             ("서울역→부산역", (37.5547, 126.9707), (35.1151, 129.0422))]
    pairs += [(f"무작위 {i + 1}", a, b) for i, (a, b) in enumerate(krc.pairs(links.reset_index()))]
    total = dict(kakao_shared=0, kakao_matched=0, kakao_same_side=0, ours_shared=0, ours_matched=0)
    for name, a, b in pairs:
        k = kakao(key, a, b)
        o = routedb(links, a, b)
        if k is None or o is None:
            print(name, "경로 없음")
            continue
        kd, kline, kguides = k
        od, oline, osteps = o

        def shared(pt):
            if pt.distance(kline) > NEAR or pt.distance(oline) > NEAR:
                return False
            d = kline.project(pt)
            piece = substring(kline, max(0.0, d - 80.0), min(kline.length, d + 80.0))
            return all(Point(c).distance(oline) <= NEAR for c in piece.coords)

        k_all = [g for g in kguides if shared(g[0])]
        k_shared = [g for g in k_all if g[1] not in INFO]
        o_shared = [s for s in osteps if s[1] != "Straight" and shared(s[0])]
        k_matched = k_same = 0
        notes = []
        for g in k_shared:
            near = [s for s in o_shared if s[0].distance(g[0]) <= MATCH]
            if near:
                k_matched += 1
                if any(side(s[1]) == kakao_side(g[1]) for s in near):
                    k_same += 1
                else:
                    notes.append(f"방향 다름 카카오 {g[2]}({g[3]}) ↔ {near[0][2]}")
            else:
                notes.append(f"routedb 안내 없음 카카오 {g[2]}({g[3]})")
        o_matched = 0
        for s in o_shared:
            if any(s[0].distance(g[0]) <= MATCH for g in k_shared):
                o_matched += 1
            else:
                near = [g for g in k_all if s[0].distance(g[0]) <= MATCH]
                notes.append(f"카카오 {near[0][2] if near else '안내 없음'} ↔ {s[2]}")
        print(f"== {name} 카카오 {kd}m routedb {od:.0f}m | 카카오 회전 안내 {len(kguides)} 겹침 {len(k_shared)} "
              f"routedb 안내 있음 {k_matched} 방향 같음 {k_same} | routedb 회전 안내 "
              f"{len([s for s in osteps if s[1] != 'Straight'])} 겹침 {len(o_shared)} 카카오 안내 있음 {o_matched}")
        for n in notes:
            print("   ", n)
        total["kakao_shared"] += len(k_shared)
        total["kakao_matched"] += k_matched
        total["kakao_same_side"] += k_same
        total["ours_shared"] += len(o_shared)
        total["ours_matched"] += o_matched
    print("합계", total)


if __name__ == "__main__":
    main()

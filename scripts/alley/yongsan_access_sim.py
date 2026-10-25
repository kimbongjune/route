"""골목 허용 방식을 비교한다: 골목 없음 / 골목 전체 허용 / 출발·도착 반경 안에서만 허용(접근 전용).

- 용산구 골목 CSV(maps/골목/용산)와 그 주변 3km 표준노드링크로 작은 그래프를 만들고 파이썬 다익스트라로 최소시간 경로를 구한다
- 시간은 길이 / MAX_SPD이다(회전 비용은 넣지 않는다). 골목 속도는 임포터와 같은 ALLEY_SPD이다
- 접근 전용은 골목 링크의 중점이 출발이나 도착에서 RADIUS 안에 있을 때만 그 링크를 지나게 한다
- yongsan_compare.py의 10쌍으로 방식마다 거리, 시간, 골목 링크 수, 카카오(최소시간) 경로와의 겹침을 출력한다. 카카오 응답은 저장하지 않는다

먼저 python -X utf8 scripts/alley/extract_alleys.py 11170 으로 maps/골목/용산을 만든다.
importer-alley는 maps/골목 아래 폴더를 모두 읽으므로 실험 뒤에는 maps/골목/용산을 지운다(서울특별시와 겹친다)

python -X utf8 scripts/alley/yongsan_access_sim.py [로컬 주소]
"""
import heapq
import math
import sys
from collections import defaultdict
from pathlib import Path

import geopandas as gpd
import numpy as np
import pandas as pd
from shapely.geometry import LineString
from shapely.ops import substring

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "nodelink"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
import kakao_time_compare as k  # noqa: E402
from nodelink import NODE_DIR, TM, WGS84, wgs84_to_tm  # noqa: E402
from yongsan_compare import PAIRS  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
ALLEY_DIR = ROOT / "maps" / "골목" / "용산"
ALLEY_SPD = 10
MARGIN_M = 3000
RADII = [500, 1000]


def load_graph():
    nodes = pd.read_csv(ALLEY_DIR / "용산_골목_노드.csv", encoding="utf-8-sig")
    alleys = pd.read_csv(ALLEY_DIR / "용산_골목_링크.csv", encoding="utf-8-sig")
    xy = np.array([wgs84_to_tm(lat, lon) for lat, lon in zip(nodes.lat, nodes.lon)])
    coord = {int(n): (x, y) for n, (x, y) in zip(nodes.node_id, xy)}
    minx, miny, maxx, maxy = xy[:, 0].min() - MARGIN_M, xy[:, 1].min() - MARGIN_M, xy[:, 0].max() + MARGIN_M, xy[:, 1].max() + MARGIN_M
    links = gpd.read_file(NODE_DIR / "MOCT_LINK.shp", encoding="cp949", engine="pyogrio",
                          columns=["LINK_ID", "F_NODE", "T_NODE", "MAX_SPD"], bbox=(minx, miny, maxx, maxy))
    # 골목이 닿는 표준노드링크 링크를 비율 위치에서 나눈다(임포터 alley.rs와 같은 규칙)
    cuts = defaultdict(list)
    for r in nodes.itertuples():
        if pd.isna(r.at):
            continue
        if not pd.isna(r.on_link):
            cuts[int(r.on_link)].append((r.at, int(r.node_id)))
        if not pd.isna(r.on_link_rev):
            cuts[int(r.on_link_rev)].append((1 - r.at, int(r.node_id)))
    edges = []  # (from, to, length, speed, is_alley, coords)
    for r in links.itertuples():
        geom, f, t = r.geometry, int(r.F_NODE), int(r.T_NODE)
        coord.setdefault(f, geom.coords[0])
        coord.setdefault(t, geom.coords[-1])
        stops = [(0.0, f)] + sorted((a, n) for a, n in cuts.get(int(r.LINK_ID), []) if 1e-6 < a < 1 - 1e-6) + [(1.0, t)]
        for (a0, n0), (a1, n1) in zip(stops, stops[1:]):
            part = substring(geom, a0, a1, normalized=True)
            edges.append((n0, n1, part.length, int(r.MAX_SPD), False, list(part.coords)))
    for r in alleys.itertuples():
        pts = [float(v) for v in r.wkt.split()]
        line = gpd.GeoSeries([LineString(list(zip(pts[0::2], pts[1::2])))], crs=WGS84).to_crs(TM).iloc[0]
        c = list(line.coords)
        edges.append((int(r._2), int(r.to), line.length, ALLEY_SPD, True, c))
        edges.append((int(r.to), int(r._2), line.length, ALLEY_SPD, True, c[::-1]))
    adj = defaultdict(list)
    for i, e in enumerate(edges):
        adj[e[0]].append(i)
    return coord, edges, adj


def dijkstra(coord, edges, adj, src, dst, allow):
    dist = {src: 0.0}
    prev = {}
    heap = [(0.0, src)]
    while heap:
        d, u = heapq.heappop(heap)
        if u == dst:
            break
        if d > dist.get(u, math.inf):
            continue
        for i in adj[u]:
            f, t, length, spd, is_alley, _ = edges[i]
            if not allow(i):
                continue
            nd = d + length / (spd / 3.6)
            if nd < dist.get(t, math.inf):
                dist[t] = nd
                prev[t] = i
                heapq.heappush(heap, (nd, t))
    if dst not in prev:
        return None
    path = []
    u = dst
    while u != src:
        path.append(prev[u])
        u = edges[prev[u]][0]
    return path[::-1], dist[dst]


def nearest(coord, ids, xy, p):
    d = np.hypot(xy[:, 0] - p[0], xy[:, 1] - p[1])
    i = int(d.argmin())
    return ids[i], d[i]


def main():
    key = k.env("KAKAO_REST_API_KEY")
    coord, edges, adj = load_graph()
    alley_ids = {e[0] for e in edges if e[4]} | {e[1] for e in edges if e[4]}
    all_ids = list(coord)
    all_xy = np.array([coord[i] for i in all_ids])
    moct_ids = [i for i in all_ids if i not in alley_ids]
    moct_xy = np.array([coord[i] for i in moct_ids])
    mid = np.array([np.mean(np.array(e[5]), axis=0) for e in edges])
    is_alley = np.array([e[4] for e in edges])
    print(f"그래프: 노드 {len(coord):,} 간선 {len(edges):,}(골목 {int(is_alley.sum()):,})")

    modes = ["없음", "전체"] + [f"접근 {r}m" for r in RADII]
    results = defaultdict(list)
    for a, b in PAIRS:
        pa, pb = k.geocode(a), k.geocode(b)
        if not pa or not pb:
            continue
        ta, tb = np.array(wgs84_to_tm(pa[0], pa[1])), np.array(wgs84_to_tm(pb[0], pb[1]))
        theirs = k.kakao_route(key, pa[:2], pb[:2])
        k_line = k.to_tm(theirs["coords"])
        k_buf = k_line.buffer(k.BUFFER_M)
        near = {r: (np.hypot(*(mid - ta).T) <= r) | (np.hypot(*(mid - tb).T) <= r) for r in RADII}
        print(f"\n{a.replace('서울 용산구 ', '')} → {b.replace('서울 용산구 ', '')} | 카카오 {theirs['distance'] / 1000:.2f}km {theirs['duration'] / 60:.0f}분")
        for mode in modes:
            if mode == "없음":
                src, ga = nearest(coord, moct_ids, moct_xy, ta)
                dst, gb = nearest(coord, moct_ids, moct_xy, tb)
                allow = lambda i: not is_alley[i]
            else:
                src, ga = nearest(coord, all_ids, all_xy, ta)
                dst, gb = nearest(coord, all_ids, all_xy, tb)
                if mode == "전체":
                    allow = lambda i: True
                else:
                    ok = near[int(mode.split()[1][:-1])]
                    allow = lambda i, ok=ok: not is_alley[i] or ok[i]
            found = dijkstra(coord, edges, adj, src, dst, allow)
            if not found:
                print(f"  {mode:9s} 경로 없음")
                continue
            path, sec = found
            length = sum(edges[i][2] for i in path)
            n_alley = sum(1 for i in path if edges[i][4])
            m_alley = sum(edges[i][2] for i in path if edges[i][4])
            line = LineString([c for i in path for c in edges[i][5]])
            in_k = line.intersection(k_buf).length / line.length
            in_m = k_line.intersection(line.buffer(k.BUFFER_M)).length / k_line.length
            results[mode].append((length, sec, n_alley, m_alley, in_k, in_m, ga, gb, theirs["distance"]))
            print(f"  {mode:9s} {length / 1000:.2f}km {sec / 60:.1f}분 | 골목 {n_alley}/{len(path)}링크 {m_alley / 1000:.2f}km | "
                  f"겹침 {in_k:.0%}/{in_m:.0%} | 스냅 {ga:.0f}m/{gb:.0f}m")

    print("\n방식별 중앙값 (쌍 수 | 거리 차이(카카오 대비) | 시간 | 골목 링크 수 | 골목 길이 | 겹침 로컬→카카오/카카오→로컬 | 출발·도착 스냅)")
    for mode in modes:
        r = np.array(results[mode])
        if not len(r):
            continue
        print(f"  {mode:9s} {len(r)}쌍 | {np.median((r[:, 0] - r[:, 8]) / r[:, 8]):+.0%} | {np.median(r[:, 1]) / 60:.1f}분 | "
              f"{np.median(r[:, 2]):.0f}개(합 {int(r[:, 2].sum())}) | {np.median(r[:, 3]) / 1000:.2f}km | "
              f"{np.median(r[:, 4]):.0%}/{np.median(r[:, 5]):.0%} | {np.median(r[:, 6]):.0f}m/{np.median(r[:, 7]):.0f}m")


if __name__ == "__main__":
    main()

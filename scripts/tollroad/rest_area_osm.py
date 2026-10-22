"""표준노드링크에 없는 휴게소 하이패스 요금소 도로를 OpenStreetMap 도로로 만들어 표준노드링크에 연결한다.

대상은 행담도 732, 덕평휴게소 220, 진영휴게소 758이다.
입력
  maps/osm/south-korea-261020.osm.pbf     OpenStreetMap 도로
  maps/[2026-09-14]NODELINKDATA/MOCT_LINK  연결할 표준노드링크 링크(ROAD_USE=1 제외)
방법
  1) 요금소 좌표 반경 RADIUS_M 안의 OSM 도로를 선분으로 나눈다. 선분 가운데 점에서 COVER_M 안에 진행 방향이 나란한
     표준노드링크 링크가 있으면 표준노드링크에 있는 선분으로 본다.
  2) 표준노드링크에 없는 휴게소 도로 선분(motorway_link, service. 주차장 통로 parking_aisle 제외)으로 방향 그래프를 만든다.
     motorway, motorway_link, oneway=yes는 그린 방향으로만, oneway=-1은 반대 방향으로만 통행한다.
  3) 이 그래프의 꼭짓점 중 표준노드링크에 있는 선분과 만나는 꼭짓점을 접점으로 한다.
     표준노드링크 막다른 길의 끝 노드에서 CONTACT_M 안의 꼭짓점도 접점이며 그 끝 노드에 바로 연결한다.
  4) 요금소에서 가장 가까운 선분과, 그보다 TOLL_M 안쪽으로 더 떨어지고 요금소에서 TOLL_MAX_M 안인 나란한 선분을 요금소 선분으로 한다.
     접점에서 요금소 선분까지, 요금소 선분에서 접점까지의 최단 경로의 선분만 남긴다.
  5) 접점은 만나는 선분을 덮는 표준노드링크 링크 위로 옮긴다. 양방향 도로는 F_NODE, T_NODE가 반대인 링크까지
     같은 위치에서 나누고 새 노드를 만든다. 링크 끝 END_M 안이면 그 끝 노드를 쓴다.
  6) 남긴 선분을 접점, 갈림 꼭짓점, 통행 방향이 바뀌는 꼭짓점에서 끊어 링크로 만든다. 양방향 도로는 방향별 링크 2개이다.
출력
  maps/유료도로/휴게소_연결링크.csv  LINK_ID, F_NODE, T_NODE, LENGTH, 원본LINK_ID, 순서, 영업소코드, 좌표
    새 링크와, 나눈 표준노드링크 링크의 조각이다. 조각은 원본LINK_ID와 원본 안의 순서(0부터)가 있다.
    좌표는 TM(EPSG:5186) x y를 ;로 연결한다. 요금소 선분을 지나는 새 링크에 영업소코드가 있다.
  maps/유료도로/휴게소_연결노드.csv  NODE_ID, x, y (TM)
새 링크 ID는 9900100001부터, 조각 ID는 9900200001부터, 새 노드 ID는 9900000001부터 붙인다.

python -X utf8 scripts/tollroad/rest_area_osm.py
"""
import re
import sys
from collections import defaultdict
from pathlib import Path

import geopandas as gpd
import networkx as nx
import numpy as np
import pyogrio
import shapely
from shapely.geometry import LineString, Point
from shapely.ops import substring

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import ROOT, TOLLROAD_DIR, write_csv
from nodelink import TM, WGS84, read_layer, NODE_DIR

OSM = ROOT / "maps" / "osm" / "south-korea-261020.osm.pbf"
RADIUS_M = 800.0   # OSM 도로를 읽는 반경
COVER_M = 8.0      # OSM 선분 가운데 점에서 표준노드링크 링크까지 이 거리 안이면 표준노드링크에 있는 선분
CONTACT_M = 15.0   # 막다른 길 끝 노드에서 접점 꼭짓점까지 최대 거리
END_M = 5.0        # 링크 끝에서 이 거리 안이면 나누지 않고 끝 노드를 쓴다
TOLL_M = 15.0      # 요금소에서 가장 가까운 선분보다 이 거리 안쪽으로 더 떨어진 나란한 선분도 요금소 선분
TOLL_MAX_M = 25.0  # 요금소 선분의 요금소 좌표에서 최대 거리
TARGETS = [("732", "행담도", 36.9443906, 126.8097252), ("220", "덕평휴게소", 37.2413826, 127.3886334),
           ("758", "진영휴게소", 35.2765681, 128.7170201)]
REST_ROAD = {"motorway_link", "service"}
DRIVE_ROAD = REST_ROAD | {"motorway", "trunk", "trunk_link", "primary", "primary_link", "secondary", "secondary_link",
                          "tertiary", "tertiary_link", "unclassified", "residential"}
NEW_NODE, NEW_LINK, PIECE = 9900000001, 9900100001, 9900200001
LINK_COLS = ["LINK_ID", "F_NODE", "T_NODE", "LENGTH", "원본LINK_ID", "순서", "영업소코드", "좌표"]


def tag(tags, key):
    """OSM other_tags 문자열에서 key 값을 꺼낸다."""
    m = re.search(rf'"{key}"=>"([^"]*)"', tags or "")
    return m.group(1) if m else ""


def osm_roads(center):
    """center 반경 RADIUS_M 안의 OSM 도로. 통행 방향 컬럼 fwd, bwd를 붙인다."""
    bbox = tuple(gpd.GeoSeries([center.buffer(RADIUS_M)], crs=TM).to_crs(WGS84).total_bounds)
    o = pyogrio.read_dataframe(OSM, layer="lines", where="highway IS NOT NULL", columns=["osm_id", "highway", "other_tags"], bbox=bbox)
    o = o[o.geometry.notna() & o.highway.isin(DRIVE_ROAD)].to_crs(TM).reset_index(drop=True)
    o = o[[tag(t, "service") != "parking_aisle" for t in o.other_tags]].reset_index(drop=True)
    oneway = [tag(t, "oneway") for t in o.other_tags]
    implied = o.highway.isin(["motorway", "motorway_link"]) | np.array([tag(t, "junction") == "roundabout" for t in o.other_tags])
    o["fwd"] = [w != "-1" for w in oneway]
    o["bwd"] = [w == "no" or (w not in ("yes", "1", "true", "-1") and not imp) for w, imp in zip(oneway, implied)]
    return o


def key(x, y):
    return (round(x, 3), round(y, 3))


def unit(a, b):
    v = np.array([b[0] - a[0], b[1] - a[1]])
    return v / (np.hypot(*v) or 1)


def direction_at(line, p):
    """선 위 p 근처 6m 구간의 진행 방향 단위 벡터"""
    at = line.project(p)
    a, b = line.interpolate(max(at - 3, 0)), line.interpolate(min(at + 3, line.length))
    return unit((a.x, a.y), (b.x, b.y))


def cover(near, a, b):
    """선분 a-b 가운데 점에서 COVER_M 안의 나란한 표준노드링크 링크 행 번호. 같은 방향 링크를 먼저 고른다."""
    m = Point((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
    u = unit(a, b)
    best = None
    for i in near.sindex.query(m.buffer(COVER_M), predicate="intersects"):
        line = near.geometry.iloc[i]
        cos = float(direction_at(line, m) @ u)
        if abs(cos) < 0.9:
            continue
        rank = (cos < 0, line.distance(m))
        if best is None or rank < best[0]:
            best = (rank, near.index[i])
    return None if best is None else best[1]


def build_graph(o, near):
    """표준노드링크에 없는 휴게소 도로 선분의 방향 그래프와, 꼭짓점별 만나는 표준노드링크 링크"""
    g = nx.DiGraph()
    touch = defaultdict(set)    # 꼭짓점 -> 그 꼭짓점의 표준노드링크에 있는 선분을 덮는 링크 행 번호
    for r in o.itertuples():
        pts = [key(x, y) for x, y in shapely.get_coordinates(r.geometry)]
        for a, b in zip(pts, pts[1:]):
            c = cover(near, a, b)
            if c is not None:
                touch[a].add(c)
                touch[b].add(c)
                continue
            if r.highway not in REST_ROAD:
                continue
            d = float(np.hypot(a[0] - b[0], a[1] - b[1]))
            if r.fwd:
                g.add_edge(a, b, w=d)
            if r.bwd:
                g.add_edge(b, a, w=d)
    return g, {v: s for v, s in touch.items() if v in g}


def toll_edges(g, center):
    """요금소 선분. 가장 가까운 선분과, 그보다 TOLL_M 안쪽으로 더 떨어지고 요금소에서 TOLL_MAX_M 안인 나란한 선분"""
    dist = {e: LineString(e).distance(center) for e in g.edges}
    first = min(dist, key=dist.get)
    u = unit(*first)
    return [e for e, d in dist.items() if d <= min(dist[first] + TOLL_M, TOLL_MAX_M) and abs(float(unit(*e) @ u)) > 0.8]


def keep_edges(g, tolls, contacts):
    """접점에서 요금소 선분까지, 요금소 선분에서 접점까지 최단 경로의 선분 집합"""
    keep = set(tolls)
    for u, v in tolls:
        for c in contacts:
            if c != u and nx.has_path(g, c, u):
                p = nx.shortest_path(g, c, u, weight="w")
                keep.update(zip(p, p[1:]))
            if c != v and nx.has_path(g, v, c):
                p = nx.shortest_path(g, v, c, weight="w")
                keep.update(zip(p, p[1:]))
    return keep


def main():
    links = read_layer(NODE_DIR / "MOCT_LINK.shp", ["LINK_ID", "F_NODE", "T_NODE", "LENGTH", "ROAD_RANK", "ROAD_USE"])
    links = links[links.ROAD_USE != "1"].set_crs(TM, allow_override=True).reset_index(drop=True)
    out_links, out_nodes = [], []
    splits = defaultdict(list)      # MOCT 링크 행 번호 -> [(위치 m, 노드 ID)]
    next_node, next_link = NEW_NODE, NEW_LINK
    for code, name, lat, lon in TARGETS:
        center = gpd.GeoSeries([Point(lon, lat)], crs=WGS84).to_crs(TM).iloc[0]
        o = osm_roads(center)
        near = links[links.intersects(center.buffer(RADIUS_M + 200))]
        g, touch = build_graph(o, near)
        tolls = toll_edges(g, center)
        node_at, end_xy = {}, {}
        # 표준노드링크 막다른 길의 끝 노드
        nbr, ends_xy = defaultdict(set), {}
        for r in near.itertuples():
            nbr[r.F_NODE].add(r.T_NODE)
            nbr[r.T_NODE].add(r.F_NODE)
            xy = shapely.get_coordinates(r.geometry)
            ends_xy[r.F_NODE], ends_xy[r.T_NODE] = tuple(xy[0]), tuple(xy[-1])
        verts = np.array(list(g.nodes))
        for nid, ns in nbr.items():
            d = np.hypot(verts[:, 0] - ends_xy[nid][0], verts[:, 1] - ends_xy[nid][1])
            if len(ns) == 1 and d.min() <= CONTACT_M:
                c = tuple(verts[d.argmin()])
                node_at[c], end_xy[c] = int(nid), ends_xy[nid]
        contacts = set(touch) | set(node_at)
        keep = keep_edges(g, tolls, contacts)
        used = {v for e in keep for v in e}
        # 접점을 표준노드링크 링크 위로 옮긴다
        for c in sorted(used & set(touch) - set(node_at)):
            p = Point(c)
            i = min(touch[c], key=lambda j: links.geometry[j].distance(p))
            l = links.loc[i]
            at = l.geometry.project(p)
            q = l.geometry.interpolate(at)
            end_xy[c] = (q.x, q.y)
            if at <= END_M:
                node_at[c], end_xy[c] = int(l.F_NODE), tuple(shapely.get_coordinates(l.geometry)[0])
            elif l.geometry.length - at <= END_M:
                node_at[c], end_xy[c] = int(l.T_NODE), tuple(shapely.get_coordinates(l.geometry)[-1])
            else:
                node_at[c] = next_node
                out_nodes.append({"NODE_ID": next_node, "x": round(q.x, 3), "y": round(q.y, 3)})
                splits[i].append((at, next_node))
                for j in near[(near.F_NODE == l.T_NODE) & (near.T_NODE == l.F_NODE)].index:
                    splits[j].append((links.geometry[j].project(q), next_node))
                next_node += 1
            print(f"  {name} 접점 -> 링크 {l.LINK_ID} {l.geometry.distance(p):.1f}m 위치 {at / l.geometry.length:.3f} 노드 {node_at[c]}")
        for c in sorted(used & set(node_at) - set(touch)):
            print(f"  {name} 접점 -> 막다른 길 끝 노드 {node_at[c]}")
        # 남긴 선분을 링크로 끊는다
        k = nx.DiGraph()
        k.add_edges_from(keep)
        und = k.to_undirected()

        def two_way(a, b):
            return k.has_edge(a, b) and k.has_edge(b, a)

        def is_break(v):
            if v in node_at or und.degree(v) != 2:
                return True
            a, b = und.neighbors(v)
            return two_way(a, v) != two_way(b, v)

        done = set()
        for u, v in sorted(keep):
            if (u, v) in done or not is_break(u):
                continue
            chain = [u, v]
            done.add((u, v))
            while not is_break(chain[-1]):
                nxt = next(w for w in k.successors(chain[-1]) if w != chain[-2])
                done.add((chain[-1], nxt))
                chain.append(nxt)
            ends = []
            for v_ in (chain[0], chain[-1]):
                if v_ not in node_at:
                    node_at[v_] = next_node
                    out_nodes.append({"NODE_ID": next_node, "x": v_[0], "y": v_[1]})
                    next_node += 1
                ends.append(node_at[v_])
            # 접점은 표준노드링크 위로 옮긴 위치로 끝낸다
            line = LineString([end_xy.get(chain[0], chain[0])] + chain[1:-1] + [end_xy.get(chain[-1], chain[-1])])
            has_toll = any(e in tolls for e in zip(chain, chain[1:]))
            out_links.append({"LINK_ID": next_link, "F_NODE": ends[0], "T_NODE": ends[1], "LENGTH": round(line.length, 3),
                              "영업소코드": code if has_toll else "",
                              "좌표": ";".join(f"{x:.3f} {y:.3f}" for x, y in line.coords)})
            next_link += 1
        print(f"{code} {name} OSM 도로 {len(o)} 요금소 선분 {len(tolls)} 남긴 선분 {len(keep)} 접점 {len(used & contacts)}")
    # 접점 위치에서 표준노드링크 링크를 나눈다
    next_piece = PIECE
    for i, cuts in splits.items():
        l = links.loc[i]
        cuts = sorted(cuts)
        bounds = [0.0] + [a for a, _ in cuts] + [l.geometry.length]
        ids = [int(l.F_NODE)] + [n for _, n in cuts] + [int(l.T_NODE)]
        for k_ in range(len(bounds) - 1):
            piece = substring(l.geometry, bounds[k_], bounds[k_ + 1])
            out_links.append({"LINK_ID": next_piece, "F_NODE": ids[k_], "T_NODE": ids[k_ + 1],
                              "LENGTH": round(float(l.LENGTH) * piece.length / l.geometry.length, 3),
                              "원본LINK_ID": l.LINK_ID, "순서": k_,
                              "좌표": ";".join(f"{x:.3f} {y:.3f}" for x, y in shapely.get_coordinates(piece))})
            next_piece += 1
    write_csv(TOLLROAD_DIR / "휴게소_연결링크.csv", out_links, LINK_COLS)
    write_csv(TOLLROAD_DIR / "휴게소_연결노드.csv", out_nodes, ["NODE_ID", "x", "y"])
    print("저장 링크", len(out_links), "노드", len(out_nodes), "나눈 표준노드링크 링크", len(splits))


if __name__ == "__main__":
    main()

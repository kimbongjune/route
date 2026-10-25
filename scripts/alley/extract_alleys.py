"""골목 실험 1단계: 연속수치지형도 도로중심선의 양방통행 골목을 표준노드링크에 잇는 입력 파일을 만든다.

- 구역(도로명주소 전자지도 TL_SCCO_SIG의 시군구 코드. 기본 용산구 11170, 앞 두 자리만 주면 시도 전체) 안의 도로중심선(N3L_A0020000, EPSG:5179)을 읽는다
- 양방통행(ONSD ITH002)만 쓰고, 선 길이의 절반 이상이 표준노드링크 링크 COVER_M 안에 있는 선은 이미 있는 도로로 보고 뺀다.
  다만 뺀 선 가운데 끝점이 남긴 골목의 끝점과 닿는 짧은 선(STUB_M 미만)은 큰길과 골목을 잇는 토막이므로 되살린다
- 골목 끝점은 표준노드링크 노드 SNAP_M 안이면 그 노드에 잇고, 노드는 없지만 표준노드링크 링크 MID_M 안이면 링크 위에 새 교차로를
  만들어 잇는다(임포터가 그 링크를 그 자리에서 둘로 나눈다). 골목끼리는 끝점이 JOIN_M 안이면 같은 노드이다
- 표준노드링크에 이어진 성분만 남기고 결과를 CSV로 쓴다
  maps/골목/<구역>/<구역>_골목_노드.csv  컬럼 node_id, lat, lon, on_link, on_link_rev, at(링크 위 위치 비율. 링크 위 교차로만 값이 있다)
  maps/골목/<구역>/<구역>_골목_링크.csv  컬럼 link_id, from, to, length_m, width_m, name, rddv, wkt(WGS84 lon lat)
  구역 이름은 시군구명(용산구→용산)이나 시도명(서울특별시)이다
- 연결 통계(끝점이 노드·링크 위 교차로·골목·어디에도 이어지지 않은 수, 성분 수)를 출력한다

python -X utf8 scripts/alley/extract_alleys.py [시군구 코드. 기본 11170. 11이면 서울 전체]
임포터에 넣을 때: ROUTE_ALLEY_DIR=maps/골목/용산 ./rust/target/release/importer-knl.exe
"""
import glob
import sys
from collections import defaultdict
from pathlib import Path

import geopandas as gpd
import numpy as np
import pandas as pd
import pyogrio
from shapely.geometry import LineString, Point
from shapely.ops import unary_union
from shapely.strtree import STRtree

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, write_csv

TM5186, UTMK, WGS84 = "EPSG:5186", "EPSG:5179", "EPSG:4326"
CENTERLINE = sorted(glob.glob(str(ROOT / "maps" / "도로중심선" / "*" / "*.shp")))
MOCT = ROOT / "maps" / "[2026-09-14]NODELINKDATA"
CODE = sys.argv[1] if len(sys.argv) > 1 else "11170"
COVER_M = 15      # 표준노드링크 링크에서 이 거리 안이면 같은 도로
STUB_M = 30       # 되살리는 연결 토막의 최대 길이
SNAP_M = 10       # 골목 끝점이 표준노드링크 노드에 이어지는 거리
MID_M = 20        # 골목 끝점이 표준노드링크 링크 위 새 교차로에 이어지는 거리
JOIN_M = 0.5      # 골목 끝점끼리 같은 점으로 보는 거리
# 새 노드·링크 번호. 시도별로 겹치지 않게 시도 코드(앞 두 자리)로 범위를 나눈다.
# 노드는 20,000,000,000 + 코드 × 100,000,000부터, 링크는 30,000,000,000 + 코드 × 1,000,000,000부터 2씩(정방향 짝수, 역방향은 +1)이다
NODE_BASE = 20_000_000_000 + int(CODE[:2]) * 100_000_000
LINK_BASE = 30_000_000_000 + int(CODE[:2]) * 1_000_000_000
RDDV = {"RDD000": "미분류", "RDD001": "고속국도", "RDD002": "일반국도", "RDD003": "지방도", "RDD008": "면리간도로", "RDD009": "소로"}


def region(code):
    """시군구 코드(앞 두 자리면 시도)의 구역 도형과 이름(용산구→용산, 서울특별시→서울)"""
    sig = pd.concat([pyogrio.read_dataframe(f, encoding="cp949") for f in glob.glob(str(ROOT / "maps/도로명주소 전자지도/*/*/TL_SCCO_SIG.shp"))])
    sig = gpd.GeoDataFrame(sig, geometry="geometry").set_crs(UTMK, allow_override=True)
    part = sig[sig.SIG_CD.str.startswith(code)]
    if len(code) == 2:
        sido = pyogrio.read_dataframe(glob.glob(str(ROOT / f"maps/도로명주소 전자지도/*/{code}000/TL_SCCO_CTPRVN.shp"))[0], encoding="cp949")
        name = sido.CTP_KOR_NM.iloc[0]
    else:
        name = part.SIG_KOR_NM.iloc[0].rstrip("구시군")
    return unary_union(list(part.geometry)), name


def read_centerlines(area):
    minx, miny, maxx, maxy = area.bounds
    parts = [pyogrio.read_dataframe(f, bbox=(minx, miny, maxx, maxy), encoding="cp949") for f in CENTERLINE]
    cl = gpd.GeoDataFrame(pd.concat([p for p in parts if len(p)], ignore_index=True), geometry="geometry", crs=UTMK)
    return cl[cl.intersects(area)].reset_index(drop=True)


def covered_share(lines, links):
    """선마다 길이 중 표준노드링크 링크 COVER_M 안에 든 비율. 선 근처 링크의 버퍼만 합쳐 교차한다"""
    buffers = links.buffer(COVER_M).reset_index(drop=True)
    tree = STRtree(list(buffers))
    share = np.ones(len(lines))
    for i, g in enumerate(lines):
        if g.length == 0:
            continue
        near = tree.query(g, predicate="intersects")
        if len(near) == 0:
            share[i] = 0.0
            continue
        share[i] = g.intersection(unary_union(list(buffers.iloc[near]))).length / g.length
    return pd.Series(share, index=lines.index)


class Nodes:
    """끝점을 노드 번호로 바꾼다. 표준노드링크 노드, 링크 위 새 교차로, 골목끼리 묶은 새 노드 순으로 정한다"""

    def __init__(self, nodes, links):
        self.nodes = nodes
        self.node_tree = STRtree(list(nodes.geometry))
        self.links = links.reset_index(drop=True)
        self.link_tree = STRtree(list(self.links.geometry))
        # 반대 방향 링크(F_NODE·T_NODE가 바뀐 링크)
        key = {(f, t): lid for f, t, lid in zip(self.links.F_NODE, self.links.T_NODE, self.links.LINK_ID)}
        self.reverse = {lid: key.get((t, f), "") for f, t, lid in zip(self.links.F_NODE, self.links.T_NODE, self.links.LINK_ID)}
        self.new_nodes = {}      # node_id -> (Point TM, on_link, on_link_rev, at)
        self.junction_on = {}    # (link_id, round(at,4)) -> node_id
        self.count = {"node": 0, "junction": 0, "alley": 0, "none": 0}

    def next_id(self):
        return NODE_BASE + len(self.new_nodes)

    def resolve(self, pt):
        """표준노드링크 노드 또는 링크 위 교차로 번호. 없으면 None"""
        i = self.node_tree.nearest(pt)
        if i is not None and self.nodes.geometry.iloc[i].distance(pt) <= SNAP_M:
            self.count["node"] += 1
            return int(self.nodes.NODE_ID.iloc[i]), self.nodes.geometry.iloc[i]
        j = self.link_tree.nearest(pt)
        line = self.links.geometry.iloc[j]
        if j is not None and line.distance(pt) <= MID_M:
            at = line.project(pt, normalized=True)
            lid = self.links.LINK_ID.iloc[j]
            key = (lid, round(at, 4))
            if key not in self.junction_on:
                nid = self.next_id()
                self.new_nodes[nid] = (line.interpolate(at, normalized=True), lid, self.reverse.get(lid, ""), at)
                self.junction_on[key] = nid
            self.count["junction"] += 1
            return self.junction_on[key], self.new_nodes[self.junction_on[key]][0]
        return None


def main():
    area, name = region(CODE)
    out = ROOT / "maps" / "골목" / name
    cl = read_centerlines(area)
    print(f"{name} 도로중심선", len(cl), "ONSD", cl.ONSD.value_counts(dropna=False).to_dict(), "RDDV", cl.RDDV.map(RDDV).value_counts().to_dict())
    two_way = cl[cl.ONSD == "ITH002"].to_crs(TM5186).reset_index(drop=True)

    area_tm = gpd.GeoSeries([area], crs=UTMK).to_crs(TM5186).iloc[0]
    minx, miny, maxx, maxy = area_tm.buffer(200).bounds
    links = pyogrio.read_dataframe(MOCT / "MOCT_LINK.shp", bbox=(minx, miny, maxx, maxy), encoding="cp949",
                                   columns=["LINK_ID", "F_NODE", "T_NODE", "ROAD_NAME", "ROAD_RANK", "MAX_SPD", "LANES", "ROAD_USE"])
    links = links[links.ROAD_USE != "1"].set_crs(TM5186, allow_override=True)
    nodes = pyogrio.read_dataframe(MOCT / "MOCT_NODE.shp", bbox=(minx, miny, maxx, maxy), encoding="cp949", columns=["NODE_ID"])
    nodes = nodes.set_crs(TM5186, allow_override=True)
    print(f"{name} 주변 표준노드링크 링크", len(links), "노드", len(nodes))

    # 이미 표준노드링크에 있는 도로 제외, 연결 토막은 되살림
    inside = covered_share(two_way.geometry, links.geometry)
    kept_mask = inside < 0.5
    kept_ends = STRtree([Point(c) for g in two_way[kept_mask].geometry for c in (g.coords[0], g.coords[-1])])
    revived = 0
    for i in np.where(~kept_mask)[0]:
        g = two_way.geometry.iloc[i]
        if g.length < STUB_M and any(len(kept_ends.query(Point(c).buffer(JOIN_M))) for c in (g.coords[0], g.coords[-1])):
            kept_mask.iloc[i] = True
            revived += 1
    alleys = two_way[kept_mask].reset_index(drop=True)
    print(f"양방통행 {len(two_way)}개 {two_way.geometry.length.sum() / 1000:.1f}km 중 표준노드링크에 없는 골목 {len(alleys)}개 {alleys.geometry.length.sum() / 1000:.1f}km"
          f"(되살린 연결 토막 {revived}개), 도로구분 {alleys.RDDV.map(RDDV).value_counts().to_dict()}, 폭 중앙값 {float(alleys.RVWD.median())}m")

    # 끝점 → 노드
    resolver = Nodes(nodes, links)
    ends = [(c if side == 0 else None) for g in alleys.geometry for side, c in ((0, g.coords[0]), (1, g.coords[-1]))]
    ends = [Point(c) for g in alleys.geometry for c in (g.coords[0], g.coords[-1])]
    end_node = [None] * len(ends)
    end_point = [None] * len(ends)
    for k, pt in enumerate(ends):
        r = resolver.resolve(pt)
        if r is not None:
            end_node[k], end_point[k] = r
    # 골목끼리 묶기(표준노드링크에 이어지지 않은 끝점)
    rest = [k for k in range(len(ends)) if end_node[k] is None]
    tree = STRtree([ends[k] for k in rest])
    parent = {k: k for k in rest}

    def find(x):
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    for pos, k in enumerate(rest):
        for q in tree.query(ends[k].buffer(JOIN_M)):
            parent[find(k)] = find(rest[q])
    groups = defaultdict(list)
    for k in rest:
        groups[find(k)].append(k)
    for members in groups.values():
        nid = resolver.next_id()
        resolver.new_nodes[nid] = (ends[members[0]], "", "", None)
        for k in members:
            end_node[k] = nid
            end_point[k] = ends[members[0]]
        if len(members) == 1:
            resolver.count["none"] += 1
        else:
            resolver.count["alley"] += len(members)
    c = resolver.count
    print(f"골목 끝점 {len(ends)}개: 표준노드링크 노드 {c['node']}개, 링크 위 새 교차로 {c['junction']}개, 골목끼리 {c['alley']}개, 어디에도 안 이어짐 {c['none']}개({c['none'] / len(ends):.0%})")

    # 성분: 표준노드링크 노드 또는 링크 위 교차로가 든 성분만 남긴다
    adj = defaultdict(set)
    for i in range(len(alleys)):
        a, b = end_node[2 * i], end_node[2 * i + 1]
        adj[a].add(b)
        adj[b].add(a)
    comp = {}
    for start in list(adj):
        if start in comp:
            continue
        stack = [start]
        comp[start] = start
        while stack:
            v = stack.pop()
            for w in adj[v]:
                if w not in comp:
                    comp[w] = start
                    stack.append(w)
    attached = {n for n in comp if n < 20_000_000_000 or (n in resolver.new_nodes and resolver.new_nodes[n][1])}
    roots = {comp[n] for n in attached}
    keep = [i for i in range(len(alleys)) if comp[end_node[2 * i]] in roots and end_node[2 * i] != end_node[2 * i + 1]]
    kept = alleys.loc[keep]
    dropped_len = alleys.geometry.length.sum() - kept.geometry.length.sum()
    print(f"성분 {len({comp[n] for n in comp})}개 중 표준노드링크에 이어진 성분 {len(roots)}개. 남긴 골목 {len(kept)}개 {kept.geometry.length.sum() / 1000:.1f}km, 고립 {dropped_len / 1000:.1f}km")

    # 출력. 골목 선 끝을 이어진 노드 위치까지 늘여 기하가 닿게 한다
    out.mkdir(parents=True, exist_ok=True)
    rows, used = [], set()
    for i in keep:
        a, b = end_node[2 * i], end_node[2 * i + 1]
        coords = list(alleys.geometry.iloc[i].coords)
        pa, pb = end_point[2 * i], end_point[2 * i + 1]
        if Point(coords[0]).distance(pa) > 0.01:
            coords.insert(0, (pa.x, pa.y))
        if Point(coords[-1]).distance(pb) > 0.01:
            coords.append((pb.x, pb.y))
        line = LineString(coords)
        wgs = gpd.GeoSeries([line], crs=TM5186).to_crs(WGS84).iloc[0]
        rows.append({"link_id": LINK_BASE + 2 * len(rows), "from": a, "to": b, "length_m": round(line.length, 1),
                     "width_m": alleys.RVWD.iloc[i], "name": alleys.RDNM.iloc[i] or alleys.NAME.iloc[i] or "",
                     "rddv": RDDV.get(alleys.RDDV.iloc[i], alleys.RDDV.iloc[i]),
                     "wkt": " ".join(f"{lon:.7f} {lat:.7f}" for lon, lat in wgs.coords)})
        used.update(n for n in (a, b) if n >= NODE_BASE)
    node_rows = []
    for nid in sorted(used):
        pt, on_link, on_rev, at = resolver.new_nodes[nid]
        w = gpd.GeoSeries([pt], crs=TM5186).to_crs(WGS84).iloc[0]
        node_rows.append({"node_id": nid, "lat": round(w.y, 7), "lon": round(w.x, 7), "on_link": on_link, "on_link_rev": on_rev,
                          "at": "" if at is None else round(at, 5)})
    write_csv(out / f"{name}_골목_링크.csv", pd.DataFrame(rows))
    write_csv(out / f"{name}_골목_노드.csv", pd.DataFrame(node_rows))
    junctions = sum(1 for r in node_rows if r["on_link"])
    print(f"저장 링크 {len(rows)}개, 새 노드 {len(node_rows)}개(그중 링크 위 교차로 {junctions}개) → {out}")


if __name__ == "__main__":
    main()

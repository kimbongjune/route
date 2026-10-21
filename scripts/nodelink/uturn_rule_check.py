"""U턴 판정 규칙을 TURNINFO로 검증한다.

- F_NODE·T_NODE가 서로 뒤집힌 반대 방향 링크 수
- 011 U-TURN의 진입 링크 끝 방향과 진출 링크 시작 방향의 코사인, 두 링크가 나란한 거리
- 후보 규칙별로 011을 U턴으로 잡는 수와 101·102·103 금지 회전을 U턴으로 잘못 잡는 수
- 진입 링크 도착 노드에서 진출 링크 출발 노드까지 5개 이하 링크로 갈 수 없는 회전정보의 유형, NODE_ID 노드 유형, 두 노드 거리

python -X utf8 scripts/nodelink/uturn_rule_check.py
"""
import sys
from collections import defaultdict, deque
from pathlib import Path

import numpy as np
from shapely.geometry import Point

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
from nodelink import read_links, read_nodes, read_table


def unit(a, b):
    v = np.array(b) - np.array(a)
    return v / np.hypot(*v)


def end_dir(g):
    c = g.coords
    return unit(c[-2], c[-1])


def start_dir(g):
    c = g.coords
    return unit(c[0], c[1])


def side_distance(st, ed, xy):
    """진출 링크 끝점에서 진입 링크까지, 진입 링크 시작점에서 진출 링크까지 거리 중 작은 값"""
    out = []
    for (sg, sf), (eg, et) in zip(zip(st.geometry, st.F_NODE), zip(ed.geometry, ed.T_NODE)):
        out.append(min(sg.distance(Point(xy.loc[et])), eg.distance(Point(xy.loc[sf]))))
    return np.array(out)


def main():
    links = read_links(["LINK_ID", "F_NODE", "T_NODE", "ROAD_USE"])
    links = links[links.ROAD_USE != "1"]
    nodes = read_nodes(["NODE_ID"])
    xy = nodes.set_index("NODE_ID").geometry.apply(lambda p: (p.x, p.y))
    pairs = set(zip(links.F_NODE, links.T_NODE))
    reversed_count = sum((t, f) in pairs for f, t in zip(links.F_NODE, links.T_NODE))
    print("ROAD_USE=1 제외 링크", f"{len(links):,}", "| F_NODE·T_NODE가 뒤집힌 반대 방향 링크 있음", f"{reversed_count:,}")

    turns = read_table("TURNINFO")
    L = links.set_index("LINK_ID")
    turns = turns[turns.ST_LINK.isin(L.index) & turns.ED_LINK.isin(L.index)]

    def measure(ty):
        t = turns[turns.TURN_TYPE.isin(ty)]
        st, ed = L.loc[t.ST_LINK], L.loc[t.ED_LINK]
        cos = np.array([end_dir(a) @ start_dir(b) for a, b in zip(st.geometry, ed.geometry)])
        return t, st, ed, cos, side_distance(st, ed, xy)

    u, st, ed, cos, side = measure(["011"])
    node_dist = np.hypot(*(np.array(xy.loc[st.F_NODE].tolist()) - np.array(xy.loc[ed.T_NODE].tolist())).T)
    print("011", f"{len(u):,}건")
    print("  ST.F_NODE == ED.T_NODE", (st.F_NODE.to_numpy() == ed.T_NODE.to_numpy()).sum(),
          "| ST.F_NODE~ED.T_NODE 거리 중앙값", f"{np.median(node_dist):.1f}m", "75%", f"{np.percentile(node_dist, 75):.1f}m")
    print("  cos 분위 5% 25% 50%", np.percentile(cos, [5, 25, 50]).round(2), "| cos<-0.9", f"{(cos < -0.9).sum():,}")
    print("  나란한 거리 분위 50% 90% 95%", np.percentile(side, [50, 90, 95]).round(1))

    f, _, _, cos2, side2 = measure(["101", "102", "103"])
    print("101·102·103", f"{len(f):,}건")
    for c, dm in ((-0.9, None), (-0.8, 50), (-0.9, 30), (-0.9, 50)):
        hit = cos < c if dm is None else (cos < c) & (side <= dm)
        miss = cos2 < c if dm is None else (cos2 < c) & (side2 <= dm)
        rule = f"cos<{c}" + ("" if dm is None else f" 및 나란한 거리<={dm}m")
        print(f"  규칙 {rule}: 011 중 U턴 판정 {hit.sum():,}/{len(hit):,} | 101·102·103 중 U턴 판정 {miss.sum():,}/{len(miss):,}")


def within_links(adj, a, b, limit=5):
    seen = {a: 0}
    q = deque([a])
    while q:
        v = q.popleft()
        if v == b:
            return True
        if seen[v] < limit:
            for w in adj[v]:
                if w not in seen:
                    seen[w] = seen[v] + 1
                    q.append(w)
    return False


def no_path():
    links = read_links(["LINK_ID", "F_NODE", "T_NODE", "ROAD_RANK", "ROAD_USE"], geometry=False)
    links = links[links.ROAD_USE != "1"]
    nodes = read_nodes(["NODE_ID", "NODE_TYPE"]).set_index("NODE_ID")
    adj = defaultdict(list)
    for f, t in zip(links.F_NODE, links.T_NODE):
        adj[f].append(t)
    L = links.set_index("LINK_ID")
    turns = read_table("TURNINFO")
    turns = turns[turns.TURN_TYPE.isin(["002", "003", "011", "101", "102", "103"])
                  & turns.ST_LINK.isin(L.index) & turns.ED_LINK.isin(L.index)]
    st, ed = L.loc[turns.ST_LINK], L.loc[turns.ED_LINK]
    far = np.array([not within_links(adj, a, b) for a, b in zip(st.T_NODE, ed.F_NODE)])
    gap = np.array([nodes.geometry.loc[a].distance(nodes.geometry.loc[b]) for a, b in zip(st.T_NODE, ed.F_NODE)])
    f = turns[far]
    print("5개 이하 링크로 갈 수 없는 회전정보", f"{len(f):,}", "| 유형", f.TURN_TYPE.value_counts().to_dict())
    print("  두 노드 직선거리 분위 10% 50% 90%", np.percentile(gap[far], [10, 50, 90]).round(0))
    u = f[f.TURN_TYPE == "011"]
    print("  011", f"{len(u):,}", "| NODE_ID 노드 유형", nodes.loc[u.NODE_ID].NODE_TYPE.value_counts().to_dict(),
          "| 진입 링크 고속국도", (L.loc[u.ST_LINK].ROAD_RANK == "101").sum())
    s, e = L.loc["2840198100"], L.loc["2840197800"]
    print("  오창IC 2840034002: 진입", "2840198100", s.F_NODE, "->", s.T_NODE, "| 진출", "2840197800", e.F_NODE, "->", e.T_NODE,
          f"| 두 노드 거리 {nodes.geometry.loc[s.T_NODE].distance(nodes.geometry.loc[e.F_NODE]):.1f}m")


if __name__ == "__main__":
    main()
    no_path()

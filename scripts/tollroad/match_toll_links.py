"""match_units_moct.py로 만든 영업소_요금지점.csv의 영업소 요금 지점을 표준노드링크 링크에 매칭한다.

- LINK_IDS가 있는 분기점 출구·입구 영업소는 그 링크만 기록한다.
- 기준이 MOCT노드인 영업소는 NODE_IDS의 노드에 F_NODE나 T_NODE로 접한 링크를 모두 기록하며 하이패스IC 노드는 램프만 기록한다.
  거리m, 거리순위는 비워 둔다.
- 기준이 국가POI나 도로공사인 영업소는 그 좌표 반경 RADIUS_M 이내의 링크 중 최근접 링크와 도로명, 램프 여부가 같은 링크를 거리순으로 기록한다.
- 스마트톨링 구간인 용마터널 030과 청라하늘대교 032는 양 끝 노드 이름으로 노드를 찾고 링크 길이 기준 양방향 최단 경로의 링크를 기록한다.
    030 용마터널: 용마터널(서측) <-> 용마터널(동측)
    032 청라하늘대교: 제3연륙교(서측) <-> 제3연륙교(동측)
결과는 요금지점_링크.csv이며 컬럼은 영업소코드, 영업소명, 기준, 이름, LINK_ID, 거리m, 거리순위, ROAD_NAME, ROAD_RANK, 램프, F_NODE, T_NODE이다.

python -X utf8 scripts/tollroad/match_toll_links.py
"""
import heapq
import sys
from collections import defaultdict
from pathlib import Path

import pandas as pd
import shapely

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import TOLLROAD_DIR, read_csv, write_csv
from nodelink import points, read_links, read_nodes

RADIUS_M = 100.0
SECTIONS = [("030", "용마터널", "용마터널(서측)", "용마터널(동측)"),
            ("032", "청라하늘대교", "제3연륙교(서측)", "제3연륙교(동측)")]
LINK_FIELDS = ["LINK_ID", "F_NODE", "T_NODE", "ROAD_NAME", "ROAD_RANK", "ROAD_USE", "CONNECT", "LENGTH"]
B_COLS = ["영업소코드", "영업소명", "기준", "이름", "LINK_ID", "거리m", "거리순위", "ROAD_NAME", "ROAD_RANK", "램프", "F_NODE", "T_NODE"]


def link_row(l, **extra):
    return {"LINK_ID": l["LINK_ID"], "ROAD_NAME": l["ROAD_NAME"], "ROAD_RANK": l["ROAD_RANK"],
            "램프": "Y" if l["CONNECT"] == "1" else "", "F_NODE": l["F_NODE"], "T_NODE": l["T_NODE"], **extra}


def near_links(pts, links):
    """점마다 반경 RADIUS_M 이내의 링크를 거리, LINK_ID 순으로 {점 index: [(거리, LINK_ID)]}로 반환한다."""
    pi, li = links.sindex.query(pts.geometry, predicate="dwithin", distance=RADIUS_M)
    hits = pd.DataFrame({"p": pts.index[pi], "LINK_ID": links.LINK_ID.to_numpy()[li]})
    hits["d"] = shapely.distance(pts.geometry.to_numpy()[pi], links.geometry.to_numpy()[li])
    # 끝 노드를 공유하는 링크처럼 거리가 같은 링크는 계산 오차와 상관없이 LINK_ID 순으로 둔다
    hits["d6"] = hits.d.round(6)
    hits = hits.sort_values(["p", "d6", "LINK_ID"])
    return {p: list(zip(g.d, g.LINK_ID)) for p, g in hits.groupby("p", sort=False)}


def main():
    links = read_links(LINK_FIELDS)
    by_id = links.drop(columns="geometry").set_index("LINK_ID", drop=False).to_dict("index")

    units = read_csv(TOLLROAD_DIR / "영업소_요금지점.csv")

    # 노드 기준 영업소는 노드에 접한 링크를, 좌표 기준 영업소는 반경 안 링크를 찾는다
    node_units = units[(units.NODE_IDS != "") & ~units.기준.str.startswith("링크지정")]
    unit_nodes = node_units.NODE_IDS.str.split(";").explode()
    touched = {}
    for i, ns in unit_nodes.groupby(level=0):
        m = links.F_NODE.isin(ns) | links.T_NODE.isin(ns)
        touched[i] = sorted(set(links.LINK_ID[m]))
    coord = units[(units.NODE_IDS == "") & (units.위도 != "") & ~units.기준.isin(["제외", "없음"]) & ~units.기준.str.startswith("링크지정")]
    near = near_links(points(coord), links)

    rows, no_link = [], []
    for i, u in units.iterrows():
        base = {"영업소코드": u.영업소코드, "영업소명": u.영업소명, "기준": u.기준}
        if u.기준 in ("제외", "없음"):
            continue
        if u.기준.startswith("링크지정"):  # 링크를 직접 지정한 영업소. 비어 있으면 MOCT에 해당 도로가 없는 영업소이다
            for lid in (u.LINK_IDS.split(";") if u.LINK_IDS else []):
                rows.append({**base, "이름": u.참조명, **link_row(by_id[lid])})
            if not u.LINK_IDS:
                rows.append({**base, "이름": u.참조명})
                no_link.append(f"{u.영업소코드} {u.영업소명} ({u.참조명})")
            continue
        if u.NODE_IDS:
            lids = touched.get(i, [])
            if "하이패스IC" in u.노드명:  # 하이패스IC 노드는 본선 위의 분기점이므로 요금소가 있는 램프만 남긴다
                lids = [l for l in lids if by_id[l]["CONNECT"] == "1"]
            for lid in lids:
                rows.append({**base, "이름": u.노드명, **link_row(by_id[lid])})
            if not lids:
                rows.append({**base, "이름": u.노드명})
                no_link.append(f"{u.영업소코드} {u.영업소명}")
        else:
            hits = near.get(i, [])
            if hits:  # 램프 요금소 옆의 본선이 포함되지 않도록 최근접 링크와 도로명, 램프 여부가 같은 링크만 남긴다
                l0 = by_id[hits[0][1]]
                hits = [h for h in hits if (by_id[h[1]]["ROAD_NAME"], by_id[h[1]]["CONNECT"]) == (l0["ROAD_NAME"], l0["CONNECT"])]
            for rank, (d, lid) in enumerate(hits, 1):
                rows.append({**base, "이름": u.참조명, **link_row(by_id[lid], 거리m=round(d, 1), 거리순위=rank)})
            if not hits:
                rows.append({**base, "이름": u.참조명})
                no_link.append(f"{u.영업소코드} {u.영업소명} ({u.참조명})")

    names = {n for _, _, a, b in SECTIONS for n in (a, b)}
    nodes = read_nodes(["NODE_ID", "NODE_NAME"], geometry=False)
    node_ids = nodes[nodes.NODE_NAME.isin(names)].groupby("NODE_NAME").NODE_ID.agg(list).to_dict()
    adj = defaultdict(list)
    for l in links[links.ROAD_USE != "1"].drop(columns="geometry").to_dict("records"):
        adj[l["F_NODE"]].append(l)
    for code, title, a, b in SECTIONS:
        for src, dst in ((a, b), (b, a)):
            path = shortest(adj, node_ids[src], set(node_ids[dst]))
            for l in path:
                rows.append({"영업소코드": code, "영업소명": title, "기준": "구간", "이름": f"{src}->{dst}", **link_row(l)})
            print(title, src, "->", dst, "링크", len(path), "길이m", round(sum(l["LENGTH"] for l in path)))

    out_b = TOLLROAD_DIR / "요금지점_링크.csv"
    write_csv(out_b, pd.DataFrame(rows, dtype=object), B_COLS)
    print("영업소 요금 지점", (~units.기준.isin(["제외", "없음"])).sum(), "링크 없음", len(no_link), no_link)
    print("저장:", out_b)


def shortest(adj, starts, goals, limit=20000.0):
    dist = {s: 0.0 for s in starts}
    prev = {}
    heap = [(0.0, s) for s in starts]
    while heap:
        d, u = heapq.heappop(heap)
        if d > dist.get(u, float("inf")):
            continue
        if u in goals:
            path = []
            while u in prev:
                path.append(prev[u])
                u = prev[u]["F_NODE"]
            return path[::-1]
        if d > limit:
            break
        for l in adj.get(u, ()):
            nd = d + l["LENGTH"]
            if nd < dist.get(l["T_NODE"], float("inf")):
                dist[l["T_NODE"]] = nd
                prev[l["T_NODE"]] = l
                heapq.heappush(heap, (nd, l["T_NODE"]))
    return []


if __name__ == "__main__":
    main()

"""표준노드링크(2026-10-14판)에서 경로 탐색에 문제가 되는 항목을 ID·좌표 목록으로 뽑는다.

- 회전정보_연결링크없음: 금지·U턴 회전정보(002·003·011·101·102·103) 중 진입 링크 도착 노드에서 진출 링크 출발 노드까지
  ROAD_USE=0 링크 5개 이하로 갈 수 없는 행
- 회전정보_참조오류: 진입·진출 링크가 MOCT_LINK에 없거나 ROAD_USE=1이거나, 진입 링크 T_NODE가 NODE_ID와 다른 행.
  진출 링크 F_NODE가 NODE_ID와 다른 것은 교차로 고리 구조라 대부분이 그렇고(13 문서 "회전 제한") 오류로 보지 않는다
- 진출불가_링크: 그래프에서 들어오는 통행은 있고 나가는 통행이 없는 링크 중 끝 노드에서 나가는 다른 링크가 원본에 있는 것과 그 원인
  (이어지는 링크가 모두 ROAD_USE=1, 이어지는 사용 링크에 TURNINFO 금지, 25m 이하 링크로 이어진 교차로(NODE_TYPE 101) 묶음에서 나가는 사용 링크 없음, 그 밖)
- 고립_링크: 그래프에서 들어오는 통행도 나가는 통행도 없고 회전 경유 링크도 아닌 링크 중, 시작 노드로 들어오는 다른 링크나
  끝 노드에서 나가는 다른 링크가 원본에 없는 것. 양쪽이 모두 이어지는 것(대부분 교차로 안 10m 안팎 링크)은 임포터가 교차로 그룹 내부의
  더 짧은 경로를 택해 쓰이지 않는 것이라 넣지 않는다
- ROAD_USE1_링크: ROAD_USE=1 링크 전체
입력의 그래프 목록은 routedb-core 예제 turns_of --dead가 쓴 파일(LINK_ID,in|isolated)이다
  cargo run --release -p routedb-core --example turns_of -- maps/routedb/moct.rdb --dead <목록 파일>
출력은 docs/15-데이터-문제/표준노드링크/ 아래 CSV(UTF-8 BOM)이고 좌표는 WGS84 위도·경도이다

python -X utf8 scripts/report/nodelink_issues.py <목록 파일>
"""
import sys
from collections import Counter, defaultdict, deque
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT
from nodelink import WGS84, read_links, read_nodes, read_table

OUT = ROOT / "docs" / "15-데이터-문제" / "표준노드링크"
TURN = {"001": "비보호회전", "002": "버스만회전", "003": "회전금지", "011": "U-TURN", "012": "P-TURN",
        "101": "좌회전금지", "102": "직진금지", "103": "우회전금지"}
RANK = {"101": "고속국도", "102": "도시고속도로", "103": "일반국도", "104": "특별·광역시도", "105": "국가지원지방도",
        "106": "지방도", "107": "시군도", "108": "기타"}


def save(df, name):
    OUT.mkdir(parents=True, exist_ok=True)
    df.to_csv(OUT / f"{name}.csv", index=False, encoding="utf-8-sig")
    print(f"{name}: {len(df):,}행")


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


def main():
    links = read_links(["LINK_ID", "F_NODE", "T_NODE", "LANES", "ROAD_RANK", "ROAD_NAME", "ROAD_USE", "MAX_SPD", "LENGTH"])
    nodes = read_nodes(["NODE_ID", "NODE_TYPE", "NODE_NAME"])
    node_ll = nodes.set_index("NODE_ID").geometry.to_crs(WGS84)
    node_xy = nodes.set_index("NODE_ID").geometry
    nodes = nodes.set_index("NODE_ID")
    L = links.set_index("LINK_ID")
    mid = links.set_index("LINK_ID").geometry.interpolate(0.5, normalized=True).to_crs(WGS84)
    used = links[links.ROAD_USE == "0"]
    turns = read_table("TURNINFO")

    def ll(node):
        p = node_ll.get(node)
        return (round(p.y, 7), round(p.x, 7)) if p is not None else (None, None)

    # 회전정보 참조 오류
    rows = []
    for t in turns.itertuples():
        problems = []
        for role, lid in (("진입", t.ST_LINK), ("진출", t.ED_LINK)):
            if lid not in L.index:
                problems.append(f"{role} 링크가 MOCT_LINK에 없음")
            elif L.at[lid, "ROAD_USE"] == "1":
                problems.append(f"{role} 링크가 ROAD_USE=1")
        if t.ST_LINK in L.index and L.at[t.ST_LINK, "T_NODE"] != t.NODE_ID:
            problems.append("진입 링크 T_NODE가 NODE_ID와 다름")
        if t.NODE_ID not in nodes.index:
            problems.append("NODE_ID가 MOCT_NODE에 없음")
        if problems:
            lat, lon = ll(t.NODE_ID)
            rows.append({"TURN_ID": t.TURN_ID, "TURN_TYPE": t.TURN_TYPE, "유형": TURN.get(t.TURN_TYPE, ""), "NODE_ID": t.NODE_ID,
                         "ST_LINK": t.ST_LINK, "ED_LINK": t.ED_LINK, "문제": " / ".join(problems), "위도": lat, "경도": lon})
    ref = pd.DataFrame(rows)
    save(ref, "회전정보_참조오류")
    if len(ref):
        print("  ", Counter(p for s in ref["문제"] for p in s.split(" / ")))

    # 회전정보 연결 링크 없음
    adj = defaultdict(list)
    for f, t in zip(used.F_NODE, used.T_NODE):
        adj[f].append(t)
    U = used.set_index("LINK_ID")
    tt = turns[turns.TURN_TYPE.isin(["002", "003", "011", "101", "102", "103"])
               & turns.ST_LINK.isin(U.index) & turns.ED_LINK.isin(U.index)]
    rows = []
    for t in tt.itertuples():
        a, b = U.at[t.ST_LINK, "T_NODE"], U.at[t.ED_LINK, "F_NODE"]
        if within_links(adj, a, b):
            continue
        lat, lon = ll(t.NODE_ID)
        rows.append({"TURN_ID": t.TURN_ID, "TURN_TYPE": t.TURN_TYPE, "유형": TURN[t.TURN_TYPE], "NODE_ID": t.NODE_ID,
                     "NODE_TYPE": nodes.NODE_TYPE.get(t.NODE_ID, ""), "NODE_NAME": nodes.NODE_NAME.get(t.NODE_ID, ""),
                     "ST_LINK": t.ST_LINK, "ST_도로명": U.at[t.ST_LINK, "ROAD_NAME"], "ST_도착노드": a,
                     "ED_LINK": t.ED_LINK, "ED_도로명": U.at[t.ED_LINK, "ROAD_NAME"], "ED_출발노드": b,
                     "두노드거리_m": round(node_xy.loc[a].distance(node_xy.loc[b]), 1), "위도": lat, "경도": lon})
    nopath = pd.DataFrame(rows)
    save(nopath, "회전정보_연결링크없음")
    print("  ", nopath["유형"].value_counts().to_dict())

    # 진출 불가 링크와 고립 링크
    dead = pd.read_csv(sys.argv[1], header=None, names=["LINK_ID", "kind"], dtype=str)
    out_of = U.reset_index().groupby("F_NODE")[["LINK_ID", "T_NODE"]].apply(lambda g: list(zip(g.LINK_ID, g.T_NODE))).to_dict()
    all_out = links.groupby("F_NODE")[["LINK_ID", "T_NODE"]].apply(lambda g: list(zip(g.LINK_ID, g.T_NODE))).to_dict()
    all_in = links.groupby("T_NODE").LINK_ID.apply(list).to_dict()
    rules_of = {k: g for k, g in turns.groupby("ST_LINK")}
    cross = set(nodes.index[nodes.NODE_TYPE == "101"])
    near = defaultdict(list)
    for r in used.itertuples():
        if r.F_NODE in cross and r.T_NODE in cross and r.geometry.length <= 25:
            near[r.F_NODE].append(r.T_NODE)
            near[r.T_NODE].append(r.F_NODE)
    rows = []
    for lid in dead[dead.kind == "in"].LINK_ID:
        a = L.loc[lid]
        others = [(o, t) for o, t in all_out.get(a.T_NODE, []) if t != a.F_NODE]
        if not others:
            continue
        outs = [o for o, t in out_of.get(a.T_NODE, []) if t != a.F_NODE]
        detail = ""
        if not outs:
            cause = "이어지는 링크가 모두 ROAD_USE=1"
            detail = ",".join(o for o, _ in others)
        elif lid in rules_of:
            cause = "이어지는 사용 링크에 TURNINFO 금지"
            g = rules_of[lid]
            detail = ",".join(f"{r.ED_LINK}:{r.TURN_TYPE}" for r in g.itertuples())
        else:
            group, stack = {a.T_NODE}, [a.T_NODE]
            while stack:
                for w in near.get(stack.pop(), []):
                    if w not in group:
                        group.add(w)
                        stack.append(w)
            exits = [o for v in group for o, t in out_of.get(v, []) if t not in group]
            cause = "교차로 묶음에서 나가는 사용 링크 없음" if len(group) > 1 and not exits else "그 밖"
            detail = ",".join(sorted(group))
        lat, lon = ll(a.T_NODE)
        rows.append({"LINK_ID": lid, "도로명": a.ROAD_NAME, "등급": RANK.get(a.ROAD_RANK, a.ROAD_RANK), "F_NODE": a.F_NODE,
                     "T_NODE": a.T_NODE, "원인": cause, "관련": detail, "끝노드_위도": lat, "끝노드_경도": lon})
    stuck = pd.DataFrame(rows)
    save(stuck, "진출불가_링크")
    print("  ", stuck["원인"].value_counts().to_dict())

    rows = []
    for lid in dead[dead.kind == "isolated"].LINK_ID:
        a = L.loc[lid]
        f_in = [x for x in all_in.get(a.F_NODE, []) if L.at[x, "F_NODE"] != a.T_NODE]
        t_out = [o for o, t in all_out.get(a.T_NODE, []) if t != a.F_NODE]
        rows.append({"LINK_ID": lid, "도로명": a.ROAD_NAME, "등급": RANK.get(a.ROAD_RANK, a.ROAD_RANK), "F_NODE": a.F_NODE,
                     "T_NODE": a.T_NODE, "길이_m": round(a.LENGTH, 1), "시작노드로_들어오는_링크": len(f_in),
                     "끝노드에서_나가는_링크": len(t_out), "위도": round(mid.loc[lid].y, 7), "경도": round(mid.loc[lid].x, 7)})
    iso = pd.DataFrame(rows)
    a, b = iso["시작노드로_들어오는_링크"], iso["끝노드에서_나가는_링크"]
    print("   고립 링크 전체", len(iso), "| 양쪽 모두 이어짐(제외)", ((a > 0) & (b > 0)).sum())
    iso = iso[(a == 0) | (b == 0)].copy()
    iso.insert(5, "구분", ["양쪽 모두 이어지는 링크 없음" if x == 0 and y == 0 else "시작 노드로 들어오는 링크 없음" if x == 0
                         else "끝 노드에서 나가는 링크 없음" for x, y in zip(iso["시작노드로_들어오는_링크"], iso["끝노드에서_나가는_링크"])])
    save(iso, "고립_링크")
    print("  ", iso["구분"].value_counts().to_dict(), "| 등급", iso["등급"].value_counts().to_dict())

    ru = links[links.ROAD_USE == "1"]
    save(pd.DataFrame({"LINK_ID": ru.LINK_ID, "도로명": ru.ROAD_NAME, "등급": ru.ROAD_RANK.map(lambda r: RANK.get(r, r)),
                       "차로수": ru.LANES, "제한속도": ru.MAX_SPD, "길이_m": ru.LENGTH.round(1),
                       "위도": [round(mid.loc[x].y, 7) for x in ru.LINK_ID], "경도": [round(mid.loc[x].x, 7) for x in ru.LINK_ID]}),
         "ROAD_USE1_링크")
    print("   등급", ru.ROAD_RANK.map(lambda r: RANK.get(r, r)).value_counts().to_dict())


if __name__ == "__main__":
    main()

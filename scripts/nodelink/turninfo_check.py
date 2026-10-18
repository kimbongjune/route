"""TURNINFO 44,099건을 MOCT_LINK와 대조한다.

- TURN_TYPE, TURN_OPER 분포
- 진입 링크 ST_LINK의 T_NODE가 NODE_ID인지, 진출 링크 ED_LINK의 F_NODE가 NODE_ID인지
- 011 U-TURN의 진입·진출 링크가 ID ±100 대응링크인지, 진입 링크의 차로수와 등급, 좌회전금지와 같이 있는 노드 비율

python -X utf8 scripts/nodelink/turninfo_check.py
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from nodelink import read_links, read_nodes, read_table

TURN = {"001": "비보호회전", "002": "버스만회전", "003": "회전금지", "011": "U-TURN", "012": "P-TURN",
        "101": "좌회전금지", "102": "직진금지", "103": "우회전금지"}
LINK_COLS = ["LINK_ID", "F_NODE", "T_NODE", "LANES", "ROAD_RANK", "ROAD_USE"]


def main():
    links = read_links(LINK_COLS, geometry=False)
    turns = read_table("TURNINFO")
    print("TURNINFO", f"{len(turns):,}", "필드", list(turns.columns))
    vc = turns.TURN_TYPE.value_counts().sort_index()
    print("TURN_TYPE", " · ".join(f"{k} {TURN.get(k, '?')} {v:,}" for k, v in vc.items()))
    print("TURN_OPER", turns.TURN_OPER.value_counts().to_dict())
    print("REMARK 값 있음", (turns.REMARK != "").sum())
    print("TURN_ID 범위", turns.TURN_ID.min(), "~", turns.TURN_ID.max())

    st = links.add_prefix("ST_")
    ed = links.add_prefix("ED_")
    t = turns.merge(st, left_on="ST_LINK", right_on="ST_LINK_ID").merge(ed, left_on="ED_LINK", right_on="ED_LINK_ID")
    print("ST_LINK·ED_LINK 둘 다 MOCT_LINK에 있음", f"{len(t):,}", "없음", f"{len(turns) - len(t):,}")
    print("ST_LINK.T_NODE == NODE_ID", f"{(t.ST_T_NODE == t.NODE_ID).sum():,}")
    t["ED_F_같음"] = t.ED_F_NODE == t.NODE_ID
    print("ED_LINK.F_NODE == NODE_ID", f"{t.ED_F_같음.sum():,}")
    for ty in ("011", "101", "102", "103"):
        s = t[t.TURN_TYPE == ty]
        print(f"  {ty} {TURN[ty]}: ED_LINK.F_NODE == NODE_ID", f"{s.ED_F_같음.sum():,} / {len(s):,}")
    print("ST 또는 ED가 ROAD_USE=1", f"{((t.ST_ROAD_USE == '1') | (t.ED_ROAD_USE == '1')).sum():,}")

    # 011 진입 링크와 진출 링크의 관계
    u = t[t.TURN_TYPE == "011"]
    diff = (u.ST_LINK.astype("int64") - u.ED_LINK.astype("int64")).abs()
    vc = diff.value_counts()
    print("011", f"{len(u):,}건. ST·ED ID 차이:", " · ".join(f"{k} {v:,}" for k, v in vc.head(8).items()))
    print("  차이가 정확히 100(대응링크)", f"{(diff == 100).sum():,}", "| 95~105", f"{diff.between(95, 105).sum():,}",
          "| 권역번호(앞 3자리) 같음", f"{(u.ST_LINK.str[:3] == u.ED_LINK.str[:3]).sum():,}")
    print("  ST.F_NODE == ED.T_NODE(노드까지 뒤집힌 쌍)", (u.ST_F_NODE == u.ED_T_NODE).sum(),
          "| ST.T_NODE == ED.F_NODE(한 노드에서 이어짐)", (u.ST_T_NODE == u.ED_F_NODE).sum())
    u_nodes = set(u.NODE_ID)
    cross = (read_nodes(["NODE_TYPE"], geometry=False).NODE_TYPE == "101").sum()
    print("011 있는 노드", f"{len(u_nodes):,}", "교차로 노드", f"{cross:,}", f"({len(u_nodes) / cross:.1%})")
    print("011 진입 링크 2차로 이상", f"{(u.ST_LANES >= 2).mean():.1%}", "전체 링크 2차로 이상", f"{(links.LANES >= 2).mean():.1%}")
    print("011 진입 링크 시군도(107) 비율", f"{(u.ST_ROAD_RANK == '107').mean():.1%}", "전체", f"{(links.ROAD_RANK == '107').mean():.1%}")
    left = set(turns.loc[turns.TURN_TYPE == "101", "NODE_ID"])
    print("011 노드 중 좌회전금지(101)도 있는 비율", f"{len(u_nodes & left) / len(u_nodes):.1%}",
          "교차로 전체에서 101 있는 비율", f"{len(left) / cross:.1%}")


if __name__ == "__main__":
    main()

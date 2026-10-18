"""MOCT_NODE, MULTILINK, 내역서.csv의 건수와 분포를 집계한다.

python -X utf8 scripts/nodelink/node_stats.py
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import read_csv
from nodelink import NODE_DIR, read_nodes, read_table

TYPE = {"101": "교차로", "102": "도로 시·종점", "103": "속성변화점", "104": "도로시설물", "105": "행정경계",
        "106": "연결로접속부", "107": "최소노드배치점", "109": "기타"}


def main():
    nodes = read_nodes(["NODE_ID", "NODE_TYPE", "NODE_NAME", "TURN_P"], geometry=False)
    print("노드", f"{len(nodes):,}")
    vc = nodes.NODE_TYPE.value_counts()
    print("NODE_TYPE", " · ".join(f"{k} {TYPE.get(k, '')} {v:,}" for k, v in vc.items()))
    t = (nodes.TURN_P == "1").sum()
    print("TURN_P=1", f"{t:,}", f"({t / len(nodes):.1%})")
    print("이름 없는 노드", f"{(nodes.NODE_NAME == '').sum():,}")

    multi = read_table("MULTILINK")
    print("MULTILINK", f"{len(multi):,}", "필드", list(multi.columns))
    print("내역서.csv", len(read_csv(NODE_DIR / "내역서.csv")), "줄")


if __name__ == "__main__":
    main()

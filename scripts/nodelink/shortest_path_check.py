"""표준노드링크 전체로 scipy 다익스트라를 돌려 routedb 결과와 대조할 최단 거리를 구한다.

python -X utf8 scripts/nodelink/shortest_path_check.py
"""
import sys
from pathlib import Path

import pandas as pd
from scipy.sparse import csr_matrix
from scipy.sparse.csgraph import dijkstra

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from nodelink import read_links, read_nodes

PAIRS = [("서울역 -> 강남역사거리", "1010009300", "1210034101"),
         ("서울역 -> 부산역", "1010009300", "1320000204")]


def main():
    nodes = read_nodes(["NODE_ID"], geometry=False)
    links = read_links(["F_NODE", "T_NODE", "LENGTH"], geometry=False)
    index = pd.Series(range(len(nodes)), index=nodes.NODE_ID)
    # 같은 출발·도착 쌍이 여러 개면 가장 짧은 링크만 둔다
    edges = pd.DataFrame({"f": links.F_NODE.map(index), "t": links.T_NODE.map(index), "w": links.LENGTH})
    edges = edges.groupby(["f", "t"]).w.min().reset_index()
    graph = csr_matrix((edges.w, (edges.f, edges.t)), shape=(len(nodes), len(nodes)))
    for name, src, dst in PAIRS:
        dist = dijkstra(graph, indices=index[src])
        print(name, f"{dist[index[dst]]:,.0f}m")


if __name__ == "__main__":
    main()

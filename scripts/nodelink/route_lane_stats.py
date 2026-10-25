"""최소시간 경로가 작은 길(1차로, 시군도·지방도)을 얼마나 지나는지 전국 무작위 구간으로 잰다.

- 표준노드링크 노드에서 고정 시드로 뽑은 쌍(직선 3~60km) PAIRS개를 로컬 nav-api /route(최소시간)로 탐색한다
- 경로 링크를 표준노드링크 속성(LANES, ROAD_RANK, MAX_SPD)과 이어 도로등급·차로수별 길이 비율, 1차로 링크의 제한속도 분포,
  1차로 시군도·지방도를 500m 넘게 지나는 경로 수를 출력한다. 전체 링크의 같은 분포도 같이 보여 비교한다

python -X utf8 scripts/nodelink/route_lane_stats.py [쌍 수. 기본 300] [로컬 주소]
"""
import json
import sys
import urllib.request
from pathlib import Path

import numpy as np
import pandas as pd
import pyogrio

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
from nodelink import tm_to_wgs84

ROOT = Path(__file__).resolve().parents[2]
FOLDER = ROOT / "maps" / "[2026-09-14]NODELINKDATA"
PAIRS = int(sys.argv[1]) if len(sys.argv) > 1 else 300
LOCAL = sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:3000"
RANK_NAMES = {"101": "고속국도", "102": "도시고속", "103": "일반국도", "104": "특광역시도", "105": "국가지원지방도", "106": "지방도", "107": "시군도", "108": "기타"}


def route(a, b):
    body = json.dumps({"from": {"lat": a[0], "lon": a[1]}, "to": {"lat": b[0], "lon": b[1]}, "profile": "time"}).encode()
    req = urllib.request.Request(f"{LOCAL}/route", data=body, headers={"Content-Type": "application/json"})
    try:
        return json.load(urllib.request.urlopen(req, timeout=60))["data"]
    except urllib.error.HTTPError:
        return None


def main():
    links = pyogrio.read_dataframe(FOLDER / "MOCT_LINK.shp", read_geometry=False, encoding="cp949",
                                   columns=["LINK_ID", "ROAD_RANK", "LANES", "MAX_SPD", "LENGTH", "ROAD_USE"])
    links["LANES"] = pd.to_numeric(links.LANES, errors="coerce").fillna(0).astype(int)
    links["MAX_SPD"] = pd.to_numeric(links.MAX_SPD, errors="coerce").fillna(0).astype(int)
    links = links.set_index("LINK_ID")
    nodes = pyogrio.read_dataframe(FOLDER / "MOCT_NODE.shp", encoding="cp949", columns=["NODE_ID"])
    rng = np.random.default_rng(11)
    xs, ys = nodes.geometry.x.to_numpy(), nodes.geometry.y.to_numpy()
    pairs = []
    while len(pairs) < PAIRS:
        i, j = rng.integers(len(nodes), size=2)
        d = np.hypot(xs[i] - xs[j], ys[i] - ys[j])
        if 3000 <= d <= 60000:
            pairs.append((tm_to_wgs84(xs[i], ys[i]), tm_to_wgs84(xs[j], ys[j])))

    used = []
    heavy = 0
    found = 0
    for a, b in pairs:
        r = route(a, b)
        if not r:
            continue
        found += 1
        ids = [str(l) for l in r["links"]]
        part = links.reindex(ids).dropna(subset=["LENGTH"])
        part = part.assign(route=found)
        used.append(part)
        small = part[(part.LANES <= 1) & part.ROAD_RANK.isin(["105", "106", "107", "108"])]
        if small.LENGTH.sum() > 500:
            heavy += 1
    used = pd.concat(used)
    total = used.LENGTH.sum()
    print(f"경로 {found}개, 링크 {len(used):,}개, 길이 {total / 1000:,.0f}km")
    print("\n도로등급별 길이 비율(경로 | 전체 링크)")
    all_by_rank = links.groupby("ROAD_RANK").LENGTH.sum() / links.LENGTH.sum()
    for rank, share in (used.groupby("ROAD_RANK").LENGTH.sum() / total).sort_index().items():
        print(f"  {RANK_NAMES.get(rank, rank):8s} {share:6.1%} | {all_by_rank.get(rank, 0):6.1%}")
    one = used[used.LANES <= 1]
    print(f"\n1차로 링크: 경로 길이의 {one.LENGTH.sum() / total:.1%} (전체 링크는 {links[links.LANES <= 1].LENGTH.sum() / links.LENGTH.sum():.1%})")
    print("  1차로 링크 등급별:", {RANK_NAMES.get(k, k): f"{v / 1000:.0f}km" for k, v in one.groupby("ROAD_RANK").LENGTH.sum().items()})
    print("  경로 위 1차로 링크 제한속도 분포(길이 km):", {int(k): round(v / 1000) for k, v in one.groupby("MAX_SPD").LENGTH.sum().items()})
    all_one = links[links.LANES <= 1]
    print("  전체 1차로 링크 제한속도 분포(길이 km):", {int(k): round(v / 1000) for k, v in all_one.groupby("MAX_SPD").LENGTH.sum().items()})
    print(f"\n1차로 시군도·지방도를 500m 넘게 지나는 경로 {heavy}개 / {found}개")
    print("\n차로수별 제한속도 중앙값(전체 링크):", links.groupby("LANES").MAX_SPD.median().to_dict())


if __name__ == "__main__":
    main()

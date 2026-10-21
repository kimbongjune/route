"""MAX_SPD=10 링크의 분포를 세고, MAX_SPD별로 1차로 링크를 카카오 길찾기 소요시간으로 본 속도와 비교한다.

- ROAD_USE=1을 뺀 MAX_SPD=10 링크의 도로등급, 차로수, 교차로 노드(101) 사이 25m 이하 링크 비율, 같은 도로명에 다른 속도가 섞인 도로 수
- 300m 이상 1차로 링크를 MAX_SPD·도로등급별로 고정 시드 30개씩 골라 링크 시작 30m 뒤에서 끝 30m 앞까지 카카오 거리 우선 경로를 구한다
- 카카오 거리가 링크 길이에서 60m를 뺀 값과 20% 넘게 다르면 다른 길로 간 것으로 보고 뺀다
- 카카오 거리 / 소요시간으로 본 속도와 도로별 traffic_speed의 중앙값

python -X utf8 scripts/nodelink/max_spd_kakao_check.py
"""
import json
import sys
import time
import urllib.request
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
from common import env
from nodelink import read_links, read_nodes, tm_to_wgs84

GROUPS = [(10, "107"), (20, "107"), (30, "107"), (40, "107"), (50, "107"), (60, "107"), (10, "104"), (30, "104")]


def kakao(key, a, b):
    """카카오 거리 우선 경로의 (거리 m, 소요시간 초, 도로별 traffic_speed)"""
    url = (f"https://apis-navi.kakaomobility.com/v1/directions?origin={a[1]},{a[0]}&destination={b[1]},{b[0]}"
           "&priority=DISTANCE&summary=false")
    req = urllib.request.Request(url, headers={"Authorization": "KakaoAK " + key})
    r = json.load(urllib.request.urlopen(req, timeout=30))["routes"][0]
    if r["result_code"] != 0:
        return None
    return r["summary"]["distance"], r["summary"]["duration"], [rd["traffic_speed"] for s in r["sections"] for rd in s["roads"]]


def distribution():
    links = read_links(["LINK_ID", "F_NODE", "T_NODE", "ROAD_RANK", "LANES", "ROAD_NAME", "MAX_SPD", "ROAD_USE", "LENGTH"], geometry=False)
    links = links[links.ROAD_USE != "1"]
    nodes = read_nodes(["NODE_ID", "NODE_TYPE"], geometry=False)
    cross = set(nodes.NODE_ID[nodes.NODE_TYPE == "101"])
    inner = links.F_NODE.isin(cross) & links.T_NODE.isin(cross) & (links.LENGTH <= 25)
    s10 = links.MAX_SPD == 10
    t = links[s10]
    print("MAX_SPD=10", f"{len(t):,}", "| 도로등급", t.ROAD_RANK.value_counts().to_dict(), f"| 1차로 {(t.LANES <= 1).mean():.1%}")
    print(f"교차로 노드 사이 25m 이하 링크 중 10 비율 {s10[inner].mean():.1%} | 그 밖 링크 중 10 비율 {s10[~inner].mean():.1%}")
    named = t[~t.ROAD_NAME.isin(["", "-"])].ROAD_NAME.unique()
    mixed = links[links.ROAD_NAME.isin(named) & ~s10].ROAD_NAME.nunique()
    print(f"10 링크의 도로명 {len(named):,} | 그중 다른 속도 링크도 있는 도로명 {mixed:,}")
    small = links[(links.ROAD_RANK == "107") & (links.LANES <= 1)]
    print("1차로 시군도 MAX_SPD별 링크 수", small.MAX_SPD.value_counts().sort_index().to_dict())


def main():
    distribution()
    key = env("KAKAO_REST_API_KEY")
    links = read_links(["LINK_ID", "ROAD_RANK", "LANES", "MAX_SPD", "ROAD_USE", "LENGTH"])
    links = links[(links.ROAD_USE != "1") & (links.LENGTH >= 300) & (links.LANES <= 1)]
    for spd, rank in GROUPS:
        sample = links[(links.MAX_SPD == spd) & (links.ROAD_RANK == rank)].sample(30, random_state=5)
        rows = []
        for geo in sample.geometry:
            p, q = geo.interpolate(30), geo.interpolate(geo.length - 30)
            k = kakao(key, tm_to_wgs84(p.x, p.y), tm_to_wgs84(q.x, q.y))
            time.sleep(0.05)
            if k is None:
                continue
            d, t, speeds = k
            if abs(d - (geo.length - 60)) > 0.2 * (geo.length - 60) or not t:
                continue
            rows.append((d / t * 3.6, np.median(speeds)))
        v = np.array(rows)
        print(f"MAX_SPD={spd} 도로등급 {rank}: 비교 {len(v)}개 | 소요시간으로 본 속도 중앙값 {np.median(v[:, 0]):.1f}km/h"
              f" 25% {np.percentile(v[:, 0], 25):.1f} 75% {np.percentile(v[:, 0], 75):.1f} | traffic_speed 중앙값 {np.median(v[:, 1]):.1f}")


if __name__ == "__main__":
    main()

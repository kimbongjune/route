"""용산구 골목 실험 평가: 골목 주소 사이 경로를 로컬 nav-api와 카카오(최소시간)로 비교한다.

- 출발·도착은 용산구 골목 도로명주소를 로컬 지오코더로 좌표로 바꾼 것이다(PAIRS)
- 로컬 경로의 끝이 도착 좌표에서 얼마나 떨어졌는지(스냅 거리), 거리, 경로 겹침(30m)을 출력한다
- 골목을 넣기 전과 후에 같은 명령으로 돌려 비교한다. 카카오 응답은 저장하지 않는다

python -X utf8 scripts/alley/yongsan_compare.py [로컬 주소]
"""
import math
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "nodelink"))
import kakao_time_compare as k  # noqa: E402

PAIRS = [
    ("서울 용산구 원효로89길 39", "서울 용산구 효창원로104라길 35"),
    ("서울 용산구 소월로40길 71", "서울 용산구 대사관로6가길 42"),
    ("서울 용산구 효창원로42길 18", "서울 용산구 보광로12가길 18"),
    ("서울 용산구 이태원로27가길 46", "서울 용산구 이태원로14길 50"),
    ("서울 용산구 한남대로27가길 24", "서울 용산구 원효로58길 10"),
    ("서울 용산구 녹사평대로40다길 23", "서울 용산구 독서당로6길 29"),
    ("서울 용산구 두텁바위로1길 78", "서울 용산구 회나무로44가길 63"),
    ("서울 용산구 녹사평대로26가길 29", "서울 용산구 효창원로26가길 20"),
    ("서울 용산구 신흥로11길 3", "서울 용산구 청파로57길 23"),
    ("서울 용산구 장문로49길 7", "서울 용산구 녹사평대로26길 73"),
]


def dist_m(a, b):
    lat = math.radians((a[0] + b[0]) / 2)
    return math.hypot(math.radians(a[0] - b[0]) * 6371000, math.radians(a[1] - b[1]) * 6371000 * math.cos(lat))


def main():
    key = k.env("KAKAO_REST_API_KEY")
    rows = []
    for a, b in PAIRS:
        pa, pb = k.geocode(a), k.geocode(b)
        if not pa or not pb:
            continue
        mine = k.local_route(pa[:2], pb[:2])
        theirs = k.kakao_route(key, pa[:2], pb[:2])
        if not mine or not theirs:
            print(f"{a}→{b}: {'로컬' if not mine else '카카오'} 경로 없음")
            continue
        start_gap = dist_m(pa[:2], tuple(mine["geometry"][0]))
        end_gap = dist_m(pb[:2], tuple(mine["geometry"][-1]))
        my_line = k.to_tm([(lon, lat) for lat, lon in mine["geometry"]])
        k_line = k.to_tm(theirs["coords"])
        in_k = my_line.intersection(k_line.buffer(k.BUFFER_M)).length / my_line.length if my_line.length else 0
        in_m = k_line.intersection(my_line.buffer(k.BUFFER_M)).length / k_line.length if k_line.length else 0
        rows.append((start_gap, end_gap, mine["distance_m"], theirs["distance"], in_k, in_m))
        print(f"{a.replace('서울 용산구 ', '')}→{b.replace('서울 용산구 ', '')}: 출발 스냅 {start_gap:.0f}m 도착 스냅 {end_gap:.0f}m | "
              f"거리 로컬 {mine['distance_m'] / 1000:.2f}km 카카오 {theirs['distance'] / 1000:.2f}km | 겹침 로컬→카카오 {in_k:.0%} 카카오→로컬 {in_m:.0%}")
    r = np.array(rows)
    print(f"\n{len(r)}쌍 | 출발 스냅 중앙값 {np.median(r[:, 0]):.0f}m 최대 {r[:, 0].max():.0f}m | 도착 스냅 중앙값 {np.median(r[:, 1]):.0f}m 최대 {r[:, 1].max():.0f}m | "
          f"거리 차이 중앙값 {np.median((r[:, 2] - r[:, 3]) / r[:, 3]):+.0%} | 겹침 중앙값 로컬→카카오 {np.median(r[:, 4]):.0%} 카카오→로컬 {np.median(r[:, 5]):.0%}")


if __name__ == "__main__":
    main()

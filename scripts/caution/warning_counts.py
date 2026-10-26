"""로컬 nav-api 최소시간 경로의 경고를 종류별로 세고, 돌발(incident)·사고다발(accident_zone)·어린이보호구역(school_zone) 경고 문구를 출력한다.

- 경로는 서울역→강남역, 서울역→부산역, 서울역→강릉역, 수원역→서울역, 일산 백석역→판교역이다
- 경로마다 거리, 시간(분), 종류별 경고 수, 사고다발 구간 수와 그중 사고 건수 합이 5건 이상인 구간 수를 출력한다

python -X utf8 scripts/caution/warning_counts.py [로컬 주소. 기본 http://127.0.0.1:3000]
"""
import json
import sys
import urllib.request
from collections import Counter

LOCAL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000"
P = {"서울역": (37.5547, 126.9707), "강남역": (37.4979, 127.0276), "부산역": (35.1151, 129.0422),
     "강릉역": (37.7640, 128.8996), "수원역": (37.2664, 127.0002), "일산 백석역": (37.6431, 126.7880),
     "판교역": (37.3948, 127.1112)}
PAIRS = [("서울역", "강남역"), ("서울역", "부산역"), ("서울역", "강릉역"), ("수원역", "서울역"), ("일산 백석역", "판교역")]
SHOW = ("incident", "accident_zone", "school_zone")


def route(a, b):
    body = json.dumps({"from": {"lat": P[a][0], "lon": P[a][1]}, "to": {"lat": P[b][0], "lon": P[b][1]},
                       "profile": "time"}).encode()
    req = urllib.request.Request(f"{LOCAL}/route", data=body, headers={"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(req, timeout=120))["data"]


def main():
    for a, b in PAIRS:
        d = route(a, b)
        kinds = Counter(w["kind"] for w in d["warnings"])
        sections = d["accident_sections"]
        counted = sum(1 for x in sections if sum(z["accidents"] for z in x["zones"]) >= 5)
        print(f"{a}→{b} {d['distance_m'] / 1000:.1f}km {d['duration_s'] / 60:.1f}분 경고 {len(d['warnings'])}건, "
              f"사고다발 구간 {len(sections)}개 중 사고 5건 이상 {counted}개")
        print("  ", dict(kinds.most_common()))
        for w in d["warnings"]:
            if w["kind"] in SHOW:
                print(f"   {w['at_m'] / 1000:7.1f}km {w['kind']} {w['text'][:90]}")


if __name__ == "__main__":
    main()

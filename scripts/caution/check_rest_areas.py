"""로컬 nav-api의 최소시간 경로에서 휴게소 안내(rest_area 경고)를 확인한다.

- 서울역→부산역, 부산역→서울역, 서울역→강릉역, 서울역→광주송정역 경로의 휴게소 안내를 경로 위 거리 순서로 출력한다
- 서울역→부산역 경로에서 1.5km 안의 휴게소를 진행 방향 왼쪽(L)·오른쪽(R)과 함께 출력하여 빠진 휴게소를 확인한다
- 경로의 도로 이름 구간(segments)을 출력한다

python -X utf8 scripts/caution/check_rest_areas.py [로컬 주소. 기본 http://127.0.0.1:3000]
"""
import csv
import json
import math
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOCAL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000"
P = {"서울역": (37.5547, 126.9707), "부산역": (35.1151, 129.0422), "강릉역": (37.7640, 128.8996),
     "광주송정역": (35.1374, 126.7930)}


def route(a, b):
    body = json.dumps({"from": {"lat": P[a][0], "lon": P[a][1]}, "to": {"lat": P[b][0], "lon": P[b][1]},
                       "profile": "time"}).encode()
    req = urllib.request.Request(f"{LOCAL}/route", data=body, headers={"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(req, timeout=120))["data"]


def near(d, rows, within_m):
    """경로에서 within_m 안의 휴게소마다 (경로 위 거리, 거리, 좌우, 이름)"""
    pts = [tuple(p) if isinstance(p, list) else (p["lat"], p["lon"]) for p in d["geometry"]]
    cum = [0.0]
    for a, b in zip(pts, pts[1:]):
        cum.append(cum[-1] + math.hypot((b[1] - a[1]) * 88_000, (b[0] - a[0]) * 110_950))
    scale = d["distance_m"] / cum[-1]
    out = []
    for r in rows:
        la, lo = float(r["lat"]), float(r["lon"])
        cos = math.cos(math.radians(la))
        best = None
        for i, (a, b) in enumerate(zip(pts, pts[1:])):
            ax, ay = (a[1] - lo) * 111_320 * cos, (a[0] - la) * 110_950
            dx, dy = (b[1] - a[1]) * 111_320 * cos, (b[0] - a[0]) * 110_950
            n = dx * dx + dy * dy
            if n == 0:
                continue
            t = max(0.0, min(1.0, -(ax * dx + ay * dy) / n))
            dd = math.hypot(ax + t * dx, ay + t * dy)
            if best is None or dd < best[0]:
                side = "R" if dx * (-ay) - dy * (-ax) < 0 else "L"
                best = (dd, (cum[i] + t * (cum[i + 1] - cum[i])) * scale, side)
        if best and best[0] < within_m:
            out.append((best[1], best[0], best[2], r["name"]))
    return sorted(out)


def main():
    for a, b in [("서울역", "부산역"), ("부산역", "서울역"), ("서울역", "강릉역"), ("서울역", "광주송정역")]:
        d = route(a, b)
        w = [x for x in d["warnings"] if x["kind"] == "rest_area"]
        print(f"{a}→{b} {d['distance_m'] / 1000:.1f}km 휴게소 안내 {len(w)}곳")
        for x in w:
            print(f"  {x['at_m'] / 1000:7.1f}km {x['text']}")
    d = route("서울역", "부산역")
    rows = list(csv.DictReader(open(ROOT / "maps" / "유료도로" / "휴게소.csv", encoding="utf-8-sig")))
    print("서울역→부산역 경로 1.5km 안의 휴게소")
    for at, dist, side, name in near(d, rows, 1_500):
        print(f"  {at / 1000:7.1f}km {dist:5.0f}m {side} {name}")
    print("서울역→부산역 도로 구간")
    for s in d["segments"]:
        print(f"  {s['from_m'] / 1000:7.1f}km 등급 {s['rank']} {s['name']}")


if __name__ == "__main__":
    main()

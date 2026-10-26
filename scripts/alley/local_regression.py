"""큰길·골목 통합 탐색(3km 이하)을 고치기 전후의 경로를 비교한다.

- 서울(경도 126.85~127.15, 위도 37.45~37.65) 안에서 무작위로(seed 42) 3km 이하 쌍 300개와 5~20km 쌍 30개를 만든다
- 쌍마다 최소시간(time)·최단거리(distance) 경로의 링크 열, 거리, 시간을 save로 저장하고 compare로 같은지 비교한다
- compare는 바뀐 경로 수와, 바뀐 경로마다 수정 전 경로에 같은 링크가 연달아 있었는지(되돌아가기), 수정 전후 거리·시간을 출력한다

python -X utf8 scripts/alley/local_regression.py save <저장 파일> [로컬 주소]
python -X utf8 scripts/alley/local_regression.py compare <저장 파일> [로컬 주소]
"""
import json
import math
import random
import sys
import urllib.error
import urllib.request

PROFILES = ["time", "distance"]


def pairs():
    rnd = random.Random(42)
    out = []
    for n, (lo, hi) in [(300, (300, 3000)), (30, (5000, 20000))]:
        for _ in range(n):
            lat, lon = rnd.uniform(37.45, 37.65), rnd.uniform(126.85, 127.15)
            d, a = rnd.uniform(lo, hi), rnd.uniform(0, 2 * math.pi)
            out.append(((lat, lon), (lat + d * math.cos(a) / 110_950, lon + d * math.sin(a) / 88_000)))
    return out


def route(local, a, b, profile):
    body = json.dumps({"from": {"lat": a[0], "lon": a[1]}, "to": {"lat": b[0], "lon": b[1]}, "profile": profile}).encode()
    req = urllib.request.Request(f"{local}/route", data=body, headers={"Content-Type": "application/json"})
    try:
        d = json.load(urllib.request.urlopen(req, timeout=120))["data"]
    except urllib.error.HTTPError as e:
        return {"error": e.code}
    return {"links": d["links"], "distance_m": round(d["distance_m"], 3), "duration_s": round(d["duration_s"], 3)}


def run(local):
    return {f"{i}:{p}": route(local, a, b, p) for i, (a, b) in enumerate(pairs()) for p in PROFILES}


def main():
    mode, path = sys.argv[1], sys.argv[2]
    local = sys.argv[3] if len(sys.argv) > 3 else "http://127.0.0.1:3000"
    now = run(local)
    if mode == "save":
        json.dump(now, open(path, "w", encoding="utf-8"))
        print(f"저장 {len(now)}개, 경로 없음 {sum(1 for r in now.values() if 'error' in r)}개")
        return
    old = json.load(open(path, encoding="utf-8"))
    changed = [k for k in old if old[k] != now[k]]
    print(f"경로 {len(old)}개 중 바뀐 것 {len(changed)}개")
    for k in changed:
        o, n = old[k], now[k]
        links = o.get("links", [])
        dup = sorted({links[i] for i in range(1, len(links)) if links[i] == links[i - 1]})
        print(f"  {k} 수정 전 {o.get('distance_m')}m {o.get('duration_s')}초 → 수정 후 {n.get('distance_m')}m {n.get('duration_s')}초,"
              f" 수정 전 연달아 같은 링크 {dup}")


if __name__ == "__main__":
    main()

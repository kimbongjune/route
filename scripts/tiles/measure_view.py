"""한 화면을 그리는 데 필요한 타일을 자체 지도와 브이월드로 받아 개수·용량·시간을 비교한다.

- 화면은 가로 1568, 세로 579픽셀(256픽셀 타일)이고 중심은 서울역이다. 줌은 인자로 받는다(기본 11 14 16)
- 자체 지도는 rust/nav/web/style-day.json에서 그 줌에 보이는 레이어가 있는 소스마다 요청할 타일(소스 maxzoom 위는 maxzoom 타일)을 구한다
- 브라우저처럼 동시에 6개씩 받으며, 용량은 받은 그대로와 gzip으로 줄였을 때를 함께 센다
- 같은 타일을 두 번 받아 두 번째(서버 캐시) 시간을 함께 출력한다

python -X utf8 scripts/tiles/measure_view.py [로컬 주소] [줌 ...]
"""
import gzip
import json
import math
import sys
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOCAL = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1].startswith("http") else "http://127.0.0.1:3000"
ZOOMS = [int(z) for z in sys.argv[1:] if z.isdigit()] or [11, 14, 16]
CENTER = (126.9707, 37.5547)
WIDTH, HEIGHT = 1568, 579


def tile_range(z):
    n = 2 ** z
    x = (CENTER[0] + 180) / 360 * n
    lat = math.radians(CENTER[1])
    y = (1 - math.log(math.tan(lat) + 1 / math.cos(lat)) / math.pi) / 2 * n
    dx, dy = WIDTH / 256 / 2, HEIGHT / 256 / 2
    return [(xx, yy) for xx in range(int(x - dx), int(x + dx) + 1) for yy in range(int(y - dy), int(y + dy) + 1)]


def fetch(url):
    t = time.perf_counter()
    try:
        body = urllib.request.urlopen(url, timeout=60).read()
    except urllib.error.HTTPError as e:
        body = b"" if e.code in (204, 400, 404) else e.read()
    return len(body), len(gzip.compress(body)) if body else 0, time.perf_counter() - t


def measure(urls):
    started = time.perf_counter()
    with ThreadPoolExecutor(6) as ex:
        res = list(ex.map(fetch, urls))
    wall = time.perf_counter() - started
    return len(urls), sum(r[0] for r in res), sum(r[1] for r in res), wall


def main():
    style = json.loads((ROOT / "rust/nav/web/style-day.json").read_text(encoding="utf-8").replace("{origin}", LOCAL))
    for z in ZOOMS:
        own = []
        # 이 줌에서 보이는 레이어가 있는 소스만 요청한다(MapLibre도 그렇게 한다)
        used = {l["source"] for l in style["layers"] if "source" in l and l.get("minzoom", 0) <= z < l.get("maxzoom", 99)}
        for name, src in style["sources"].items():
            lo, hi = src.get("minzoom", 0), src.get("maxzoom", 22)
            if z < lo or name not in used:
                continue
            sz = min(z, hi)
            tiles = {(x >> (z - sz), y >> (z - sz)) for x, y in tile_range(z)}
            own += [src["tiles"][0].format(z=sz, x=x, y=y) for x, y in tiles]
        vworld = [f"{LOCAL}/tiles/base/{z}/{x}/{y}" for x, y in tile_range(z)]
        for label, urls in (("자체 지도", own), ("브이월드", vworld)):
            first = measure(urls)
            second = measure(urls)
            print(f"줌 {z} {label}: 타일 {first[0]}개, {first[1] / 1024:,.0f}KB(gzip {first[2] / 1024:,.0f}KB), "
                  f"받기 {first[3]:.2f}초, 다시 받기 {second[3]:.2f}초")


if __name__ == "__main__":
    main()

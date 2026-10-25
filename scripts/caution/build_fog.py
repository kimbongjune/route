"""안개취약 구간(maps/주의운전/안개취약.csv, CP949)을 nav-api가 읽는 구간 CSV로 만든다.

- 원본은 고속도로 노선, 이정, 구간("대관령IC~강릉SA(13km)")만 있고 좌표가 없다. 파일 이름이 자모가 분리된 형태(NFD)여서 이름을 정규화하여 찾는다
- 구간 양 끝 이름을 로컬 nav-api 지오코더(/geocode)로 찾는다. SA는 휴게소, TN은 터널로 고쳐 찾고,
  이름이 하나뿐인 서해대교는 "서해대교(당진측)"과 "서해대교(평택측)"을 양 끝으로 한다(ENDPOINTS)
- 양 끝 사이 최소시간 경로(/route)를 두 방향으로 구하고, 경로 링크 중 표준노드링크 고속국도(ROAD_RANK 101) 본선(CONNECT 0)이
  가장 길게 이어지는 부분을 그 방향의 안개 구간으로 한다. 시작점·끝점은 그 부분 첫 링크의 첫 점과 마지막 링크의 끝 점이다
- 출력: maps/주의운전/안개구간.csv  컬럼 id, kind, region, road, length_m, start_lat, start_lon, end_lat, end_lon.
  방향마다 한 행이며 같은 구간의 두 방향은 id가 같다
- 구간마다 원본 길이와 방향별 본선 길이를 출력한다

python -X utf8 scripts/caution/build_fog.py [로컬 주소. 기본 http://127.0.0.1:3000]
"""
import json
import re
import sys
import unicodedata
import urllib.parse
import urllib.request
from pathlib import Path

import pandas as pd
import pyogrio

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
from common import ROOT, write_csv
from nodelink import NODE_DIR, tm_to_wgs84

LOCAL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000"
FOLDER = ROOT / "maps" / "주의운전"
MOCT_LINK = NODE_DIR / "MOCT_LINK.shp"
ENDPOINTS = {"서해대교": ("서해대교(당진측)", "서해대교(평택측)")}


def find(name):
    for p in FOLDER.iterdir():
        if unicodedata.normalize("NFC", p.name) == name:
            return p
    raise SystemExit(f"{name} 이 없습니다")


def geocode(name):
    q = name.replace("SA", "휴게소").replace("TN", "터널")
    items = json.load(urllib.request.urlopen(f"{LOCAL}/geocode?" + urllib.parse.urlencode({"q": q, "size": 1})))["data"]["items"]
    if not items:
        raise SystemExit(f"지오코더에 없음: {q}")
    return items[0]["lat"], items[0]["lon"], items[0]["title"]


def route_links(a, b):
    body = json.dumps({"from": {"lat": a[0], "lon": a[1]}, "to": {"lat": b[0], "lon": b[1]}, "profile": "time"}).encode()
    req = urllib.request.Request(f"{LOCAL}/route", data=body, headers={"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(req, timeout=120))["data"]["links"]


def main():
    src = pd.read_csv(find("안개취약.csv"), encoding="cp949")
    links = pyogrio.read_dataframe(MOCT_LINK, encoding="cp949", columns=["LINK_ID", "ROAD_RANK", "CONNECT", "LENGTH"])
    links["LINK_ID"] = links.LINK_ID.astype("int64")
    links = links.set_index("LINK_ID")
    rows = []
    for i, r in src.iterrows():
        m = re.match(r"(.+?)(?:~(.+?))?\((\d+)km\)", r["구간"])
        first, second, km = m.group(1), m.group(2), int(m.group(3))
        names = (first, second) if second else ENDPOINTS[first]
        a, b = geocode(names[0]), geocode(names[1])
        sid = f"안개{i + 1}"
        lengths = []
        for p, q in [(a, b), (b, a)]:
            ids = [l for l in route_links(p, q) if l in links.index]
            best, cur = [], []
            for l in ids:
                row = links.loc[l]
                if row.ROAD_RANK == "101" and row.CONNECT == "0":
                    cur.append(l)
                    if len(cur) > len(best):
                        best = list(cur)
                else:
                    cur = []
            if not best:
                raise SystemExit(f"{r['구간']}: 고속국도 본선이 없음")
            sx, sy = links.loc[best[0]].geometry.coords[0]
            ex, ey = links.loc[best[-1]].geometry.coords[-1]
            (slat, slon), (elat, elon) = tm_to_wgs84(sx, sy), tm_to_wgs84(ex, ey)
            length = float(links.loc[best].LENGTH.astype(float).sum())
            lengths.append(length)
            rows.append({"id": sid, "kind": "고속국도", "region": r["구간"], "road": r["노선"], "length_m": round(length, 1),
                         "start_lat": round(slat, 7), "start_lon": round(slon, 7), "end_lat": round(elat, 7), "end_lon": round(elon, 7)})
        print(f"{sid} {r['노선']} {r['구간']}: {a[2]} ~ {b[2]} | 원본 {km}km, 본선 {lengths[0] / 1000:.1f}km / {lengths[1] / 1000:.1f}km")
    write_csv(FOLDER / "안개구간.csv", pd.DataFrame(rows))
    print(f"저장 {len(rows)}행")


if __name__ == "__main__":
    main()

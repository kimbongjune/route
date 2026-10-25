"""UTIC TCS 공사정보(tcsOpenData, CSV)를 받아 위치를 찾은 공사만 nav-api가 읽는 공사구간 CSV로 만든다.

- 호출: http://www.utic.go.kr/guide/tcsOpenData.do?key=... (키는 .env의 UTIC_API_KEY, 등록 IP 대역에서만 동작). 매일 07·10·13시 30분에 갱신된다
- 원본에는 좌표가 없고 공사도로명(constRoad)이 "간선도로", "이면도로", "국도"처럼 일반 명칭인 행이 많다.
  공사도로명에 "동·리·가·로·길 + 번호" 꼴의 주소가 있는 행만 로컬 지오코더(/geocode)로 찾는다.
  공사도로명의 첫 쉼표 앞 문자열, 그 안의 주소 꼴 부분 순서로 찾고 결과가 주소(address)이며
  등록 경찰서(poliName + 경찰서를 지오코더로 찾은 위치)에서 STATION_M 안일 때만 받아들인다
- 필드가 12개가 아닌 행(값 안 줄바꿈으로 깨진 행), 시작·종료일이 20YYMMDD가 아닌 행, 종료일이 실행일 전인 행은 뺀다.
  진행 여부는 nav-api가 요청 날짜로 다시 판단한다
- 출력: maps/주의운전/공사구간.csv  컬럼 id, police, road, name, control, from_date, to_date, lat, lon, address
- 단계별 행 수를 출력한다

python -X utf8 scripts/caution/build_construction.py [로컬 주소. 기본 http://127.0.0.1:3000]
"""
import csv
import datetime
import io
import json
import math
import re
import sys
import urllib.parse
import urllib.request
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, env, http_get, write_csv

LOCAL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000"
OUT = ROOT / "maps" / "주의운전" / "공사구간.csv"
STATION_M = 30_000
# 숫자 뒤에 길·번길·가길·로가 이어지면 "효령로29길"처럼 도로 이름의 일부이므로 번호로 보지 않는다
ADDRESS = re.compile(r"([가-힣0-9]+(?:동|리|가|로|길)\s*\d+(?:-\d+)?)(?!\d|-|번?길|가길|로)")


def geocode(q, size=3):
    url = f"{LOCAL}/geocode?" + urllib.parse.urlencode({"q": q, "size": size})
    return json.load(urllib.request.urlopen(url, timeout=60))["data"]["items"]


def dist(a, b):
    return math.hypot((a[0] - b[0]) * 110_950, (a[1] - b[1]) * 111_320 * math.cos(math.radians(a[0])))


def main():
    key = env("UTIC_API_KEY")
    text = http_get(f"http://www.utic.go.kr/guide/tcsOpenData.do?key={key}", timeout=300, secrets=(key,)).decode("utf-8-sig")
    rows = list(csv.reader(io.StringIO(text)))
    head, body = rows[0], rows[1:]
    good = [dict(zip(head, r)) for r in body if len(r) == 12]
    today = datetime.date.today().strftime("%Y%m%d")
    dated = [r for r in good if re.fullmatch(r"20\d{6}", r["constFromDate"]) and re.fullmatch(r"20\d{6}", r["constToDate"])
             and r["constToDate"] >= today]
    addr = [r for r in dated if ADDRESS.search(r["constRoad"])]
    stations, out = {}, []
    for r in addr:
        name = r["poliName"]
        if name not in stations:
            hit = [i for i in geocode(f"{name}경찰서") if "경찰서" in i["title"]]
            stations[name] = (hit[0]["lat"], hit[0]["lon"]) if hit else None
        st = stations[name]
        if st is None:
            continue
        cands = [r["constRoad"].split(",")[0].strip(), ADDRESS.search(r["constRoad"]).group(1)]
        found = next((i for q in cands for i in geocode(q) if i["kind"] == "address" and dist((i["lat"], i["lon"]), st) <= STATION_M), None)
        if found:
            out.append({"id": r["seq"], "police": name, "road": r["constRoad"].strip(), "name": r["constName"].strip(),
                        "control": r["trafficControl"].strip(), "from_date": r["constFromDate"], "to_date": r["constToDate"],
                        "lat": found["lat"], "lon": found["lon"], "address": found["title"]})
    write_csv(OUT, pd.DataFrame(out))
    print(f"원본 {len(body)}행, 필드 12개 {len(good)}, 날짜 형식이 맞고 {today} 이후 끝나는 행 {len(dated)}, 주소 꼴 {len(addr)}, "
          f"경찰서 {sum(1 for v in stations.values() if v)}/{len(stations)}곳 위치 찾음, 저장 {len(out)}행")


if __name__ == "__main__":
    main()

"""한국도로공사 휴게소 위치·주유소·편의시설을 nav-api가 읽는 휴게소 CSV로 만든다.

- 휴게소 위치: data.ex.co.kr/openapi/locationinfo/locationinfoRest (휴게소명, 노선, 좌표, serviceAreaCode)
- 주유소: restinfo/hiwaySvarInfoList 중 시설 구분이 주유소인 시설. 이름의 "주유소"를 "휴게소"로 바꾼 이름이 같은 휴게소에 주유소가 있다고 본다
- 편의시설: business/conveniServiceArea의 convenience(예: 수유실|쉼터)를 serviceAreaCode로 잇는다
- 키는 .env의 EX_ROAD_API_KEY이다
- 편의시설 API는 일부 행에서 오류 페이지를 돌려주므로 그 쪽은 한 건씩 받아 오류 행만 뺀다
- 출력: maps/유료도로/휴게소.csv  컬럼 code, name, route, lat, lon, gas, convenience
- 휴게소 수, 주유소가 있는 휴게소 수, 편의시설을 이은 휴게소 수를 출력한다

python -X utf8 scripts/caution/build_rest_areas.py
"""
import json
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, env, http_get, write_csv

OUT = ROOT / "maps" / "유료도로" / "휴게소.csv"


def get_json(key, path, rows, page):
    """한 쪽을 받는다. 응답이 JSON이 아니면(오류 페이지) 한 번 더 받고 그래도 아니면 None"""
    url = f"https://data.ex.co.kr/openapi/{path}?key={key}&type=json&numOfRows={rows}&pageNo={page}"
    for _ in range(2):
        try:
            return json.loads(http_get(url, secrets=(key,)))
        except json.JSONDecodeError:
            pass
    return None


def fetch_all(key, path):
    """전체 건수(count)를 99건씩 나누어 받는다. 오류 페이지가 오는 쪽은 한 건씩 다시 받고, 한 건도 오류이면 그 행을 빼고 출력한다"""
    rows, page, pages = [], 1, 1
    while page <= pages:
        d = get_json(key, path, 99, page)
        if d is None:
            for k in range((page - 1) * 99 + 1, page * 99 + 1):
                one = get_json(key, path, 1, k)
                if one is None:
                    print(f"  {path} {k}번째 행 응답이 JSON이 아니어서 뺌")
                else:
                    rows += one.get("list", [])
        else:
            rows += d.get("list", [])
            pages = -(-int(d.get("count") or 0) // 99)
        page += 1
    return rows


def main():
    key = env("EX_ROAD_API_KEY")
    rest = fetch_all(key, "locationinfo/locationinfoRest")
    facilities = fetch_all(key, "restinfo/hiwaySvarInfoList")
    conveni = {c["serviceAreaCode"]: c for c in fetch_all(key, "business/conveniServiceArea")}
    gas_names = {f["svarNm"].replace("주유소", "휴게소") for f in facilities if f.get("svarGsstClssNm") == "주유소"}
    out = []
    for r in rest:
        if not r.get("xValue") or not r.get("yValue"):
            continue
        c = conveni.get(r.get("serviceAreaCode"), {})
        out.append({"code": r["stdRestCd"], "name": r["unitName"], "route": r["routeName"], "lat": float(r["yValue"]),
                    "lon": float(r["xValue"]), "gas": "Y" if r["unitName"] in gas_names else "N",
                    "convenience": (c.get("convenience") or "").replace("|", "·")})
    df = pd.DataFrame(out).drop_duplicates("code")
    write_csv(OUT, df)
    print(f"휴게소 위치 {len(rest)}건, 저장 {len(df)}곳, 주유소가 있는 휴게소 {(df.gas == 'Y').sum()}곳, "
          f"편의시설을 이은 휴게소 {(df.convenience != '').sum()}곳")


if __name__ == "__main__":
    main()

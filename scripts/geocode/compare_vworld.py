"""로컬 지오코더(/geocode)와 브이월드 검색 API의 첫 결과를 비교한다.

검색어 40개(주소 20, 장소 20)를 양쪽에 보내 첫 결과의 거리(m)를 구한다. 브이월드는 주소는 type=ADDRESS,
장소는 type=PLACE로 호출하며 결과는 저장하지 않고 화면에만 출력한다. 키는 .env의 VWORLD_API_KEY이다.

python -X utf8 scripts/geocode/compare_vworld.py [로컬 주소. 기본 http://127.0.0.1:3000]
"""
import json
import math
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOCAL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000"
VWORLD = "https://api.vworld.kr/req/search"

ADDRESSES = [
    "서울특별시 중구 세종대로 110", "서울 용산구 한강대로 405", "부산광역시 연제구 중앙대로 1001", "대구 중구 공평로 88",
    "인천광역시 남동구 정각로 29", "광주 서구 내방로 111", "대전 서구 둔산로 100", "울산 남구 중앙로 201",
    "세종특별자치시 한누리대로 2130", "경기도 수원시 팔달구 효원로 1", "강원 춘천시 중앙로 1", "충북 청주시 상당구 상당로 82",
    "충남 홍성군 홍북읍 충남대로 21", "전북 전주시 완산구 효자로 225", "전남 무안군 삼향읍 오룡길 1", "경북 안동시 풍천면 도청대로 455",
    "경남 창원시 의창구 중앙대로 300", "제주특별자치도 제주시 문연로 6", "서울 강남구 역삼동 823", "서울 종로구 동숭동 산 2-10",
]
PLACES = [
    "서울역", "강남역", "부산역", "동대구역", "인천국제공항", "김포공항", "광주송정역", "대전역",
    "서울특별시청", "국회의사당", "롯데월드타워", "코엑스", "경복궁", "해운대해수욕장", "성산일출봉", "불국사",
    "전주한옥마을", "춘천역", "서울요금소", "올리브영 광주문흥점",
]


def env_key():
    for line in (ROOT / ".env").read_text(encoding="utf-8").splitlines():
        if line.startswith("VWORLD_API_KEY="):
            return line.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("VWORLD_API_KEY가 .env에 없음")


def get_json(url):
    with urllib.request.urlopen(url, timeout=30) as r:
        return json.load(r)


def local(q):
    t = time.perf_counter()
    body = get_json(f"{LOCAL}/geocode?" + urllib.parse.urlencode({"q": q, "size": 1}))
    ms = (time.perf_counter() - t) * 1000
    items = body.get("data", {}).get("items", [])
    return (items[0] if items else None), ms


def vworld(key, q, kind):
    params = {"service": "search", "request": "search", "version": "2.0", "format": "json", "crs": "EPSG:4326",
              "key": key, "query": q, "size": 1, "type": kind}
    if kind == "ADDRESS":
        params["category"] = "ROAD"
    body = get_json(f"{VWORLD}?" + urllib.parse.urlencode(params))
    items = body.get("response", {}).get("result", {}).get("items", [])
    if not items:
        return None
    it = items[0]
    addr = it.get("address", {})
    return {"title": it.get("title") or addr.get("road") or addr.get("parcel"), "lat": float(it["point"]["y"]), "lon": float(it["point"]["x"])}


def dist(a, b):
    lat = math.radians((a["lat"] + b["lat"]) / 2)
    dy = math.radians(a["lat"] - b["lat"]) * 6371000
    dx = math.radians(a["lon"] - b["lon"]) * 6371000 * math.cos(lat)
    return math.hypot(dx, dy)


def main():
    key = env_key()
    summary = {}
    for kind, queries in [("ADDRESS", ADDRESSES), ("PLACE", PLACES)]:
        print(f"== {kind}")
        rows = []
        for q in queries:
            mine, ms = local(q)
            theirs = vworld(key, q, kind)
            d = dist(mine, theirs) if mine and theirs else None
            rows.append((q, mine, theirs, d, ms))
            print(f"{q} | 로컬 {mine['title'] if mine else '없음'} | 브이월드 {theirs['title'] if theirs else '없음'} | "
                  f"{'-' if d is None else f'{d:.0f}m'} | {ms:.0f}ms")
            time.sleep(0.2)
        both = [r for r in rows if r[3] is not None]
        summary[kind] = {
            "검색어": len(rows), "로컬 결과 있음": sum(1 for r in rows if r[1]), "브이월드 결과 있음": sum(1 for r in rows if r[2]),
            "50m 안": sum(1 for r in both if r[3] <= 50), "200m 안": sum(1 for r in both if r[3] <= 200),
            "1km 넘음": sum(1 for r in both if r[3] > 1000), "로컬 평균 ms": round(sum(r[4] for r in rows) / len(rows), 1),
        }
    print(json.dumps(summary, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()

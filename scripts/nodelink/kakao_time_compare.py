"""로컬 nav-api의 최소시간 경로를 카카오모빌리티 길찾기(최소시간, priority=TIME)와 비교한다.

- 출발·도착은 장소 이름을 로컬 지오코더(/geocode)로 좌표로 바꾼 뒤 사용한다(아래 PAIRS)
- 거리, 소요시간(카카오는 실시간 교통 반영), 통행료, 경로 겹침(서로 30m 안에 드는 길이 비율)을 비교한다
- 겹침이 70% 미만이거나 거리 차이가 10%를 넘는 구간은 로컬 경로 중 카카오 경로 30m 밖에 있는 도로명 구간을 적는다
- 카카오 응답은 저장하지 않고 화면에만 출력한다. 키는 .env의 KAKAO_REST_API_KEY이다

python -X utf8 scripts/nodelink/kakao_time_compare.py [로컬 주소. 기본 http://127.0.0.1:3000]
"""
import json
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

import geopandas as gpd
import numpy as np
from shapely.geometry import LineString

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import env

LOCAL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000"
TM, WGS84 = "EPSG:5186", "EPSG:4326"
BUFFER_M = 30

# (출발, 도착). 시내 구간과 도시 간 구간
PAIRS = [
    ("서울역", "강남역"), ("강남역", "김포공항"), ("잠실역", "서울대입구역"), ("홍대입구역", "수서역"),
    ("부산역", "해운대해수욕장"), ("동대구역", "대구국제공항"), ("인천국제공항", "서울역"), ("수원역", "서울역"),
    ("서울역", "춘천역"), ("서울역", "강릉역"), ("서울역", "대전역"), ("대전역", "전주역"),
    ("서울역", "부산역"), ("서울역", "광주송정역"), ("광주송정역", "목포역"), ("동대구역", "울산역"),
    ("부산역", "창원중앙역"), ("대전역", "청주국제공항"), ("전주역", "여수엑스포역"), ("강릉역", "속초시청"),
    ("천안아산역", "평택역"), ("세종특별자치시청", "대전역"), ("포항역", "경주역"), ("원주역", "충주역"),
]


def get_json(url, headers=None):
    req = urllib.request.Request(url, headers=headers or {})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def geocode(name):
    body = get_json(f"{LOCAL}/geocode?" + urllib.parse.urlencode({"q": name, "size": 1}))
    items = body["data"]["items"]
    if not items:
        print(f"지오코더에 없음: {name}")
        return None
    return items[0]["lat"], items[0]["lon"], items[0]["title"]


def local_route(a, b):
    req = urllib.request.Request(f"{LOCAL}/route", data=json.dumps({"from": {"lat": a[0], "lon": a[1]}, "to": {"lat": b[0], "lon": b[1]}, "profile": "time"}).encode(),
                                 headers={"Content-Type": "application/json"})
    body = json.load(urllib.request.urlopen(req, timeout=60))
    if body["code"] != "OK":
        return None
    return body["data"]


def kakao_route(key, a, b):
    url = (f"https://apis-navi.kakaomobility.com/v1/directions?origin={a[1]},{a[0]}&destination={b[1]},{b[0]}"
           "&priority=TIME&summary=false")
    body = get_json(url, {"Authorization": "KakaoAK " + key})
    route = body["routes"][0]
    if route["result_code"] != 0:
        return None
    roads = [rd for sec in route["sections"] for rd in sec["roads"]]
    coords = [v for rd in roads for v in zip(rd["vertexes"][0::2], rd["vertexes"][1::2])]
    parts = [(rd["name"] or "-", list(zip(rd["vertexes"][0::2], rd["vertexes"][1::2]))) for rd in roads if len(rd["vertexes"]) >= 4]
    s = route["summary"]
    return {"distance": s["distance"], "duration": s["duration"], "toll": s["fare"]["toll"], "coords": coords, "parts": parts}


def to_tm(lonlat):
    return gpd.GeoSeries([LineString(lonlat)], crs=WGS84).to_crs(TM).iloc[0]


def off_segments(route, kline_tm):
    """로컬 경로 중 카카오 경로 30m 밖에 있는 길이를 도로명 구간별로 더한다"""
    pts = [(lon, lat) for lat, lon in route["geometry"]]
    line = to_tm(pts)
    buffer = kline_tm.buffer(BUFFER_M)
    # 좌표 열을 따라 누적 거리를 재고 밖에 있는 꼭짓점 사이 길이를 구간에 배분한다
    coords = list(line.coords)
    cum = np.concatenate([[0.0], np.cumsum([LineString(coords[i:i + 2]).length for i in range(len(coords) - 1)])])
    scale = route["distance_m"] / cum[-1] if cum[-1] else 1.0
    off = {}
    for i in range(len(coords) - 1):
        seg = LineString(coords[i:i + 2])
        if seg.within(buffer):
            continue
        at = cum[i] * scale
        name = next((s["name"] or "(이름 없음)" for s in route["segments"] if s["from_m"] <= at < s["to_m"]), "?")
        off[name] = off.get(name, 0.0) + seg.length * scale
    return sorted(off.items(), key=lambda kv: -kv[1])


def main():
    key = env("KAKAO_REST_API_KEY")
    places = {}
    for a, b in PAIRS:
        for name in (a, b):
            if name not in places:
                places[name] = geocode(name)
    rows = []
    for a, b in PAIRS:
        pa, pb = places[a], places[b]
        if not pa or not pb:
            continue
        mine = local_route(pa[:2], pb[:2])
        theirs = kakao_route(key, pa[:2], pb[:2])
        time.sleep(0.3)
        if not mine or not theirs:
            print(f"{a}→{b}: {'로컬' if not mine else '카카오'} 경로 없음")
            continue
        my_line = to_tm([(lon, lat) for lat, lon in mine["geometry"]])
        k_line = to_tm(theirs["coords"])
        in_k = my_line.intersection(k_line.buffer(BUFFER_M)).length / my_line.length
        in_m = k_line.intersection(my_line.buffer(BUFFER_M)).length / k_line.length
        dist_diff = (mine["distance_m"] - theirs["distance"]) / theirs["distance"]
        dur_diff = (mine["duration_s"] - theirs["duration"]) / theirs["duration"]
        toll_mine = mine["toll"]["won"]
        rows.append((dist_diff, in_k, in_m))
        flag = " <<" if in_k < 0.7 or abs(dist_diff) > 0.1 else ""
        print(f"{a}→{b}: 거리 로컬 {mine['distance_m'] / 1000:.1f}km 카카오 {theirs['distance'] / 1000:.1f}km ({dist_diff:+.0%}) | "
              f"시간 로컬 {mine['duration_s'] / 60:.0f}분 카카오 {theirs['duration'] / 60:.0f}분 ({dur_diff:+.0%}) | "
              f"통행료 로컬 {toll_mine:,} 카카오 {theirs['toll']:,} | 겹침 로컬→카카오 {in_k:.0%} 카카오→로컬 {in_m:.0%}{flag}")
        if flag:
            off = off_segments(mine, k_line)[:6]
            print("    카카오 밖 로컬 구간:", ", ".join(f"{n} {m:,.0f}m" for n, m in off))
            k_off = {}
            my_buffer = my_line.buffer(BUFFER_M)
            for name, pts in theirs["parts"]:
                part = to_tm(pts)
                miss = part.length - part.intersection(my_buffer).length
                if miss > 50:
                    k_off[name] = k_off.get(name, 0.0) + miss
            print("    로컬 밖 카카오 도로:", ", ".join(f"{n} {m:,.0f}m" for n, m in sorted(k_off.items(), key=lambda kv: -kv[1])[:6]))
    r = np.array(rows)
    print(f"\n{len(r)}구간 | 거리 차이 중앙값 {np.median(r[:, 0]):+.1%} | 겹침(로컬→카카오) 중앙값 {np.median(r[:, 1]):.0%} 70% 미만 {(r[:, 1] < 0.7).sum()}구간 | 거리 10% 넘게 다름 {(abs(r[:, 0]) > 0.1).sum()}구간")


if __name__ == "__main__":
    main()

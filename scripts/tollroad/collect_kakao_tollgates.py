"""카카오 로컬 검색으로 전국의 '톨게이트' 분류 장소를 모두 수집한다.

카카오 키워드 검색은 한 번에 15건씩 3페이지, 최대 45건까지만 반환하므로 전국 범위 rect에서 시작하여
결과가 15건을 넘으면 4등분하여 다시 검색한다. 45건 기준으로 나누면 함양TG, 동명동호TG, 군위TG가 누락되는 것을 확인하였다.
검색어는 "톨게이트", "요금소", "TG", "하이패스", "영업소"이며 category_name이 "톨게이트"로 끝나는 장소를 채택한다.
범위 검색만으로는 새만금TG, 동양평TG, 진부TG, 고흥TG, 애전TG, 군위TG 등이 누락되어
ETC_75 영업소 목록에서 가동 Y, 가상 N인 영업소명으로도 "{이름}TG", "{이름}톨게이트", "{이름}요금소"를 검색하여
분류가 톨게이트인 결과를 추가한다. 영업소명 끝의 상/하/동/서/본선/지선/JC/JCT/IC, 괄호 부분, 숫자,
'터널', '대교', '지하도로', '휴게소'를 제거한 이름으로도 검색한다.
출력은 API_수집/카카오_톨게이트_전국.csv이며 카카오 장소 ID로 중복을 제거한다.
"""
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import TOLLROAD_DIR, env, read_csv, write_csv

API_DIR = TOLLROAD_DIR / "API_수집"
KOREA = (33.0, 124.5, 38.7, 131.0)  # 남, 서, 북, 동 순서


def search(key, q, b, page):
    rect = f"{b[1]},{b[0]},{b[3]},{b[2]}"  # 서,남,동,북
    url = ("https://dapi.kakao.com/v2/local/search/keyword.json?size=15&page=" + str(page)
           + "&rect=" + rect + "&query=" + urllib.parse.quote(q))
    req = urllib.request.Request(url, headers={"Authorization": "KakaoAK " + key})
    for attempt in range(4):
        try:
            return json.loads(urllib.request.urlopen(req, timeout=30).read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            if e.code == 429:
                time.sleep(2 + attempt * 2)
                continue
            raise SystemExit(f"카카오 HTTP {e.code}")
        except Exception:
            time.sleep(1 + attempt)
    raise SystemExit("카카오 요청 실패")


def search_any(key, q):
    url = "https://dapi.kakao.com/v2/local/search/keyword.json?size=15&query=" + urllib.parse.quote(q)
    req = urllib.request.Request(url, headers={"Authorization": "KakaoAK " + key})
    for attempt in range(4):
        try:
            return json.loads(urllib.request.urlopen(req, timeout=30).read().decode("utf-8"))["documents"]
        except urllib.error.HTTPError as e:
            if e.code == 429:
                time.sleep(2 + attempt * 2)
                continue
            raise SystemExit(f"카카오 HTTP {e.code}")
        except Exception:
            time.sleep(1 + attempt)
    raise SystemExit("카카오 요청 실패")


def collect(key, q, found):
    stack = [KOREA]
    while stack:
        b = stack.pop()
        d = search(key, q, b, 1)
        total = d["meta"]["total_count"]
        if total > 15 and (b[2] - b[0]) > 0.002:
            s, w, n, e = b
            ms, mw = (s + n) / 2, (w + e) / 2
            stack += [(s, w, ms, mw), (s, mw, ms, e), (ms, w, n, mw), (ms, mw, n, e)]
            continue
        docs = d["documents"]
        page = 1
        while not d["meta"]["is_end"] and page < 3:
            page += 1
            d = search(key, q, b, page)
            docs += d["documents"]
        for x in docs:
            if x["category_name"].endswith("톨게이트"):
                found[x["id"]] = x


def main():
    key = env("KAKAO_REST_API_KEY")
    found = {}
    for q in ("톨게이트", "요금소", "TG", "하이패스", "영업소"):
        collect(key, q, found)
        print(q, "누적", len(found))
    units = read_csv(TOLLROAD_DIR / "ETC_75_04_01_168858.csv")
    live = units[(units.가동영업소여부 == "Y") & (units.가상영업소여부 == "N")]
    full = live.영업소명.str.replace(" ", "")
    short = full.str.replace(r"\(.*?\)", "", regex=True)
    short = short.str.replace(r"(입구JC|출구JC|JCT|JC|IC|본선|지선|상|하|동|서|\d)$", "", regex=True)
    short = short.str.replace(r"(터널|대교|지하도로|휴게소|요금소)$", "", regex=True)
    names = set(full) | set(short[short.str.len() >= 2])
    for n in sorted(names):
        for q in (n + "TG", n + "톨게이트", n + "요금소"):
            d = search_any(key, q)
            for x in d:
                if x["category_name"].endswith("톨게이트"):
                    found[x["id"]] = x
    print("영업소명 검색 후 누적", len(found))
    df = pd.DataFrame(found.values())
    df = df.rename(columns={"id": "카카오ID", "place_name": "장소명", "category_name": "분류", "address_name": "주소",
                            "road_address_name": "도로명주소", "y": "위도", "x": "경도"})
    df = df.sort_values("장소명", kind="stable")
    write_csv(API_DIR / "카카오_톨게이트_전국.csv", df, ["카카오ID", "장소명", "분류", "주소", "도로명주소", "위도", "경도"])
    print("저장", len(df))


if __name__ == "__main__":
    main()

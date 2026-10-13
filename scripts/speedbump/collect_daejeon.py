"""대전광역시 과속방지턱 조회서비스 OpenAPI 전체를 수집한다. 공공데이터포털 15110688이다.

엔드포인트: https://apis.data.go.kr/6300000/GetSdhpListService/getSdhpList
사용 키: .env의 DATA_GO_KR_API_KEY
결과: maps/과속방지턱정보/나중/02_API_수집/대전광역시_과속방지턱.csv 8,608건, 2022-12-10 기준

실행: python scripts/speedbump/collect_daejeon.py
테스트: python scripts/speedbump/collect_daejeon.py --max-pages 1 --out <임시 경로>
"""
import argparse
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import SPEEDBUMP_DIR, env, get_json, write_csv  # noqa: E402

BASE = "https://apis.data.go.kr/6300000/GetSdhpListService/getSdhpList"
COLS = ["CTPRVNNM", "SIGNGU_NM", "ROADNM", "LNMADR", "ITLPC", "OVRSPDPRVNMATRL",
        "OVRSPDPRVNTYPE", "BT", "ET", "ROAD_TP", "LATITUDE", "LONGITUDE", "REFERENCEDATE"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(SPEEDBUMP_DIR / "나중" / "02_API_수집" / "대전광역시_과속방지턱.csv"))
    ap.add_argument("--rows", type=int, default=1000, help="한 페이지 건수")
    ap.add_argument("--max-pages", type=int, default=0, help="테스트용. 0이면 전체")
    a = ap.parse_args()

    key = env("DATA_GO_KR_API_KEY")
    rows, page = [], 1
    while True:
        url = f"{BASE}?serviceKey={key}&numOfRows={a.rows}&pageNo={page}&type=json"
        body = get_json(url, secrets=(key,))["response"]["body"]
        total = body["totalCount"]
        items = body.get("items", {}).get("item", [])
        if isinstance(items, dict):
            items = [items]
        rows += items
        print(f"page {page}: 누적 {len(rows)} / {total}")
        if len(rows) >= total or not items or (a.max_pages and page >= a.max_pages):
            break
        page += 1
    write_csv(a.out, pd.DataFrame(rows, dtype=object), COLS)
    print("저장:", a.out)


if __name__ == "__main__":
    main()

"""전남 곡성군 과속방지턱 정보 데이터 조회 서비스 OpenAPI 전체를 수집한다.

엔드포인트: https://apis.data.go.kr/4860000/speedBumpInfo/getSpeedBumpInfoList
사용 키: .env의 DATA_GO_KR_API_KEY
파라미터는 serviceKey, pageNo, numOfRows, resultType이며 resultType은 xml 또는 json이다.
결과: maps/과속방지턱정보/나중/02_API_수집/전남광주통합특별시_곡성군_과속방지턱.csv 407건, 2023-12-31 기준

실행: python scripts/speedbump/collect_gokseong.py
테스트: python scripts/speedbump/collect_gokseong.py --max-pages 1 --rows 20 --out <임시 경로>
"""
import argparse
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import SPEEDBUMP_DIR, env, get_json, write_csv  # noqa: E402

BASE = "https://apis.data.go.kr/4860000/speedBumpInfo/getSpeedBumpInfoList"
COLS = ["NODEID", "CTPRVNNM", "SIGNGUNM", "ROADNM", "RDNMADR", "LNMADR", "INSTLPLACE", "OVRSPDPRVNMATRLSE",
        "OVRSPDPRVNSTLESE", "OVRSPDPRVNHG", "OVRSPDPRVNBT", "OVRSPDPRVNEXTN", "ROADTYSE", "STNDRDYN",
        "LATITUDE", "LONGITUDE", "SEPRATYN", "CTNUSTLEYN", "INSTITUTIONNM", "PHONENUMBER", "REFERENCEDATE",
        "IMGNM"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(SPEEDBUMP_DIR / "나중" / "02_API_수집" / "전남광주통합특별시_곡성군_과속방지턱.csv"))
    ap.add_argument("--rows", type=int, default=100, help="한 페이지 건수")
    ap.add_argument("--max-pages", type=int, default=0, help="테스트용. 0이면 전체")
    a = ap.parse_args()

    key = env("DATA_GO_KR_API_KEY")
    rows, page = [], 1
    while True:
        url = f"{BASE}?serviceKey={key}&pageNo={page}&numOfRows={a.rows}&resultType=json"
        d = get_json(url, secrets=(key,))
        if "response" not in d:
            raise SystemExit(f"오류 응답: {str(d)[:300]}")
        body = d["response"]["body"]
        total = body["totalCount"]
        items = body.get("items", {}).get("item", []) if body.get("items") else []
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

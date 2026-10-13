"""세종특별자치시 과속방지턱 조회 서비스 OpenAPI 전체를 수집한다.

엔드포인트: https://apis.data.go.kr/5690000/sjSpeedBump/sj_00000170
가이드 문서의 Call Back URL에는 /list.do가 붙어 있으나 붙이면 SERVICE_KEY_IS_NOT_REGISTERED_ERROR가 반환되어 붙이지 않는다.
파라미터 이름은 다른 API와 달리 pageIndex, pageUnit, dataTy이다.
사용 키: .env의 DATA_GO_KR_API_KEY
결과: maps/과속방지턱정보/나중/02_API_수집/세종특별자치시_과속방지턱.csv 1,216건, 2022-12-31 기준

실행: python scripts/speedbump/collect_sejong.py
테스트: python scripts/speedbump/collect_sejong.py --max-pages 1 --out <임시 경로>
"""
import argparse
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import SPEEDBUMP_DIR, env, get_json, write_csv  # noqa: E402

BASE = "https://apis.data.go.kr/5690000/sjSpeedBump/sj_00000170"
COLS = ["addr", "instlPlace", "ovrspdPrvnMatrlSe", "ovrspdPrvnStlese", "ovrspdPrvnHg", "ovrspdPrvnBt",
        "ovrspdPrvnExtn", "roadTySe", "stndrdYn", "la", "lo", "sepratYn", "ctnuStleYn",
        "instlYear", "mngInstNm", "telno", "crtrYmd"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(SPEEDBUMP_DIR / "나중" / "02_API_수집" / "세종특별자치시_과속방지턱.csv"))
    ap.add_argument("--rows", type=int, default=500, help="한 페이지 건수")
    ap.add_argument("--max-pages", type=int, default=0, help="테스트용. 0이면 전체")
    a = ap.parse_args()

    key = env("DATA_GO_KR_API_KEY")
    rows, page = [], 1
    while True:
        url = f"{BASE}?serviceKey={key}&pageIndex={page}&pageUnit={a.rows}&dataTy=json"
        d = get_json(url, secrets=(key,))
        if "header" not in d:
            raise SystemExit(f"오류 응답: {str(d)[:300]}")
        total = d["header"]["totalCount"]
        items = d["body"]["items"]
        rows += items
        print(f"page {page}: 누적 {len(rows)} / {total}")
        if len(rows) >= total or not items or (a.max_pages and page >= a.max_pages):
            break
        page += 1
    write_csv(a.out, pd.DataFrame(rows, dtype=object), COLS)
    print("저장:", a.out)


if __name__ == "__main__":
    main()

"""행정안전부 과속방지턱정보 조회서비스 OpenAPI를 수집한다.

엔드포인트: https://apis.data.go.kr/1741000/speed_bump_info/info
사용 키: .env의 DATA_GO_KR_API_KEY
한 번에 최대 100건을 조회하므로 전체 141,140건은 1,412번 이상 호출해야 하며 일일 한도는 확인하지 못하였다.
이 API의 전체 건수는 maps/과속방지턱정보/행안부/과속방지턱정보.csv와 같은 141,140건이며
서울 종로·마포·성동·동작·도봉, 대구 수성 등 누락 지역은 이 API에도 없다.

--count : 조건에 맞는 건수만 1회 호출로 조회한다. 지금까지 사용한 모드이다.
--since : 데이터갱신시점이 YYYYMMDDHHMMSS 이후인 변경분만 수집한다. 전체 수집은 이 스크립트로 실행한 적이 없다.

예)
  python scripts/speedbump/collect_mois.py --count
  python scripts/speedbump/collect_mois.py --count --sgg 종로 --sgg 마포
  python scripts/speedbump/collect_mois.py --since 20260101000000 --out <경로>
"""
import argparse
import sys
import urllib.parse
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import env, get_json, write_csv  # noqa: E402

BASE = "https://apis.data.go.kr/1741000/speed_bump_info/info"
COLS = ["OPN_ATMY_GRP_CD", "MNG_NO", "SPDMDB_MNG_NO", "ROAD_NM", "CTPV_NM", "SGG_NM", "LCTN_ROAD_NM_ADDR",
        "LCTN_LOTNO_ADDR", "INSTL_PLC", "SPDMDB_MATRL", "SPDMDB_SHP_SE", "SPDMDB_HGT", "SPDMDB_WDT",
        "SPDMDB_PRLG", "ROAD_TYPE_SE", "SPCFCT_YN", "WGS84_LAT", "WGS84_LOT", "MTRFC_SEPR_YN", "CNTNS_YN",
        "SPDMDB_INSTL_YR", "MNG_INST_NM", "MNG_INST_TELNO", "DAT_CRTR_YMD", "LAST_MDFCN_PNT",
        "DAT_UPDT_PNT", "DAT_UPDT_SE"]


def call(key, page, rows, sgg=None, since=None):
    q = f"serviceKey={key}&pageNo={page}&numOfRows={rows}&returnType=json"
    if sgg:
        q += "&cond[SGG_NM::LIKE]=" + urllib.parse.quote(sgg)
    if since:
        q += "&cond[DAT_UPDT_PNT::GTE]=" + since
    return get_json(f"{BASE}?{q}", secrets=(key,))["response"]["body"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--count", action="store_true", help="건수만 조회")
    ap.add_argument("--sgg", action="append", help="시군구명 포함 검색(여러 번 지정 가능)")
    ap.add_argument("--since", help="데이터갱신시점 이상, YYYYMMDDHHMMSS")
    ap.add_argument("--out", help="CSV 저장 경로(전체/변경분 수집 시 필요)")
    ap.add_argument("--max-pages", type=int, default=0, help="테스트용. 0이면 전체")
    a = ap.parse_args()
    key = env("DATA_GO_KR_API_KEY")

    if a.count:
        for sgg in (a.sgg or [None]):
            print(sgg or "(전체)", call(key, 1, 1, sgg, a.since)["totalCount"])
        return
    if not a.out:
        raise SystemExit("--out 이 필요합니다")
    rows, page = [], 1
    while True:
        body = call(key, page, 100, (a.sgg or [None])[0], a.since)
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

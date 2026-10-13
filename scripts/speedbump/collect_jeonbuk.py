"""전북특별자치도 과속방지턱 오픈API를 수집한다. 전북 서버 오류로 수집하지 못하였다.

엔드포인트: https://apis.data.go.kr/6450000/speedbumps/getspeedbumps
가이드 문서 '전라북도 공공데이터 API - 전라북도_과속방지턱 현황.docx'의 호출 예시는 http://이며
요청 파라미터는 serviceKey, numOfRows, 0부터 시작하는 pageNo, sortKey, filterKey, filterValues, type이다.
2026-10-13에 https와 http, type=json과 xml, pageNo=0과 1, 필터와 정렬 지정을 모두 시도하였으나 전북 서버가
'RFC 3.0 오류 메세지' HTML을 반환하였다. 공공데이터포털 게이트웨이는 키를 통과시키므로 키가 아니라 전북 서버의 오류이다.
대신 maps/과속방지턱정보/나중/01_원본_파일/전북/전라북도_과속방지턱 현황(보완).csv 20,462건, 2021-12-21 기준을 사용한다.

API가 복구되면 이 스크립트로 전체를 수집한다. 복구 전이라 수집 성공은 확인하지 못하였다.
  python scripts/speedbump/collect_jeonbuk.py --out <경로>
"""
import argparse
import re
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import env, http_get, write_csv  # noqa: E402

BASE = "https://apis.data.go.kr/6450000/speedbumps/getspeedbumps"
COLS = ["ovrspdPrvnNo", "roadRouteNo", "roadRouteNoSn", "ctprvnNm", "signguNm", "roadNm", "rdnmadr", "lnmadr",
        "itlpc", "ovrspdPrvnMatrl", "ovrspdPrvnType", "hg", "bt", "et", "roadType", "stndrdYn", "latitude",
        "longitude", "mdstrpYn", "ctnuYn", "sigunCode", "institutionNm", "phoneNumber", "referenceDate"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--rows", type=int, default=1000)
    ap.add_argument("--max-pages", type=int, default=0, help="테스트용. 0이면 전체")
    a = ap.parse_args()
    key = env("DATA_GO_KR_API_KEY")

    rows, page = [], 0  # 가이드: pageNo 는 0부터
    while True:
        url = f"{BASE}?serviceKey={key}&type=xml&numOfRows={a.rows}&pageNo={page}"
        text = http_get(url, secrets=(key,)).decode("utf-8", "replace")
        if "<html" in text[:500].lower():
            raise SystemExit("전북 서버 오류 페이지(RFC 3.0 오류 메세지)가 반환됨. API 복구 후 다시 시도")
        total = int(re.search(r"<totalCount>(\d+)</totalCount>", text).group(1))
        for blk in re.findall(r"<item>(.*?)</item>", text, flags=re.S):
            rows.append({c: (re.search(rf"<{c}>(.*?)</{c}>", blk, flags=re.S) or [None, ""])[1] for c in COLS})
        print(f"page {page}: 누적 {len(rows)} / {total}")
        if len(rows) >= total or (a.max_pages and page + 1 >= a.max_pages):
            break
        page += 1
    write_csv(a.out, pd.DataFrame(rows, dtype=object), COLS)
    print("저장:", a.out)


if __name__ == "__main__":
    main()

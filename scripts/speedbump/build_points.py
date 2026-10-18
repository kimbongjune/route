"""행안부 원본과 지오코딩 결과를 합쳐 내비에서 사용할 과속방지턱 파일 두 개를 만든다.

입력
  행안부/과속방지턱정보.csv                       원본 141,140행
  행안부/지오코딩_행안부_좌표오류_대표좌표.csv     geocode_mois_rows.py로 만든 좌표 오류, 대표 좌표 행의 지오코딩 결과
규칙
  - 지오코딩 대상이 아닌 행은 원본 좌표를 그대로 사용한다.
  - 지오코딩 대상 행은 변환 좌표를 사용하고 찾지 못한 행은 제외한다.
  - 변환 좌표 하나에 REP_LIMIT행 이상 모이면 군청이나 구 이름만 있는 대표 주소이므로 그 행들도 제외한다.
  - 노면에 그린 가상형은 안내제외 Y로 두고 데이터에는 유지한다.
  - 지점 파일은 좌표 문자열이 완전히 같은 행을 한 지점으로 병합한다. 연속 방지턱은 몇 m 간격으로 따로 있으므로 거리로는 병합하지 않는다.
출력
  행안부/과속방지턱_최종.csv   행 단위
  행안부/과속방지턱_지점.csv   지점 단위

python -X utf8 scripts/speedbump/build_points.py [--out-dir 다른폴더]
"""
import argparse
import sys
from pathlib import Path

import numpy as np
import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import SPEEDBUMP_DIR, read_csv, write_csv

REP_LIMIT = 20
REAL = ("원호형", "사다리꼴", "기타")
ROW_COLS = ["관리번호", "시도명", "시군구명", "도로명", "설치장소", "형태", "안내제외", "위도", "경도", "좌표출처"]
POINT_COLS = ["위도", "경도", "시도명", "시군구명", "도로명", "설치장소", "안내제외", "연속형", "행수", "실제턱행수", "가상형행수", "좌표출처", "관리번호목록"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out-dir", default=str(SPEEDBUMP_DIR / "행안부"))
    a = ap.parse_args()
    out_dir = Path(a.out_dir)

    src = read_csv(SPEEDBUMP_DIR / "행안부" / "과속방지턱정보.csv")
    geo = read_csv(SPEEDBUMP_DIR / "행안부" / "지오코딩_행안부_좌표오류_대표좌표.csv").drop_duplicates("관리번호", keep="last")
    got = geo[geo.변환위도 != ""].groupby(["변환위도", "변환경도"]).size()
    rep = got[got >= REP_LIMIT].index

    df = src.merge(geo[["관리번호", "변환위도", "변환경도"]], on="관리번호", how="left", indicator=True)
    target = df._merge == "both"
    missing = target & (df.변환위도 == "")
    grouped = target & ~missing & pd.MultiIndex.from_frame(df[["변환위도", "변환경도"]]).isin(rep)
    dropped = {"주소를 못 찾음": int(missing.sum()), "대표 주소로 모임": int(grouped.sum())}
    df = df[~missing & ~grouped].copy()
    target = df._merge == "both"
    df["좌표출처"] = np.where(target, "지오코딩", "원본")
    # 원본은 소수 자릿수가 제각각이라 7자리로 통일한다. 지점을 병합할 때 이 문자열을 그대로 비교한다
    df["위도"] = df.변환위도.where(target, df.WGS84위도).astype(float).map("{:.7f}".format)
    df["경도"] = df.변환경도.where(target, df.WGS84경도).astype(float).map("{:.7f}".format)
    df["형태"] = df.과속방지턱형태구분
    df["안내제외"] = np.where(df.형태 == "가상형", "Y", "N")
    print("원본", f"{len(src):,}", "→ 최종", f"{len(df):,}", "| 버림", dropped, "| 대표 주소 좌표", len(rep), "곳")
    print("좌표출처", df.좌표출처.value_counts(sort=False).to_dict(), "| 형태", df.형태.value_counts(sort=False).to_dict())
    write_csv(out_dir / "과속방지턱_최종.csv", df, ROW_COLS)

    # 이름, 장소, 연속형은 실제 턱 행에서 가져온다. 실제 턱 행이 없는 지점은 가상형 행에서 가져온다
    df["실제"] = df.형태.isin(REAL)
    df["연속"] = df.연속형여부 == "Y"
    g = df.groupby(["위도", "경도"], sort=False)
    df["지점에실제턱"] = g.실제.transform("any")
    use = df[df.실제 | ~df.지점에실제턱]
    pts = g.agg(행수=("관리번호", "size"), 실제턱행수=("실제", "sum"),
                좌표출처=("좌표출처", lambda s: ";".join(sorted(set(s)))), 관리번호목록=("관리번호", ";".join))
    pts = pts.join(use.groupby(["위도", "경도"]).agg(
        시도명=("시도명", "first"), 시군구명=("시군구명", "first"), 도로명=("도로명", "first"), 설치장소=("설치장소", "first"),
        연속=("연속", "any")))
    pts["가상형행수"] = pts.행수 - pts.실제턱행수
    pts["안내제외"] = np.where(pts.실제턱행수 > 0, "N", "Y")
    pts["연속형"] = np.where(pts.연속, "Y", "N")
    pts = pts.reset_index().sort_values(["시도명", "시군구명"], kind="stable")
    write_csv(out_dir / "과속방지턱_지점.csv", pts, POINT_COLS)
    on = pts[pts.안내제외 == "N"]
    print("지점", f"{len(pts):,}", "| 안내 지점", f"{len(on):,}", "(연속형", f"{(on.연속형 == 'Y').sum():,}",
          "2행 이상 합침", f"{(on.행수 >= 2).sum():,})", "| 가상형만 있는 지점", f"{len(pts) - len(on):,}")
    print("저장:", out_dir)


if __name__ == "__main__":
    main()

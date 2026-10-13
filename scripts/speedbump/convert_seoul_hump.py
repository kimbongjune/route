"""서울시 험프 셰이프파일 A067_A를 위경도 중심점 CSV로 바꾼다.

원본: maps/과속방지턱정보/나중/05_서울_험프/원본/A067_A(험프20250417)/A067_A.shp, .dbf
  서울시 교통안전시설물 T-GIS 자료이며 서울 열린데이터광장 OA-15541에서 받았다.
  도형은 중앙값 약 6.3m x 6.3m 폴리곤이고 좌표계는 EPSG:5186과 파라미터가 같은 KGD2002 중부원점이다.
  주소, 도로명, 높이, 폭 필드는 없고 구코드 GU_CDE, 종류 A067_KND_C(001 요철, 002 이미지), 상태 등만 있다.
처리: 폴리곤 외곽 사각형의 중심을 WGS84로 변환한다.
결과: maps/과속방지턱정보/나중/05_서울_험프/서울시_험프_A067_20250417_중심점.csv (18,384건)

실행: python scripts/speedbump/convert_seoul_hump.py
테스트: python scripts/speedbump/convert_seoul_hump.py --out <임시 파일>
"""
import argparse
import sys
from pathlib import Path

import geopandas as gpd
import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import SPEEDBUMP_DIR, write_csv  # noqa: E402

GU = {"110": "종로구", "140": "중구", "170": "용산구", "200": "성동구", "210": "광진구", "230": "동대문구",
      "260": "중랑구", "290": "성북구", "300": "강북구", "320": "도봉구", "350": "노원구", "380": "은평구",
      "410": "서대문구", "440": "마포구", "470": "양천구", "500": "강서구", "530": "구로구", "540": "금천구",
      "560": "영등포구", "590": "동작구", "620": "관악구", "650": "서초구", "680": "강남구", "710": "송파구",
      "740": "강동구"}  # 공통코드테이블정의서(교통안전시설물관리시스템) GU_CDE
COLS = {"MGRNU": "관리번호", "GU_CDE": "구코드", "A067_KND_C": "종류코드(001요철/002이미지)", "STAT_CDE": "상태코드",
        "EVE_CDE": "위치코드(001지상)", "ROD_GBN_CD": "도로구분(001시도/002구도)", "ESB_YMD": "설치일", "CAE_YMD": "교체일",
        "FRM_CDE": "공사형태"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default=str(SPEEDBUMP_DIR / "나중" / "05_서울_험프" / "원본" / "A067_A(험프20250417)" / "A067_A"))
    ap.add_argument("--out", default=str(SPEEDBUMP_DIR / "나중" / "05_서울_험프" / "서울시_험프_A067_20250417_중심점.csv"))
    a = ap.parse_args()

    # 고리가 닫히지 않은 폴리곤이 있어 on_invalid="fix"로 읽는다. 외곽 사각형은 바뀌지 않는다
    g = gpd.read_file(a.src + ".shp", encoding="cp949", engine="pyogrio", on_invalid="fix").set_crs("EPSG:5186", allow_override=True)
    g = g[g.geometry.notna()]
    b = g.bounds
    center = gpd.GeoSeries(gpd.points_from_xy((b.minx + b.maxx) / 2, (b.miny + b.maxy) / 2), crs="EPSG:5186").to_crs("EPSG:4326")
    df = g[list(COLS)].fillna("").rename(columns=COLS)
    df.insert(1, "구", df.구코드.map(GU).fillna(df.구코드))
    df["위도"] = [f"{y:.7f}" for y in center.y]
    df["경도"] = [f"{x:.7f}" for x in center.x]
    write_csv(a.out, df)
    print(f"저장: {a.out} ({len(df)}건)")


if __name__ == "__main__":
    main()

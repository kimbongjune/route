"""어린이보호구역, 노인장애인보호구역 표준데이터 분포를 집계한다.

python -X utf8 scripts/zone/stats.py
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, read_csv

CHILD = ROOT / "maps" / "전국어린이보호구역표준데이터" / "전국어린이보호구역표준데이터.csv"
ELDER = ROOT / "maps" / "전국노인장애인보호구역표준데이터" / "전국노인장애인보호구역표준데이터.csv"


def empty(df, col):
    return (df[col].str.strip() == "").sum()


def main():
    df = read_csv(CHILD)
    print("어린이보호구역", f"{len(df):,}건 컬럼", list(df.columns))
    print("  시설종류", df.시설종류.value_counts().to_dict())
    print("  빈값: 도로명주소", empty(df, "소재지도로명주소"), "지번주소", f"{empty(df, '소재지지번주소'):,}",
          "위도", empty(df, "위도"), "CCTV설치대수", f"{empty(df, 'CCTV설치대수'):,}", "보호구역도로폭", f"{empty(df, '보호구역도로폭'):,}")
    print("  CCTV설치여부", df.CCTV설치여부.value_counts(sort=False).to_dict())
    col = next(c for c in df.columns if c.startswith("어린이보호구역지정여부"))
    print(f"  {col}", df[col].map(repr).value_counts().to_dict())
    print("  도로폭 범위 표기 예", list(df.보호구역도로폭[df.보호구역도로폭.str.contains("~")][:3]))
    print("  데이터기준일자 종류", df.데이터기준일자.nunique(), "대상시설명 고유", f"{df.대상시설명.nunique():,}")
    print("  시간제·제한속도 컬럼:", [c for c in df.columns if "시간" in c or "속도" in c])

    df = read_csv(ELDER)
    print("노인·장애인보호구역", f"{len(df):,}건 컬럼", list(df.columns))
    print("  장소유형코드", df.장소유형코드.value_counts().to_dict())
    print("  제한속도", df.제한속도.value_counts().to_dict())
    print("  빈값: 도로명주소", empty(df, "소재지도로명주소"), "지번주소", empty(df, "소재지지번주소"),
          "CCTV설치대수", f"{empty(df, 'CCTV설치대수'):,}", "보호구역도로폭", f"{empty(df, '보호구역도로폭'):,}")
    print("  시간제 컬럼:", [c for c in df.columns if "시간" in c])


if __name__ == "__main__":
    main()

"""전국무인교통단속카메라표준데이터 분포를 집계하고 도로공사 구간단속현황과 이정을 대조한다.

python -X utf8 scripts/camera/stats.py
"""
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, read_csv

SRC = ROOT / "maps" / "전국무인교통단속카메라표준데이터" / "전국무인교통단속카메라표준데이터.csv"
EX = ROOT / "maps" / "구간단속현황" / "한국도로공사_구간단속현황_20260801.csv"
TOL = 0.2


def norm(code):
    # '01+02', '1+2', '02'가 섞여 있어 앞의 0을 제거하고 '+'로 연결한 형식으로 통일한다
    return "+".join(str(int(x)) for x in code.split("+") if x.strip().isdigit())


def main():
    df = read_csv(SRC)
    df["단속"] = df.단속구분.map(norm)
    print("건수", f"{len(df):,}", "컬럼", len(df.columns) - 1)
    print("단속구분 원래 값", df.단속구분.value_counts().head(12).to_dict())
    print("단속구분 정규화", df.단속.value_counts().to_dict())
    zero = df[df.제한속도.str.strip().isin(["0", ""])]
    print("제한속도 0 또는 빈값", f"{len(zero):,}", "그중 단속구분", zero.단속.value_counts().head(4).to_dict())
    sig = df[df.단속 == "2"]
    print("신호위반(2) 카메라 중 제한속도가 0이 아닌 것", f"{(~sig.제한속도.str.strip().isin(['0', ''])).sum():,} / {len(sig):,}")

    sec = df[df.단속구간위치구분.str.strip() != ""]
    print("구간단속 행", f"{len(sec):,}", sec.단속구간위치구분.map(norm).value_counts(sort=False).to_dict())
    print("  도로종류", sec.도로종류.value_counts().to_dict())
    hw = sec[sec.도로종류 == "고속국도"]
    print("  고속국도 구간단속의 단속구분", hw.단속.value_counts(sort=False).to_dict())
    print("  같은 좌표 2행 이상", (sec.groupby(["위도", "경도"]).size() >= 2).sum(), "좌표")

    # 표준 CSV 설치장소에 '273.5km', '256.5k', '36.8K' 형식으로 이정이 기재된 행만 사용한다.
    # 노선명 표기가 '서해안선', '서해안고속도로'처럼 서로 달라 노선은 대조하지 않고 이정 숫자만 비교하며 차이가 TOL km 이내면 같은 지점으로 본다.
    ex = read_csv(EX)
    print("도로공사 구간단속현황", len(ex), "필드", list(ex.columns))
    km = hw.설치장소.str.extract(r"(\d+(?:\.\d+)?)\s*[kK]")[0].dropna().astype(float).to_numpy()
    print("  표준 CSV 고속국도 구간단속 중 설치장소에 이정이 있는 행", len(km), "/", len(hw))
    near = lambda s: (np.abs(s.astype(float).to_numpy()[:, None] - km[None, :]) <= TOL).any(axis=1)
    hit = near(ex.시점).astype(int) + near(ex.종점).astype(int)
    print(f"  도공 {len(ex)}건 중 시점·종점 하나라도 표준 CSV 이정과 {TOL}km 안", (hit >= 1).sum(), "둘 다", (hit == 2).sum(), "불일치", (hit == 0).sum())
    print("  도공 노선명", sorted(ex.노선.unique()))
    print("  표준 CSV 고속국도 노선명", sorted(hw.도로노선명.unique()))


if __name__ == "__main__":
    main()

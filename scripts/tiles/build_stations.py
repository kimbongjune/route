"""국가철도공단 도시광역철도 역사정보(전체_도시철도역사정보_20260630.xlsx)를 지도 라벨용 역 CSV로 만든다.

- 입력: maps/배경지도/전체_도시철도역사정보_20260630.xlsx 시트 "표준데이터 역사"(역사명, 노선명, 역위도, 역경도)
- 같은 역사명이고 서로 300m 안인 행은 한 역으로 묶고 좌표는 평균, 노선명은 "·"로 잇는다
- 출력: maps/배경지도/역사.csv  컬럼 name, lines, lat, lon
- 행 수, 묶은 역 수를 출력한다

python -X utf8 scripts/tiles/build_stations.py
"""
import math
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, write_csv

SRC = ROOT / "maps" / "배경지도" / "전체_도시철도역사정보_20260630.xlsx"
OUT = ROOT / "maps" / "배경지도" / "역사.csv"
NEAR_M = 300


def main():
    df = pd.read_excel(SRC, sheet_name="표준데이터 역사")
    df = df.dropna(subset=["역위도", "역경도"])
    groups = []  # [이름, 노선 목록, 위도 목록, 경도 목록]
    for r in df.itertuples():
        for g in groups:
            if g[0] == r.역사명 and math.hypot((g[3][0] - r.역경도) * 88_000, (g[2][0] - r.역위도) * 110_950) <= NEAR_M:
                if r.노선명 not in g[1]:
                    g[1].append(r.노선명)
                g[2].append(r.역위도)
                g[3].append(r.역경도)
                break
        else:
            groups.append([r.역사명, [r.노선명], [r.역위도], [r.역경도]])
    out = pd.DataFrame({"name": [g[0] for g in groups], "lines": ["·".join(g[1]) for g in groups],
                        "lat": [sum(g[2]) / len(g[2]) for g in groups], "lon": [sum(g[3]) / len(g[3]) for g in groups]})
    write_csv(OUT, out)
    print(f"행 {len(df)}개, 역 {len(out)}곳")


if __name__ == "__main__":
    main()

"""상습 결빙구간 CSV(행정안전부, CP949)를 nav-api가 읽는 UTF-8 CSV로 만든다.

- 입력: maps/주의운전/행정안전부_상습 결빙구간.csv. 파일 이름이 자모가 분리된 형태(NFD)여서 이름을 정규화하여 찾는다
- 구간 번호, 도로 분류, 대표지역, 도로(노선)명, 총길이(km), 기점·종점 위경도만 남긴다. 좌표나 길이가 없는 행은 뺀다
- 출력: maps/주의운전/결빙구간.csv  컬럼 id, kind, region, road, length_m, start_lat, start_lon, end_lat, end_lon
- 행 수, 도로 분류별 수, 기점·종점 직선거리와 총길이의 비율 분위수를 출력한다

python -X utf8 scripts/caution/build_icing.py
"""
import sys
import unicodedata
from pathlib import Path

import numpy as np
import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, write_csv

FOLDER = ROOT / "maps" / "주의운전"


def find(name):
    """정규화한 이름이 같은 파일"""
    for p in FOLDER.iterdir():
        if unicodedata.normalize("NFC", p.name) == name:
            return p
    raise SystemExit(f"{name} 이 없습니다")


def main():
    df = pd.read_csv(find("행정안전부_상습 결빙구간.csv"), encoding="cp949")
    df.columns = ["id", "agency", "kind", "region", "road", "km", "start_lat", "start_lon", "end_lat", "end_lon", "x1", "y1", "azimuth"]
    total = len(df)
    df = df.dropna(subset=["km", "start_lat", "start_lon", "end_lat", "end_lon"])
    df = df[df.km > 0]
    df["length_m"] = (df.km * 1000).round(1)
    out = df[["id", "kind", "region", "road", "length_m", "start_lat", "start_lon", "end_lat", "end_lon"]]
    write_csv(FOLDER / "결빙구간.csv", out)
    straight = np.hypot((df.start_lat - df.end_lat) * 111_320, (df.start_lon - df.end_lon) * 111_320 * np.cos(np.radians(df.start_lat)))
    ratio = (straight / df.length_m).quantile([0.05, 0.25, 0.5, 0.75, 0.95]).round(2).to_dict()
    print(f"원본 {total}행, 저장 {len(out)}행(좌표·길이 없는 행 {total - len(out)}개 제외)")
    print("도로 분류:", out.kind.value_counts().to_dict())
    print("총길이(m) 분위수:", out.length_m.quantile([0, 0.25, 0.5, 0.75, 1]).to_dict())
    print("기점·종점 직선거리 / 총길이 분위수:", ratio)


if __name__ == "__main__":
    main()

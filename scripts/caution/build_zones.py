"""어린이보호구역과 노인·장애인보호구역 표준데이터를 nav-api가 읽는 보호구역 CSV로 만든다.

- 입력: 전국어린이보호구역표준데이터.csv(14,654건), 전국노인장애인보호구역표준데이터.csv(4,184건). 둘 다 CP949이고 위경도는 보호구역 중심이다
- 종류: 어린이보호구역은 child, 노인·장애인은 `장소유형코드` 1·01 elderly, 2·02 disabled
- 제한속도: 노인·장애인 CSV의 `제한속도`만 있다. 0과 두 값이 이어진 `3050`은 빈 값으로 한다. 어린이보호구역은 제한속도 컬럼이 없어 빈 값이다
- 좌표가 없거나 위도 33~39, 경도 124~132 밖인 행은 뺀다
- 출력: maps/주의운전/보호구역.csv  컬럼 kind, name, lat, lon, limit
- 종류별 행 수와 제한속도 분포를 출력한다

python -X utf8 scripts/caution/build_zones.py
"""
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, read_csv, write_csv

CHILD = ROOT / "maps" / "전국어린이보호구역표준데이터" / "전국어린이보호구역표준데이터.csv"
ELDER = ROOT / "maps" / "전국노인장애인보호구역표준데이터" / "전국노인장애인보호구역표준데이터.csv"
OUT = ROOT / "maps" / "주의운전" / "보호구역.csv"


def main():
    child = read_csv(CHILD)
    elder = read_csv(ELDER)
    a = pd.DataFrame({"kind": "child", "name": child.대상시설명, "lat": child.위도, "lon": child.경도, "limit": ""})
    code = elder.장소유형코드.str.strip().str.lstrip("0")
    limit = elder.제한속도.str.strip()
    limit = limit.where(limit.isin(["20", "30", "40", "50", "60", "70"]), "")
    b = pd.DataFrame({"kind": code.map({"1": "elderly", "2": "disabled"}), "name": elder.대상시설명,
                      "lat": elder.위도, "lon": elder.경도, "limit": limit})
    df = pd.concat([a, b], ignore_index=True)
    total = len(df)
    df["lat"] = pd.to_numeric(df.lat, errors="coerce")
    df["lon"] = pd.to_numeric(df.lon, errors="coerce")
    df = df[df.kind.notna() & df.lat.between(33, 39) & df.lon.between(124, 132)]
    write_csv(OUT, df)
    print(f"입력 {total}행, 저장 {len(df)}행(좌표·종류가 맞지 않는 {total - len(df)}행 제외)")
    print("종류:", df.kind.value_counts().to_dict())
    print("제한속도(노인·장애인):", df[df.kind != "child"].limit.replace("", "없음").value_counts().to_dict())


if __name__ == "__main__":
    main()

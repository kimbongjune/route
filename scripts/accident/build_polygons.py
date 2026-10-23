"""교통사고다발지역 유형별 CSV 12개의 최신 배치와 Riskarea.csv의 최신 연도를 내비 API가 읽는 폴리곤 파일 하나로 만든다.

- 유형별 파일은 `사고다발지id` 앞 4자리가 가장 큰 배치만 사용한다
- Riskarea.csv는 `연도코드`가 가장 큰 연도만 사용하며 폴리곤과 중심점을 EPSG:5179에서 WGS84로 변환한다
- 폴리곤은 바깥 고리만 "경도 위도,경도 위도,..." 형식으로 기록한다. 사고위험지역 폴리곤은 고리가 둘 이상인 행이 있어 첫 고리만 사용한다
- 출력: maps/전국교통사고다발지역표준데이터/사고다발_폴리곤.csv (UTF-8)
  컬럼: 유형, id, 이름, 사고건수, 사상자수, 사망자수, 경도, 위도, 폴리곤

python -X utf8 scripts/accident/build_polygons.py
"""
import json
import re
import sys
from pathlib import Path

import pandas as pd
from pyproj import Transformer

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, read_csv, write_csv

DIR = ROOT / "maps" / "전국교통사고다발지역표준데이터"
OUT = DIR / "사고다발_폴리곤.csv"
KINDS = {"12_25_bicycle": "자전거", "12_25_oldman": "보행노인", "12_25_child": "보행어린이", "12_25_schoolzone": "스쿨존어린이",
         "13_25_freezing": "결빙", "15_25_tmzon": "연휴기간", "17_25_lg": "지자체별", "17_25_motorcycle": "이륜차",
         "17_25_violt": "법규위반", "19_25_drunk": "음주운전", "19_25_pedstrians": "보행자", "19_25_truck": "화물차"}
UTMK_TO_WGS84 = Transformer.from_crs("EPSG:5179", "EPSG:4326", always_xy=True)


def ring_text(points):
    return ",".join(f"{lon:.7f} {lat:.7f}" for lon, lat in points)


def main():
    rows = []
    for name, kind in KINDS.items():
        df = read_csv(DIR / f"{name}.csv")
        batch = df.사고다발지id.str[:4]
        latest = df[batch == batch.max()]
        for r in latest.itertuples(index=False):
            ring = json.loads(r.다발지역폴리곤)["coordinates"][0]
            rows.append((kind, r.사고다발지id, r.지점명, int(r.사고건수), int(r.사상자수), int(r.사망자수),
                         float(r.경도), float(r.위도), ring_text(ring)))
        print(f"{kind}: 배치 {batch.max()} {len(latest):,}건")

    risk = read_csv(DIR / "Riskarea.csv")
    latest = risk[risk.연도코드 == risk.연도코드.max()]
    skipped = 0
    multi = 0
    for r in latest.itertuples(index=False):
        m = re.fullmatch(r"POLYGON\s*\(\((.*?)\)\)", r.사고위험지역폴리곤.strip())
        if not m:
            skipped += 1
            continue
        # 고리가 둘 이상이면 "),(" 로 이어지므로 바깥 고리만 사용한다
        rings = m.group(1).split("),(")
        multi += len(rings) > 1
        outer = rings[0]
        xy = [tuple(map(float, p.split())) for p in outer.split(",")]
        ring = [UTMK_TO_WGS84.transform(x, y) for x, y in xy]
        lon, lat = UTMK_TO_WGS84.transform(float(r.중심점utmkx좌표), float(r.중심점utmky좌표))
        casualties = sum(int(getattr(r, c)) for c in ("총사망자수", "총중상자수", "총경상자수", "총부상신고자수"))
        rows.append(("사고위험지역", r.사고위험지역id, r.사고위험지역명, int(r.츙사고건수), casualties, int(r.총사망자수),
                     lon, lat, ring_text(ring)))
    print(f"사고위험지역: 연도 {risk.연도코드.max()} {len(latest):,}건, POLYGON이 아닌 행 {skipped}건, 고리가 둘 이상인 행 {multi}건")

    out = pd.DataFrame(rows, columns=["유형", "id", "이름", "사고건수", "사상자수", "사망자수", "경도", "위도", "폴리곤"])
    write_csv(OUT, out)
    print(f"{OUT.name}: {len(out):,}건, 점 수 중앙값 {out.폴리곤.str.count(',').median() + 1:.0f}")


if __name__ == "__main__":
    main()

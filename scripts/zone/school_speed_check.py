"""어린이보호구역 중심에서 반경 안을 지나는 표준노드링크 링크의 제한속도(MAX_SPD) 분포를 출력한다.

- 보호구역은 maps/주의운전/보호구역.csv의 child 행이다(scripts/caution/build_zones.py로 만든다)
- 반경은 인자로 받으며 기본 300m는 nav-api 보호구역 경고(caution/zone.rs ZONE_RADIUS_M)와 같다
- 고속국도(101)·도시고속국도(102)는 따로 센다
- 링크 수, MAX_SPD별 링크 수와 길이(km), 30 초과 링크의 도로등급별 수를 출력한다

python -X utf8 scripts/zone/school_speed_check.py [반경 m. 기본 300]
"""
import sys
from pathlib import Path

import geopandas as gpd
import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT
from nodelink import TM, points, read_links

RADIUS_M = int(sys.argv[1]) if len(sys.argv) > 1 else 300
RANK = {"101": "고속국도", "102": "도시고속국도", "103": "일반국도", "104": "특별광역시도",
        "105": "국가지원지방도", "106": "지방도", "107": "시군도", "108": "기타"}


def main():
    zones = pd.read_csv(ROOT / "maps" / "주의운전" / "보호구역.csv", encoding="utf-8-sig")
    child = points(zones[zones.kind == "child"].reset_index(drop=True), lat="lat", lon="lon")
    child["geometry"] = child.buffer(RADIUS_M)
    links = read_links(["LINK_ID", "ROAD_RANK", "MAX_SPD", "LENGTH"]).to_crs(TM)
    hit = gpd.sjoin(links, child[["geometry"]], predicate="intersects", how="inner")
    inside = links[links.LINK_ID.isin(hit.LINK_ID.unique())].copy()
    inside["MAX_SPD"] = pd.to_numeric(inside.MAX_SPD, errors="coerce").fillna(0).astype(int)
    express = inside.ROAD_RANK.isin(["101", "102"])
    print(f"어린이보호구역 {len(child):,}곳, 반경 {RADIUS_M}m 안을 지나는 링크 {len(inside):,}개"
          f"(고속국도·도시고속국도 {express.sum():,}개 제외하면 {(~express).sum():,}개)")
    road = inside[~express]
    by = road.groupby("MAX_SPD").agg(links=("LINK_ID", "size"), km=("LENGTH", lambda s: s.astype(float).sum() / 1000))
    print("일반도로 링크의 MAX_SPD별 링크 수와 길이")
    for spd, r in by.iterrows():
        print(f"  {spd:3d}km/h  {int(r.links):7,}개  {r.km:9,.1f}km")
    over = road[road.MAX_SPD > 30]
    print(f"MAX_SPD 30 초과 {len(over):,}개({len(over) / len(road):.1%}), 30 이하 {(road.MAX_SPD <= 30).sum():,}개")
    print("30 초과 링크의 도로등급:", {RANK.get(k, k): v for k, v in over.ROAD_RANK.value_counts().items()})


if __name__ == "__main__":
    main()

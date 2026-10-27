"""유료도로 영업소를 표준노드링크·국가관심지점과 대조하며 생긴 문제를 목록으로 만든다(12 문서).

입력은 maps/유료도로의 영업소_요금지점.csv(match_units_moct.py), 요금지점_링크.csv(match_toll_links.py)와 표준노드링크 MOCT_LINK이다.
- 영업소_위치없음: 요금 지점을 정하지 못한 영업소(기준 "없음")
- 영업소_요금자료_불일치: 영업소 목록(ETC_75)에는 가동 영업소인데 요금소가 없거나 요금표(TCS_AJ)에 통행이 없어 뺀 영업소(기준 "제외" 중 IC 구간 요금과 구간 처리 제외)
- 요금소노드_없음_불일치: 노드링크에 그 영업소의 요금소 노드가 없거나(검증 "MOCT 요금소 노드 없음") 참조 좌표와 맞지 않는(검증 "불일치") 영업소
- 요금지점링크_사용불가: 요금 지점 링크가 MOCT_LINK에 없거나 ROAD_USE=1인 행
출력은 docs/15-데이터-문제/한국도로공사_유료도로/ 아래 CSV(UTF-8 BOM)이다

python -X utf8 scripts/report/toll_issues.py
"""
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT
from nodelink import read_links

SRC = ROOT / "maps" / "유료도로"
OUT = ROOT / "docs" / "15-데이터-문제" / "한국도로공사_유료도로"


def save(df, name):
    OUT.mkdir(parents=True, exist_ok=True)
    df.to_csv(OUT / f"{name}.csv", index=False, encoding="utf-8-sig")
    print(f"{name}: {len(df):,}행")


def main():
    units = pd.read_csv(SRC / "영업소_요금지점.csv", dtype=str, encoding="utf-8-sig").fillna("")
    cols = ["영업소코드", "영업소명", "운영기관코드", "기준", "검증", "노드명", "위도", "경도", "참조출처", "참조명", "참조위도", "참조경도", "참조거리m", "비고"]
    save(units[units.기준 == "없음"][cols], "영업소_위치없음")
    ex = units[(units.기준 == "제외") & ~units.비고.str.startswith("IC 구간 요금") & ~units.비고.str.startswith("구간 처리")]
    save(ex[cols], "영업소_요금자료_불일치")
    save(units[units.검증.isin(["MOCT 요금소 노드 없음", "불일치"])][cols], "요금소노드_없음_불일치")

    links = read_links(["LINK_ID", "ROAD_USE"], geometry=False).set_index("LINK_ID").ROAD_USE
    tl = pd.read_csv(SRC / "요금지점_링크.csv", dtype=str, encoding="utf-8-sig").fillna("")
    tl["문제"] = ["MOCT_LINK에 없음" if x not in links.index else "ROAD_USE=1" if links[x] == "1" else "" for x in tl.LINK_ID]
    save(tl[tl.문제 != ""], "요금지점링크_사용불가")


if __name__ == "__main__":
    main()

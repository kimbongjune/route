"""유료도로 파일들의 건수와 매칭 결과를 집계한다.

python -X utf8 scripts/tollroad/stats.py
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import TOLLROAD_DIR, read_csv


def main():
    units = read_csv(TOLLROAD_DIR / "ETC_75_04_01_168858.csv").apply(lambda s: s.str.strip())
    live = units[(units.가동영업소여부 == "Y") & (units.가상영업소여부 == "N")]
    print("ETC_75 영업소", len(units), "가동 Y·가상 N", len(live))
    print("  영업방식구분코드", live.영업방식구분코드.value_counts().to_dict())
    print("  운영기관코드 종류", live.고속도로운영기관구분코드.nunique())

    fares = read_csv(TOLLROAD_DIR / "TCS_AJ_04_01_779961.csv")
    ops = fares[["고속도로운영기관코드", "고속도로운영기관명"]].drop_duplicates()
    print("TCS_AJ 요금 쌍", f"{len(fares):,}", "운영기관", len(ops), sorted(ops.고속도로운영기관명))
    used = set(fares.출발영업소코드) | set(fares.도착영업소코드)
    unused = live[~live.영업소코드.isin(used)]
    print("  TCS_AJ 출발·도착에 한 번도 안 나오는 가동 영업소", sorted(unused.영업소코드 + " " + unused.영업소명))

    f = read_csv(TOLLROAD_DIR / "유료도로_차종별요금.csv")
    print("유료도로_차종별요금", f"{len(f):,}행 도로", f.도로.nunique())
    poi = read_csv(TOLLROAD_DIR / "API_수집" / "국가관심지점_요금소.csv")
    print("국가관심지점 요금소", len(poi))

    pts = read_csv(TOLLROAD_DIR / "영업소_요금지점.csv")
    print("영업소_요금지점:", len(pts), "행, 영업소", pts.영업소코드.nunique())
    print("  기준별 행", pts.기준.value_counts().to_dict())
    print("  기준별 영업소", pts.groupby("기준").영업소코드.nunique().to_dict())
    moct = pts[pts.NODE_IDS != ""]
    print("  MOCT 노드 기준 행", len(moct), "검증 값", moct.검증.value_counts().to_dict())
    print("  없음 영업소", sorted(set(pts.영업소코드[pts.기준 == "없음"] + " " + pts.영업소명[pts.기준 == "없음"])))

    links = read_csv(TOLLROAD_DIR / "요금지점_링크.csv")
    with_link = set(links.영업소코드[links.LINK_ID != ""])
    no = links[links.LINK_ID == ""]
    print("요금지점_링크:", len(links), "행, 링크 있는 영업소", len(with_link), "링크 없는 영업소", sorted(set(no.영업소코드 + " " + no.영업소명)))
    print("  구간 처리 링크", links.영업소명[links.기준 == "구간"].value_counts(sort=False).to_dict())
    expanded = links[links.기준.str.contains("램프 확장")]
    print("  램프 확장 행", len(expanded), "영업소", expanded.영업소코드.nunique(), "| 링크", links.LINK_ID[links.LINK_ID != ""].nunique())



if __name__ == "__main__":
    main()

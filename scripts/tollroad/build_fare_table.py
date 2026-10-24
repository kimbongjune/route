"""통행료 계산용 표 두 개를 만든다.

입력: maps/유료도로/TCS_AJ_04_01_779961.csv  영업소 간 통행요금(CP949)
      maps/유료도로/ETC_75_04_01_168858.csv  영업소 마스터(CP949)
      maps/유료도로/유료도로_차종별요금.csv  TCS_AJ에 없는 44개 도로의 차종별 요금(build_toll_fares.py)
출력: maps/유료도로/요금표.csv  컬럼 출발코드, 도착코드, 운영기관코드, 운영기관명, 1종, 2종, 3종, 4종, 5종, 6종
      maps/유료도로/영업소.csv  컬럼 영업소코드, 영업소명, 운영기관코드, 영업방식구분코드(1 개방식, 2 폐쇄식, 3 갠트리형)

요금표는 TCS_AJ의 정상 금액이며 같은 (출발, 도착)이 운영기관별로 여러 행이면 모두 남긴다(원톨링은 더한다).
차종별요금은 ROAD_UNITS로 도로의 영업소 이름을 영업소코드에 잇는다. 영업소별 요금(도착 없음)은 출발=도착인 자기 쌍으로 넣고
그 영업소는 개방식(1)으로 표시하며, 구간 요금은 출발·도착 쌍으로 넣는다. 시간대가 있는 행과 ROAD_UNITS에 없는 이름은 뺀다.
영업소명이 "(개)"로 끝나는 영업소와 요금표에 자기 쌍만 있는 영업소는 개방식으로 표시한다. 영업소는 가동 Y, 가상 N인 646건이다.

python -X utf8 scripts/tollroad/build_fare_table.py
"""
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import TOLLROAD_DIR, write_csv

# 도로별 영업소 이름 → 영업소코드. 값이 목록이면 같은 요금을 여러 영업소에 넣는다(방향별 영업소).
# 이름이 없는 도로("")는 요금소가 하나이다. 영업소 위치가 없는 영업소(만덕센텀 동래·센텀, 인천김포 대곶·서김포통진)와
# 영업소코드가 분명하지 않은 이름(안양성남 동판교IC·삼막IC·시점부·여수대로IC, 서울문산 남고양IC·내포IC·행주산성JCT,
# 인천김포 남청라교차로·남향교차로·인천항사거리, 광주제2순환 램프·IC 요금)은 넣지 않았다.
ROAD_UNITS = {
    "인천국제공항고속도로": {"인천공항": "097", "북인천": "098", "청라": "099"},
    "수도권제1순환고속도로(일산~퇴계원)": {"호원": "026", "고양": "033", "통일로": "034", "양주": "035", "송추": "036", "별내": "037", "불암산": "038"},
    "용인서울고속도로": {"서수지": "084", "금토": "085"},
    "인천대교": {"": "083"},
    "인천김포고속도로": {"검단/양촌": "890", "남청라": "887", "북청라": "889"},
    "안양성남고속도로": {"북의왕IC": "043", "북청계IC": "042"},
    "서울문산고속도로": {"고양JCT": ["822", "823"], "금촌IC": "826", "북고양(설문)IC": "825", "사리현IC": "824", "산단IC": "828", "월롱IC": "827", "흥도IC": "045"},
    "우면산터널": {"": "008"},
    "강남순환도시고속도로": {"금천": "019", "선암": "060"},
    "앞산터널로": {("상인", "파동"): "049", ("파동", "범물"): "050"},
    "광주제2순환도로": {"소태(본선, 지선A-B)": "016", "송암(본선)": "017", "유덕(본선)": "018"},
    "울산대교": {"대교구간(울산대교~애전영업소)": "027", "전구간(울산대교~염포산영업소)": "028"},
    "일산대교": {"": "044"},
    "제3경인고속화도로": {"고잔": "088", "물왕": "086", "연성": "087"},
    "수석호평도시고속도로": {"": "071"},
    "서수원의왕고속화도로": {"": "031"},
    "덕송내각고속화도로": {"동별내": "007", "서별내": "006"},
    "비봉매송도시고속도로": {"": "005"},
    "수원북부순환로": {"장안(본선)": "021"},
    "미시령터널": {"": "070"},
    "마창대교": {"": "096"},
    "창원부산간도로": {"녹산": "079", "창원": "078"},
    "부산항대교": {"": "090"},
    "청라하늘대교": {"": "032"},
    "용마터널": {"": "030"},
    "수정산터널": {"": "080"},
    "을숙도대교": {"": "075"},
    "거가대로": {"": ["076", "077"]},
    "광안대로": {"": "094"},
}
# 도로마다 다른 차종 표기를 1~6종 열에 넣는 자리. 소형은 1종, 중형은 2·3종, 대형은 4·5종, 경차는 6종이다
CLASS_COLUMNS = {
    "1종": [1], "2종": [2], "3종": [3], "4종": [4], "5종": [5], "6종": [6],
    "경차": [6], "경형": [6], "6종(경차)": [6], "6종(경형자동차)": [6],
    "소형": [1], "소형차": [1], "1종(소형차)": [1], "소형(1종)": [1], "일반차량": [1, 2, 3],
    "중형": [2, 3], "중형차": [2, 3], "2종(중형차)": [2], "3종(대형차)": [3], "중형(2종)": [2], "중형(3종)": [3],
    "대형": [4, 5], "대형차": [4, 5], "4종(대형화물차)": [4], "5종(특수화물차)": [5], "대형(4종)": [4], "대형(5종)": [5], "특대형차": [5],
}


def road_fares(cf, units):
    """차종별요금을 요금표 행으로 바꾼다. 반환값은 (행 목록, 개방식으로 표시할 영업소코드 집합, 뺀 행 수)"""
    rows, open_units, skipped = {}, set(), 0
    for r in cf.itertuples(index=False):
        if r.시간대 or r.도로 not in ROAD_UNITS:
            skipped += 1
            continue
        names = ROAD_UNITS[r.도로]
        key = (r.출발, r.도착) if (r.출발, r.도착) in names else None
        if key is not None:
            codes = names[key]
            pairs = [(c, c) for c in (codes if isinstance(codes, list) else [codes])]
            is_open = True
        elif r.도착 == "":
            if r.출발 not in names:
                skipped += 1
                continue
            codes = names[r.출발]
            pairs = [(c, c) for c in (codes if isinstance(codes, list) else [codes])]
            is_open = True
        else:
            if r.출발 not in names or r.도착 not in names:
                skipped += 1
                continue
            a, b = names[r.출발], names[r.도착]
            pairs = [(x, y) for x in (a if isinstance(a, list) else [a]) for y in (b if isinstance(b, list) else [b]) if x != y]
            is_open = False
        cols = CLASS_COLUMNS.get(r.차종)
        if cols is None:
            skipped += 1
            continue
        for a, b in pairs:
            row = rows.setdefault((a, b), {"출발코드": a, "도착코드": b, "운영기관코드": units.get(a, ""), "운영기관명": r.도로,
                                           **{f"{k}종": 0 for k in range(1, 7)}})
            for k in cols:
                if row[f"{k}종"] == 0:
                    row[f"{k}종"] = int(r.요금)
            if is_open:
                open_units.add(a)
    return list(rows.values()), open_units, skipped


def main():
    aj = pd.read_csv(TOLLROAD_DIR / "TCS_AJ_04_01_779961.csv", encoding="cp949", dtype=str)
    aj.columns = [c.strip() for c in aj.columns]
    fare = pd.DataFrame({
        "출발코드": aj["출발영업소코드"].str.strip(),
        "도착코드": aj["도착영업소코드"].str.strip(),
        "운영기관코드": aj["고속도로운영기관코드"].str.strip(),
        "운영기관명": aj["고속도로운영기관명"].str.strip(),
    })
    for k in range(1, 7):
        fare[f"{k}종"] = pd.to_numeric(aj[f"정상{k}종금액"], errors="coerce").fillna(0).astype(int)
    fare = fare.drop_duplicates(["출발코드", "도착코드", "운영기관코드"])

    etc = pd.read_csv(TOLLROAD_DIR / "ETC_75_04_01_168858.csv", encoding="cp949", dtype=str)
    etc = etc[(etc["가동영업소여부"].str.strip() == "Y") & (etc["가상영업소여부"].str.strip() == "N")]
    units = pd.DataFrame({
        "영업소코드": etc["영업소코드"].str.strip(),
        "영업소명": etc["영업소명"].str.strip(),
        "운영기관코드": etc["고속도로운영기관구분코드"].str.strip(),
        "영업방식구분코드": etc["영업방식구분코드"].str.strip(),
    })

    cf = pd.read_csv(TOLLROAD_DIR / "유료도로_차종별요금.csv", encoding="utf-8-sig", dtype=str).fillna("")
    extra, open_units, skipped = road_fares(cf, dict(zip(units.영업소코드, units.운영기관코드)))
    fare = pd.concat([fare, pd.DataFrame(extra)], ignore_index=True)
    fare = fare.sort_values(["출발코드", "도착코드", "운영기관코드"])
    write_csv(TOLLROAD_DIR / "요금표.csv", fare)

    # 요금표에 자기 쌍만 있는 영업소(대왕판교처럼 폐쇄식으로 표시된 개방식 요금소)도 개방식이다
    pair_codes = set(fare.loc[fare.출발코드 != fare.도착코드, "출발코드"]) | set(fare.loc[fare.출발코드 != fare.도착코드, "도착코드"])
    self_only = set(fare.loc[fare.출발코드 == fare.도착코드, "출발코드"]) - pair_codes
    open_mask = units.영업소코드.isin(open_units | self_only) | units.영업소명.str.endswith("(개)")
    units.loc[open_mask, "영업방식구분코드"] = "1"
    units = units.sort_values("영업소코드")
    write_csv(TOLLROAD_DIR / "영업소.csv", units)
    print("요금표", len(fare), "행(차종별요금에서", len(extra), "행, 뺀 행", skipped, "), 자기쌍", int((fare.출발코드 == fare.도착코드).sum()),
          "영업소", len(units), "개방식", int((units.영업방식구분코드 == "1").sum()))


if __name__ == "__main__":
    main()

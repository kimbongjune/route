"""영업소코드(영업소 목록)와 카카오 톨게이트를 연결한다. 통행료 계산(TCS_AJ)에 사용한다.

대상: ETC_75 영업소 목록에서 가동영업소여부=Y, 가상영업소여부=N인 영업소.
방법:
  위치  도로공사 영업소 위치정보 API(locationinfoUnit, 실행 시 호출)에 좌표가 있으면
        반경 LOC_RADIUS_M 이내의 카카오 톨게이트를 모두 연결한다.
  이름  좌표가 없거나 반경 안에 카카오 톨게이트가 없으면 카카오 장소명에서 진입/진출·방향·램프 표기와 TG/톨게이트/요금소/영업소,
        '하이패스'를 제거한 이름이 영업소명(괄호 부분을 제거한 이름 포함)과 같은 카카오 톨게이트를 연결한다.
        영업소명이 목록에 두 번 이상 있으면 연결하지 않는다(같은이름).
        연결된 카카오 지점들이 서로 NAME_SPREAD_M 이상 떨어져 있으면 연결하지 않는다(흩어짐).
        도로공사 좌표가 있는데 반경 밖에서 이름만 같은 카카오 지점이 있으면 '확인필요'로 둔다.
2차(1차에서 연결되지 않은 영업소):
        영업소명 끝의 상/하/동/서/본선/지선, 괄호, 숫자, '터널'·'대교'·'지하도로'·'요금소'를
        (휴게소·JC·IC 영업소는 본 요금소와 다른 시설일 수 있어 제거하지 않는다)
        제거한 이름과 카카오 이름(같은 방식으로 정규화, 쉼표·괄호 제거)이 정확히 같은 카카오 톨게이트를 후보로 하고,
        후보 중 '같은 운영기관의 1차 연결 요금소들이 위치한 도로(API_수집/카카오_톨게이트_링크.csv의 최근접 링크 도로명)' 위에 있는 것만 남긴다.
        같은 운영기관에 1차 연결 요금소가 하나도 없으면(지자체 단일 요금소 등) 다른 운영기관의 도로 위에 있는 후보만 제외한다.
        1차에서 이름이 다른(정규화한 이름이 다른) 영업소에 이미 연결된 카카오 지점은 후보에서 제외한다.
        도로공사 좌표가 있으면 그 좌표에서 NAME_SPREAD_M 이내의 후보만 남긴다.
        남은 후보가 모두 NAME_SPREAD_M 이내면 연결(방법 '이름+운영기관도로').
3차(근거가 있는 이름 차이, 방법 '이름표(근거)'):
        ALIAS 표의 영업소코드 -> 카카오 장소명. 근거는 표 옆 주석.
4차(위치 직접 지정, 방법 '위치지정'):
        카카오 톨게이트 목록에 없는 영업소(휴게소 하이패스, JC 요금소 등)의 위치를 카카오의 다른 분류 장소(휴게소, 분기점, IC)나
        표준노드링크 노드, 도로공사 좌표로 지정한 것. MANUAL 표에 출처를 적었다.
출력: maps/유료도로/영업소_카카오_매칭.csv
  컬럼: 영업소코드, 영업소명, 운영기관코드, 방법, 상태, 카카오ID, 카카오장소명, 위도, 경도, 거리m
"""
import re
import sys
from collections import Counter
from pathlib import Path

import numpy as np
import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import TOLLROAD_DIR, env, get_json, read_csv, write_csv

LOC_RADIUS_M = 300
# 영업소코드 -> [(출처, 장소명, 위도, 경도)]
K, NL, EX, US = "카카오", "노드링크", "도로공사API", "사용자"
MANUAL = {
    "021": [(K, "장안TG(톨게이트, 수원시 장안구 송죽동 산 11-5)", 37.312382, 127.008505)],
    "081": [(NL, "신월요금소(노드, 서울 양천구 신월동, 신월여의지하도로 링크)", 37.526443, 126.840091)],
    "089": [(US, "정왕IC(사용자 지도)", 37.3801821, 126.7312249)],
    "950": [(US, "서초구 원지동 10-16(사용자 지도; 입구로 보이나 확실하지 않음)", 37.4607098, 127.0417013)],
    "226": [(US, "금강휴게소톨게이트(사용자 지도)", 36.2799565, 127.6728656)],
    "286": [(US, "통도사휴게소 톨게이트(사용자 지도, 푸드코트 근처 통도사IC 방향)", 35.4887002, 129.0899085), (US, "통도사휴게소 톨게이트(사용자 지도, SK엔크린 LPG 같은 줄)", 35.4880302, 129.0924850)],
    "710": [(US, "밀양하(사용자 지도; 출구·입구 모름)", 35.5178314, 128.7983577), (US, "밀양상(사용자 지도; 출구·입구 모름)", 35.5173619, 128.7982901)],
    "720": [(US, "밀양하(사용자 지도; 출구·입구 모름)", 35.5178314, 128.7983577), (US, "밀양상(사용자 지도; 출구·입구 모름)", 35.5173619, 128.7982901)],
    "770": [(US, "울주(동)톨게이트(사용자 지도)", 35.4992746, 129.2221240)],
    "780": [(US, "울주(서)톨게이트(사용자 지도)", 35.4995512, 129.2223277)],
    "711": [(K, "불갑산하이패스TG(군서면 보라리 79-1)", 35.240004, 126.507461),
            (K, "불갑산하이패스TG(영광읍 학정리 산 143-31)", 35.251039, 126.520154)],
    "717": [(K, "불갑산하이패스TG(군서면 보라리 79-1)", 35.240004, 126.507461),
            (K, "불갑산하이패스TG(영광읍 학정리 산 143-31)", 35.251039, 126.520154)],
    "732": [(US, "행담도 TG(네이버 지도 표시, 휴게소에서 서해안고속도로로 나가는 램프 위; 국가POI 행담IC 36.943161,126.809349 는 본선 교차점)", 36.9443906, 126.8097252)],
    "046": [(US, "흥도IC(사용자 지도)", 37.6209113, 126.8587165)],
    "822": [(K, "고양JC남TG(톨게이트)", 37.662980, 126.851780)],
    "823": [(K, "고양JC북TG(톨게이트)", 37.672534, 126.854310)],
    "607": [(K, "논산천안고속도로 연무영업소(관리,운영)", 36.144392, 127.057196)],
    "795": [(K, "북용인JC(분기점)", 37.271027, 127.243977)],
    "879": [(K, "양주TG(양주시 회암동 528-1; 장흥면 양주TG는 수도권제1순환 양주(개)에 연결됨)", 37.840910, 127.081850)],
    "729": [(NL, "신둔하이패스IC(노드 2300247800)", 37.291036, 127.377924)],
    "736": [(K, "삼성하이패스TG 양방향(음성군 삼성면; 도로공사 좌표 37.185579,127.441411 근처엔 없음)", 37.022379, 127.481991)],
    "220": [(US, "덕평톨게이트(하이패스전용, 덕평자연휴게소 안; 사용자 지도 확인)", 37.2413826, 127.3886334)],
    "667": [(US, "경기광주JC 요금소(사용자 지도)", 37.3737112, 127.3247383)],
    "668": [(US, "신평톨게이트(사용자 지도)", 37.3888806, 127.8852067)],
    "669": [(US, "원주JCT 요금소(사용자 지도)", 37.3868206, 127.9215257)],
    "657": [(US, "동산톨게이트(사용자 지도)", 37.7573782, 127.7417815)],
    "648": [(US, "해운대IC(사용자 지도)", 35.2120083, 129.2050243)],
    "681": [(NL, "장안TG(노드, 화성시 장안면, 평택시흥고속도로 링크)", 37.092881, 126.845138)],
    "600": [(US, "낙동분기점(사용자 지도)", 36.3661245, 128.2393881)],
    "620": [(K, "영천TG(북안면 원당리; 상주영천고속도로 본선 위. 사용자 좌표 35.9249310,128.9509349는 경부선 영천TG라서 안 씀)", 35.940245, 129.017653)],
    "650": [(US, "군위(상)톨게이트(사용자 지도; 출구·입구 모름, 군위하는 못 찾음)", 36.1552292, 128.5608239)],
    "670": [(US, "군위(상)톨게이트(사용자 지도; 출구·입구 모름, 군위하는 못 찾음)", 36.1552292, 128.5608239)],
    "680": [(US, "화산(하)톨게이트(사용자 지도; 출구·입구 모름, 화산상은 못 찾음)", 36.0253175, 128.8863057)],
    "690": [(US, "화산(하)톨게이트(사용자 지도; 출구·입구 모름, 화산상은 못 찾음)", 36.0253175, 128.8863057)],
    "671": [(K, "봉담TG(경기고속도로 봉담영업소, 봉담읍 동화리 51-13; 도로공사 좌표와 1.8km 차이)", 37.223987, 126.960576)],
    "740": [(NL, "화도JCTG(노드 2220309400; 상·하 방향 모름)", 37.629267, 127.318304),
            (NL, "화도JCTG(노드 2220309800; 상·하 방향 모름)", 37.630347, 127.317913)],
    "790": [(NL, "화도JCTG(노드 2220309400; 상·하 방향 모름)", 37.629267, 127.318304),
            (NL, "화도JCTG(노드 2220309800; 상·하 방향 모름)", 37.630347, 127.317913)],
    "837": [(NL, "중리TG(노드 2330529101)", 37.201077, 127.129757)],
    "548": [(US, "북무안IC(사용자 지도; 근처 무안공항톨게이트 35.0147296,126.4355799는 무안공항 영업소)", 35.0153582, 126.4374875)],
    "751": [(US, "기장일광톨게이트 일광읍 화전리 194-13(746 기장일광입구와 같은 부산외곽순환선 램프 요금소; 카카오 기장일광TG 화전리 210은 울산포항선 646 자리)", 35.2781895, 129.2268066)],
    "746": [(US, "기장일광톨게이트 일광읍 화전리 194-13(사용자 지도; 도로공사 API 노선이 부산외곽순환선이고, 후보 3곳 중 부산외곽순환고속도로 램프 위는 이곳뿐)", 35.2781895, 129.2268066)],
    "750": [(US, "기장동분기점톨게이트(사용자 지도)", 35.2823319, 129.2123570)],
    "760": [(US, "기장분기점(사용자 지도)", 35.2812516, 129.2201926)],
    "754": [(NL, "김해가야하이패스IC(노드 3850325001)", 35.270838, 129.004052)],
    "756": [(NL, "한림하이패스IC(노드 3850368402)", 35.294884, 128.813914)],
    "758": [(US, "진영휴게소 톨게이트(사용자 지도)", 35.2765681, 128.7170201)],
}

ALIAS = {
    # 천마산터널㈜(부산시 유료도로 현황 관리운영권자). 카카오 천마TG는 서구 암남동 95-59, 구간 '사하구 장림동~서구 암남동'
    "052": ["천마TG"],
    # 수정산투자㈜. 카카오 수정터널TG는 부산진구 가야동 553, 구간 '동구 좌천1동 ~ 부산진구 가야동 568-12'
    "080": ["수정터널TG"],
    # 북항아이브리지㈜. 카카오 영도TG·감만TG는 남구 감만동 254-16, 구간 '영도구 청학동 ~ 남구 감만동', 항공사진에 요금소
    "090": ["영도TG", "감만TG"],
    # 광안대로 구간 '수영구 남천동 49호 광장 ~ 해운대구 우동 센텀시티'. 카카오 광안대교TG는 남구 대연동 1843
    "094": ["광안대교TG"],
    # 앞산터널로(운영기관 20: 앞산·파동). 카카오 파동A·B·C TG는 수성구 파동 546(앞산TG와 같은 주소)
    "050": ["파동ATG", "파동BTG", "파동CTG"],
    # 호남선지선 양촌. 카카오 '양촌하이패스TG 순천방면/천안방면'
    "599": ["양촌하이패스TG 순천방면", "양촌하이패스TG 천안방면"],
}
NAME_SPREAD_M = 1000


def dist_m(lat1, lon1, lat2, lon2):
    lat = np.radians((lat1 + lat2) / 2)
    return np.hypot((lon1 - lon2) * 111320 * np.cos(lat), (lat1 - lat2) * 110950)


def spread(df):
    """지점들 사이의 최대 거리."""
    if len(df) < 2:
        return 0.0
    la, lo = df.lat.to_numpy(), df.lon.to_numpy()
    return dist_m(la[:, None], lo[:, None], la[None, :], lo[None, :]).max()


def by_id(df, fn):
    """카카오ID에 fn을 적용한 결과가 참인 행만 남긴다."""
    return df.loc[df.카카오ID.map(fn).astype(bool)]


def base_name(n):
    n = n.replace(" ", "")
    n = re.sub(r"(진입|진출|입구|출구|상행|하행|서울방향|부산방향|양방향|[A-Z]램프)$", "", n)
    n = re.sub(r"(하이패스)?(TG|톨게이트|요금소|영업소)$", "", n)
    return re.sub(r"^한국도로공사", "", n)


def clean(n):
    n = re.sub(r"\(.*?\)", "", n.replace(" ", "").replace(",", ""))
    while True:  # 서오창동JC처럼 끝말이 여러 겹이면 반복해서 제거한다
        m = re.sub(r"(본선|지선|상|하|동|서|\d|터널|대교|지하도로|요금소)$", "", n)
        if m == n or len(m) < 2:
            return n
        n = m


def fetch_unit_xy(key):
    """도로공사 영업소 위치정보 API에서 좌표가 있는 영업소를 읽는다. 한 페이지 최대 99건이다."""
    pages, page = [], 1
    while True:
        d = get_json(f"https://data.ex.co.kr/openapi/locationinfo/locationinfoUnit?key={key}&type=json&numOfRows=99&pageNo={page}",
                     secrets=(key,), timeout=120)
        pages.append(pd.DataFrame(d.get("list", []), columns=["unitCode", "xValue", "yValue"], dtype=object))
        if page >= int(d.get("pageSize") or 1):
            break
        page += 1
    df = pd.concat(pages).fillna("")
    df = df[(df.xValue.str.strip() != "") & (df.yValue.str.strip() != "")]
    df = df.assign(unitCode=df.unitCode.str.strip()).drop_duplicates("unitCode", keep="last")
    return {c: (float(y), float(x)) for c, x, y in zip(df.unitCode, df.xValue, df.yValue)}


def main():
    api_xy = fetch_unit_xy(env("EX_ROAD_API_KEY"))
    print("위치정보 API 좌표 있는 영업소", len(api_xy))

    units = read_csv(TOLLROAD_DIR / "ETC_75_04_01_168858.csv")
    units = units[(units.가동영업소여부 == "Y") & (units.가상영업소여부 == "N")].copy()
    units["코드"] = units.영업소코드.str.strip()
    units["운영기관"] = units.고속도로운영기관구분코드.str.strip()
    dup = units.영업소명.value_counts()
    kakao = read_csv(TOLLROAD_DIR / "API_수집" / "카카오_톨게이트_전국.csv")
    kakao["lat"], kakao["lon"] = kakao.위도.astype(float), kakao.경도.astype(float)
    kakao["이름"] = kakao.장소명.map(base_name)

    rows, stat = [], Counter()
    for u in units.itertuples():
        code, name = u.코드, u.영업소명
        base = {"영업소코드": code, "영업소명": name, "운영기관코드": u.운영기관}
        hits = kakao.iloc[0:0].assign(d=None)
        if code in api_xy:
            la, lo = api_xy[code]
            box = kakao[((kakao.lat - la).abs() < 0.01) & ((kakao.lon - lo).abs() < 0.012)]
            hits = box.assign(d=dist_m(la, lo, box.lat, box.lon)).sort_values("d", kind="stable")
            hits = hits[hits.d <= LOC_RADIUS_M]
        if len(hits):
            method, status = "위치", "연결"
        else:
            short = re.sub(r"\(.*?\)", "", name)
            cand = kakao[kakao.이름 == name.replace(" ", "")]
            if short != name:
                cand = pd.concat([cand, kakao[kakao.이름 == short]])
            hits = cand.assign(d=dist_m(*api_xy[code], cand.lat, cand.lon) if code in api_xy else None)
            method = "이름" if code not in api_xy else "이름(위치 반경 안에 없음)"
            if dup[name] > 1:
                status = "같은이름"
            elif not len(cand):
                status = "없음"
            elif spread(cand) > NAME_SPREAD_M:
                status = "흩어짐"
            elif code in api_xy:
                status = "확인필요"  # 이름은 같지만 도로공사 좌표 반경 밖
            else:
                status = "연결"
        stat[(method, status)] += 1
        if not len(hits):
            rows.append({**base, "방법": method, "상태": status})
        for k in hits.itertuples():
            rows.append({**base, "방법": method, "상태": status, "카카오ID": k.카카오ID, "카카오장소명": k.장소명,
                         "거리m": "" if k.d is None or pd.isna(k.d) else round(k.d)})
    first = pd.DataFrame(rows, dtype=object)

    # 2차: 운영기관 도로로 거르기
    near = read_csv(TOLLROAD_DIR / "API_수집" / "카카오_톨게이트_링크.csv")
    road_of = dict(near.loc[near.거리순위 == "1", ["카카오ID", "ROAD_NAME"]].values)
    linked = first[first.상태 == "연결"]
    on = by_id(linked, lambda k: road_of.get(k, "") not in ("", "-"))
    op_roads = on.groupby("운영기관코드").카카오ID.agg(lambda s: {road_of[k] for k in s}).to_dict()

    kakao["정리"] = kakao.장소명.str.replace(",", "").map(base_name).map(clean)
    done = set(linked.영업소코드)
    taken = linked.dropna(subset=["카카오ID"]).groupby("카카오ID").영업소명.agg(lambda s: {clean(n) for n in s}).to_dict()
    rows2 = [r for r in rows if r["영업소코드"] in done]
    stat = Counter()
    for u in units.itertuples():
        code = u.코드
        if code in done:
            stat[("1차", "연결")] += 1
            continue
        name, op = u.영업소명, u.운영기관
        base = {"영업소코드": code, "영업소명": name, "운영기관코드": op}
        cn = clean(name)
        cand = kakao[(kakao.정리 == cn) & (len(cn) >= 2)]
        cand = by_id(cand, lambda k: not taken.get(k) or cn in taken[k])
        if code in api_xy:
            cand = cand.loc[dist_m(*api_xy[code], cand.lat, cand.lon) <= NAME_SPREAD_M]
        if op_roads.get(op):
            on_road = by_id(cand, lambda k: road_of.get(k) in op_roads[op])
        else:  # 이 운영기관에 연결된 요금소가 없으면 다른 운영기관 도로 위의 지점만 제외한다
            others = set().union(*(v for o, v in op_roads.items() if o != op))
            on_road = by_id(cand, lambda k: road_of.get(k) not in others)
        if len(on_road) and spread(on_road) <= NAME_SPREAD_M:
            status = "연결"
            for k in on_road.itertuples():
                rows2.append({**base, "방법": "이름+운영기관도로", "상태": status, "카카오ID": k.카카오ID, "카카오장소명": k.장소명,
                              "거리m": round(dist_m(*api_xy[code], k.lat, k.lon)) if code in api_xy else ""})
        else:
            status = "흩어짐" if len(on_road) else "없음"
            prev = [r for r in rows if r["영업소코드"] == code]
            rows2.extend(prev if prev else [{**base, "방법": "이름+운영기관도로", "상태": status}])
            status = prev[0]["상태"] if prev else status
        stat[("2차", status)] += 1

    by_code = units.set_index("코드")
    # 3차: 이름표
    for code, places in ALIAS.items():
        if any(r["영업소코드"] == code and r["상태"] == "연결" for r in rows2):
            continue
        hits = kakao[kakao.장소명.isin(places)]
        hits = hits.iloc[hits.장소명.map(places.index).argsort(kind="stable")]
        if not len(hits):
            continue
        u = by_code.loc[code]
        rows2 = [r for r in rows2 if r["영업소코드"] != code]
        for k in hits.itertuples():
            rows2.append({"영업소코드": code, "영업소명": u.영업소명, "운영기관코드": u.운영기관,
                          "방법": "이름표(근거)", "상태": "연결", "카카오ID": k.카카오ID, "카카오장소명": k.장소명, "거리m": ""})
    # 4차: 위치 직접 지정. 지도에서 직접 지정한 사용자 위치는 앞 단계 결과를 덮어쓴다
    for code, places in MANUAL.items():
        if any(r["영업소코드"] == code and r["상태"] == "연결" for r in rows2) and not any(p[0] == US for p in places):
            continue
        u = by_code.loc[code]
        rows2 = [r for r in rows2 if r["영업소코드"] != code]
        for src, pl, la, lo in places:
            rows2.append({"영업소코드": code, "영업소명": u.영업소명, "운영기관코드": u.운영기관,
                          "방법": "위치지정(" + src + ")", "상태": "연결", "카카오장소명": pl, "위도": la, "경도": lo})

    out = pd.DataFrame(rows2, dtype=object)
    xy = kakao.set_index("카카오ID")[["lat", "lon"]]
    fill = out.위도.isna() & out.카카오ID.isin(xy.index)
    out.loc[fill, "위도"] = out.카카오ID[fill].map(xy.lat)
    out.loc[fill, "경도"] = out.카카오ID[fill].map(xy.lon)
    cols = ["영업소코드", "영업소명", "운영기관코드", "방법", "상태", "카카오ID", "카카오장소명", "위도", "경도", "거리m"]
    path = TOLLROAD_DIR / "영업소_카카오_매칭.csv"
    write_csv(path, out, cols)
    print("영업소", len(units), dict(stat))
    print("최종", out.drop_duplicates("영업소코드").상태.value_counts(sort=False).to_dict())
    print("저장:", path)


if __name__ == "__main__":
    main()

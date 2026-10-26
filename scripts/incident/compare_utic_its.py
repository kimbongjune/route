"""경찰청 UTIC 돌발정보(imsOpenData.do)와 국가교통정보센터 돌발상황정보(/eventInfo)를 같은 시각에 받아 비교한다.

- UTIC 키는 .env의 UTIC_API_KEY(등록 IP 대역에서만 동작), its 키는 ITS_API_KEY이다
- 건수, 링크ID가 있는 건수, 링크ID가 같은 건수, 200m 안에 상대 이벤트가 있는 건수를 출력한다
- UTIC은 돌발유형(incidenteTypeCd)·출처 코드(sourceCode)·통제정보(controlType)별 수와 통제 이벤트 목록, its는 도로 구분(type)·이벤트유형별 수를 출력한다
- 종료 일시가 있는 건수와 차단 차로 정보가 있는 건수를 출력한다
- 한쪽에만 있는 이벤트를 5건씩 출력한다

python -X utf8 scripts/incident/compare_utic_its.py
"""
import json
import math
import sys
import xml.etree.ElementTree as ET
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import env, http_get

NEAR_M = 200
# UTIC 코드정의서 Incident_Code_20260630.xlsx(https://www.utic.go.kr/upload/Incident_Code_20260630.xlsx)의
# incidentTypeCd, ControlType 시트이다
UTIC_TYPE = {"1": "사고", "2": "공사", "3": "행사", "4": "기상", "5": "통제", "6": "재난", "9": "기타"}
UTIC_CONTROL = {"0": "없음", "1": "진행방향 통제", "2": "양방향 통제"}


def utic():
    key = env("UTIC_API_KEY")
    root = ET.fromstring(http_get(f"http://www.utic.go.kr/guide/imsOpenData.do?key={key}", secrets=(key,)))
    out = []
    for r in root.iter("record"):
        g = {c.tag: (c.text or "").strip() for c in r}
        try:
            lon, lat = float(g.get("locationDataX") or 0), float(g.get("locationDataY") or 0)
        except ValueError:
            lon, lat = 0.0, 0.0
        out.append({**g, "lat": lat, "lon": lon})
    return out


def its():
    key = env("ITS_API_KEY")
    d = json.loads(http_get(f"https://openapi.its.go.kr:9443/eventInfo?apiKey={key}&type=all&eventType=all&getType=json",
                            secrets=(key,)))
    out = []
    for x in d["body"]["items"] or []:
        try:
            lon, lat = float(x.get("coordX") or 0), float(x.get("coordY") or 0)
        except ValueError:
            lon, lat = 0.0, 0.0
        out.append({**x, "lat": lat, "lon": lon})
    return out


def dist(a, b):
    return math.hypot((a["lon"] - b["lon"]) * 88_000, (a["lat"] - b["lat"]) * 110_950)


def near_any(e, others):
    return any(o["lat"] and dist(e, o) <= NEAR_M for o in others)


def main():
    u, i = utic(), its()
    u_links = {e["linkId"] for e in u if e.get("linkId")}
    i_links = {e["linkId"] for e in i if e.get("linkId")}
    print(f"UTIC {len(u)}건, 링크ID 있음 {sum(1 for e in u if e.get('linkId'))}건, 좌표 있음 {sum(1 for e in u if e['lat'])}건")
    print(f"its {len(i)}건, 링크ID 있음 {sum(1 for e in i if e.get('linkId'))}건, 좌표 있음 {sum(1 for e in i if e['lat'])}건")
    print(f"링크ID 수 UTIC {len(u_links)}, its {len(i_links)}, 같은 링크ID {len(u_links & i_links)}")
    u_near = [e for e in u if e["lat"] and near_any(e, i)]
    i_near = [e for e in i if e["lat"] and near_any(e, u)]
    print(f"{NEAR_M}m 안에 상대 이벤트가 있는 건수 UTIC {len(u_near)}/{len(u)}, its {len(i_near)}/{len(i)}")
    print("UTIC 돌발유형 코드:", Counter(UTIC_TYPE.get(e.get("incidenteTypeCd"), e.get("incidenteTypeCd")) for e in u).most_common())
    print("UTIC 출처 코드:", Counter(e.get("sourceCode") for e in u).most_common())
    print("UTIC 통제유형:", Counter(UTIC_CONTROL.get(e.get("controlType"), e.get("controlType")) for e in u).most_common())
    print(f"UTIC 종료 일시 있음 {sum(1 for e in u if e.get('endDate'))}건, 차로 정보 있음 {sum(1 for e in u if e.get('lane'))}건")
    print("its 도로 구분:", Counter(e.get("type") for e in i).most_common())
    print("its 이벤트유형:", Counter(e.get("eventType") for e in i).most_common())
    print(f"its 종료 일시 있음 {sum(1 for e in i if e.get('endDate'))}건, 차단 차로 있음 {sum(1 for e in i if e.get('lanesBlocked'))}건")
    controlled = [e for e in u if e.get("controlType") in ("1", "2")]
    print(f"UTIC 통제 이벤트 {len(controlled)}건 중 200m 안에 its 이벤트가 있는 건수 {sum(1 for e in controlled if e in u_near)}")
    for e in controlled:
        print("  ", UTIC_CONTROL[e["controlType"]], e.get("incidentTitle", "")[:80])
    print("UTIC에만 있는 이벤트(앞 5건)")
    for e in [e for e in u if e not in u_near][:5]:
        print("  ", e.get("incidentTitle", "")[:90])
    print("its에만 있는 이벤트(앞 5건)")
    for e in [e for e in i if e not in i_near][:5]:
        print("  ", e.get("type"), e.get("eventType"), e.get("eventDetailType"), e.get("roadName"), (e.get("message") or "")[:60])


if __name__ == "__main__":
    main()

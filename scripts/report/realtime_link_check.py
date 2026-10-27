"""실시간 API가 주는 링크ID를 표준노드링크(2026-10-14판)와 대조한다. 한 번 호출한 시점의 결과이다.

- 국가교통정보센터(its): 소통정보 /trafficInfo(전국 1.5도 타일 16개), 돌발상황정보 /eventInfo, 주의운전구간 /posIncidentInfo
- 경찰청 도시교통정보센터(UTIC): 돌발정보 imsOpenData.do(등록 IP 대역에서만 동작). 링크는 lineLinkId(쉼표로 여러 개), 없으면 linkId이다
- 링크ID마다 MOCT_LINK에 없거나 ROAD_USE=1이면 기록하고, 좌표가 있는 이벤트는 좌표에서 그 링크까지의 거리가 200m보다 멀면 기록한다
출력은 docs/15-데이터-문제/<제공처>/ 아래 CSV(UTF-8 BOM)이다. 키는 .env의 ITS_API_KEY, UTIC_API_KEY이다

python -X utf8 scripts/report/realtime_link_check.py
"""
import json
import sys
import xml.etree.ElementTree as ET
from datetime import datetime
from pathlib import Path

import pandas as pd
from shapely.geometry import Point

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, env, http_get
from nodelink import read_links, wgs84_to_tm

OUT = ROOT / "docs" / "15-데이터-문제"
ITS = "https://openapi.its.go.kr:9443"
FAR_M = 200


def save(rows, folder, name):
    d = OUT / folder
    d.mkdir(parents=True, exist_ok=True)
    pd.DataFrame(rows).to_csv(d / f"{name}.csv", index=False, encoding="utf-8-sig")
    print(f"{folder}/{name}: {len(rows):,}행")


def check(events, L, source):
    """events: (이벤트ID, 링크ID, 위도, 경도, 설명) 목록. 문제가 있는 행을 반환한다"""
    rows = []
    flat = [(eid, one.strip(), lat, lon, text) for eid, lid, lat, lon, text in events
            for one in (lid or "").split(",") if one.strip() and one.strip() != "null"]
    for eid, lid, lat, lon, text in flat:
        problem, dist = "", None
        if lid not in L.index:
            problem = "링크ID가 MOCT_LINK에 없음"
        else:
            if L.at[lid, "ROAD_USE"] == "1":
                problem = "ROAD_USE=1 링크"
            if lat and lon:
                dist = round(L.at[lid, "geometry"].distance(Point(wgs84_to_tm(lat, lon))), 1)
                if dist > FAR_M:
                    problem = (problem + " / " if problem else "") + f"좌표와 링크가 {FAR_M}m보다 멂"
        if problem:
            rows.append({"출처": source, "이벤트ID": eid, "링크ID": lid, "문제": problem, "좌표_링크_거리_m": dist,
                         "위도": lat or None, "경도": lon or None, "내용": text})
    return rows


def num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return 0.0


def main():
    links = read_links(["LINK_ID", "ROAD_USE"])
    L = links.set_index("LINK_ID")
    key = env("ITS_API_KEY")
    stamp = datetime.now().strftime("%Y-%m-%d %H:%M")
    print("호출 시각", stamp)

    its_rows = []
    # 소통정보
    seen, total = set(), 0
    lon = 125.0
    while lon < 131.0:
        lat = 33.0
        while lat < 39.0:
            q = f"type=all&minX={lon:.3f}&maxX={lon + 1.5:.3f}&minY={lat:.3f}&maxY={lat + 1.5:.3f}&getType=json"
            d = json.loads(http_get(f"{ITS}/trafficInfo?apiKey={key}&{q}", timeout=120, secrets=(key,)))
            for x in (d.get("body") or {}).get("items") or []:
                total += 1
                if x["linkId"] not in seen:
                    seen.add(x["linkId"])
            lat += 1.5
        lon += 1.5
    traffic = [("", lid, 0, 0, "소통정보") for lid in sorted(seen)]
    t_rows = check(traffic, L, "its trafficInfo")
    print(f"trafficInfo 항목 {total:,}, 서로 다른 링크 {len(seen):,}, 문제 {len(t_rows):,}")
    if total == 0:
        print("  trafficInfo가 SUCCESS로 응답하였으나 항목이 0건이다")
    its_rows += t_rows

    d = json.loads(http_get(f"{ITS}/eventInfo?apiKey={key}&type=all&eventType=all&getType=json", secrets=(key,)))
    ev = [(x.get("eventId", ""), x.get("linkId"), num(x.get("coordY")), num(x.get("coordX")),
           f"{x.get('eventType', '')} {x.get('eventDetailType', '')} {x.get('roadName', '')} {x.get('message', '')}".strip())
          for x in (d.get("body") or {}).get("items") or []]
    e_rows = check(ev, L, "its eventInfo")
    print(f"eventInfo {len(ev):,}건, 문제 {len(e_rows):,}")
    its_rows += e_rows

    d = json.loads(http_get(f"{ITS}/posIncidentInfo?apiKey={key}&minX=124&maxX=132&minY=33&maxY=39&getType=json", secrets=(key,)))
    items = (d.get("body") or {}).get("items") or []
    pos = {}
    for x in items:
        pos.setdefault(x.get("occrrncId", ""), (x.get("occrrncId", ""), x.get("startStdLinkId"), num(x.get("startY")),
                                                num(x.get("startX")), f"{x.get('outbrkType', '')} {x.get('roadNm', '')}".strip()))
    p_rows = check(list(pos.values()), L, "its posIncidentInfo")
    print(f"posIncidentInfo 이벤트 {len(pos):,}, 문제 {len(p_rows):,}")
    its_rows += p_rows
    for r in its_rows:
        r["호출시각"] = stamp
    save(its_rows, "국가교통정보센터_실시간", "링크ID_문제")

    ukey = env("UTIC_API_KEY")
    root = ET.fromstring(http_get(f"http://www.utic.go.kr/guide/imsOpenData.do?key={ukey}", secrets=(ukey,)))
    ev = []
    for r in root.iter("record"):
        g = {c.tag: (c.text or "").strip() for c in r}
        lid = g.get("lineLinkId") if g.get("lineLinkId") not in (None, "", "null") else g.get("linkId")
        ev.append((g.get("incidentId", ""), lid, num(g.get("locationDataY")), num(g.get("locationDataX")),
                   g.get("incidentTitle", "")))
    u_rows = check(ev, L, "UTIC imsOpenData")
    for r in u_rows:
        r["호출시각"] = stamp
    print(f"UTIC {len(ev):,}건, 링크 있음 {sum(1 for e in ev if e[1] and e[1] != 'null'):,}, 문제 {len(u_rows):,}")
    save(u_rows, "경찰청_UTIC", "링크ID_문제")


if __name__ == "__main__":
    main()

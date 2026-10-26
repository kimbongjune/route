"""나가는 통행이 모두 회전정보(TURNINFO) 금지에 걸린 링크의 금지 회전을 카카오모빌리티 길찾기(거리 우선)와 대조한다.

- 입력: routedb-core 예제 turns_of --dead가 쓴 목록(LINK_ID,in|isolated)
  cargo run --release -p routedb-core --example turns_of -- maps/routedb/moct.rdb --dead <목록 파일>
- 대상: 들어오는 통행은 있고 나가는 통행이 없는 링크 A 중, 끝 노드에서 나가는 사용 링크(ROAD_USE=0, 자기 역방향 제외)가 있고
  A를 진입 링크로 하는 TURNINFO 행이 있는 것
- 금지 행(진입 A, 진출 B)마다 A 끝에서 20m 앞(A가 짧으면 가운데)을 출발, B 시작에서 20m 뒤(B가 짧으면 가운데)를 도착으로 카카오 경로를 구한다
- 바로 회전한 거리(두 점 사이를 A·B를 따라 잰 길이)의 1.5배 + 100m 이하이면 "카카오는 바로 회전", 넘으면 "카카오도 돌아감"으로 센다
- 카카오 결과는 저장하지 않고 판정과 거리만 출력한다. 키는 .env의 KAKAO_REST_API_KEY이다
- 먼저 들어오는 통행만 있는 링크 중 끝 노드에서 나가는 다른 링크가 원본에 있는 것을 원인별로 센다
  (이어지는 링크가 모두 ROAD_USE=1, 이어지는 사용 링크에 TURNINFO 금지, 25m 이하 링크로 이어진 교차로(NODE_TYPE 101) 묶음에서
  나가는 사용 링크가 없음, 그 밖)

python -X utf8 scripts/nodelink/turn_block_check.py <목록 파일> [--no-kakao]
"""
import json
import sys
import time
import urllib.request
from collections import Counter
from pathlib import Path

import geopandas as gpd
import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import env
from nodelink import WGS84, read_links, read_nodes, read_table

TYPE = {"001": "비보호회전", "002": "버스만회전", "003": "회전금지", "011": "U-TURN", "012": "P-TURN",
        "101": "좌회전금지", "102": "직진금지", "103": "우회전금지"}


def kakao(key, a, b):
    url = (f"https://apis-navi.kakaomobility.com/v1/directions?origin={a[1]},{a[0]}&destination={b[1]},{b[0]}"
           "&priority=DISTANCE&summary=true")
    req = urllib.request.Request(url, headers={"Authorization": "KakaoAK " + key})
    route = json.load(urllib.request.urlopen(req, timeout=30))["routes"][0]
    return route["summary"]["distance"] if route["result_code"] == 0 else None


def main():
    key = env("KAKAO_REST_API_KEY")
    dead = pd.read_csv(sys.argv[1], header=None, names=["LINK_ID", "kind"], dtype=str)
    links = read_links(["LINK_ID", "F_NODE", "T_NODE", "ROAD_USE", "ROAD_NAME"])
    links = links.set_index("LINK_ID")
    turns = read_table("TURNINFO")
    used = links[links.ROAD_USE == "0"]
    out_of = used.reset_index().groupby("F_NODE")[["LINK_ID", "T_NODE"]].apply(lambda g: list(zip(g.LINK_ID, g.T_NODE))).to_dict()
    rules_of = {k: g for k, g in turns.groupby("ST_LINK")}
    targets = []
    for lid in dead[dead.kind == "in"].LINK_ID:
        a = links.loc[lid]
        outs = [o for o, t in out_of.get(a.T_NODE, []) if t != a.F_NODE]
        rules = rules_of.get(lid)
        if outs and rules is not None:
            targets.append((lid, outs, rules))
    # 원인별 분류
    nodes = read_nodes(["NODE_ID", "NODE_TYPE"], geometry=False).set_index("NODE_ID").NODE_TYPE
    cross = set(nodes[nodes == "101"].index)
    near = {}
    for r in used.itertuples():
        if r.F_NODE in cross and r.T_NODE in cross and r.geometry.length <= 25:
            near.setdefault(r.F_NODE, []).append(r.T_NODE)
            near.setdefault(r.T_NODE, []).append(r.F_NODE)
    all_out = links.reset_index().groupby("F_NODE")[["LINK_ID", "T_NODE"]].apply(lambda g: list(zip(g.LINK_ID, g.T_NODE))).to_dict()
    cause = Counter()
    for lid in dead[dead.kind == "in"].LINK_ID:
        a = links.loc[lid]
        others = [o for o, t in all_out.get(a.T_NODE, []) if t != a.F_NODE]
        if not others:
            continue
        outs = [o for o, t in out_of.get(a.T_NODE, []) if t != a.F_NODE]
        if not outs:
            cause["이어지는 링크가 모두 ROAD_USE=1"] += 1
        elif lid in rules_of:
            cause["이어지는 사용 링크에 TURNINFO 금지"] += 1
        else:
            group, stack = {a.T_NODE}, [a.T_NODE]
            while stack:
                for w in near.get(stack.pop(), []):
                    if w not in group:
                        group.add(w)
                        stack.append(w)
            exits = [o for v in group for o, t in out_of.get(v, []) if t not in group]
            cause["교차로 묶음에서 나가는 사용 링크 없음" if len(group) > 1 and not exits else "그 밖"] += 1
    print(f"끝 노드에서 나가는 다른 링크가 원본에 있는 링크 {sum(cause.values())}개:", dict(cause))
    print(f"대상 링크 {len(targets)}개")
    if "--no-kakao" in sys.argv:
        return
    result = Counter()
    for lid, outs, rules in targets:
        a = links.loc[lid].geometry
        for r in rules.itertuples():
            if r.ED_LINK not in links.index:
                continue
            b = links.loc[r.ED_LINK].geometry
            da, db = min(20.0, a.length / 2), min(20.0, b.length / 2)
            pa, pb = a.interpolate(a.length - da), b.interpolate(db)
            pts = gpd.GeoSeries([pa, pb], crs=links.crs).to_crs(WGS84)
            km = kakao(key, (pts[0].y, pts[0].x), (pts[1].y, pts[1].x))
            time.sleep(0.1)
            direct = da + db
            if km is None:
                verdict = "카카오 경로 없음"
            elif km <= direct * 1.5 + 100:
                verdict = "카카오는 바로 회전"
            else:
                verdict = "카카오도 돌아감"
            result[verdict] += 1
            print(f"  {lid}({links.loc[lid].ROAD_NAME}) → {r.ED_LINK} {TYPE.get(r.TURN_TYPE, r.TURN_TYPE)}: "
                  f"바로 {direct:.0f}m, 카카오 {km}m, {verdict}")
    print(dict(result))


if __name__ == "__main__":
    main()

"""ROAD_USE=1(통행불가) 링크 중 통행 가능한 것으로 보이는 링크를 근거와 함께 뽑는다. 국가교통정보센터에 보내는 오류 목록이다.

근거 기준은 지능형교통체계 표준 노드·링크 구축기준(국토교통부고시 제2026-344호)과 국가교통정보센터 답변(2026-10-28)이다.
- 5.3.3 가-3: 일방통행 도로는 중심선으로 방향별 링크를 생성하고 차량진행이 불가능한 링크를 통행불가로 표기한다
- 5.3.3 바: 영구적으로 차량통행 기능이 사라진 링크는 삭제한다
- 답변: 통행가능 도로인데 통행불가로 잘못 구축된 경우가 있어 LINKID 목록을 보내면 정기배포에서 수정한다
그래서 ROAD_USE=1 링크는 반대 방향 링크가 ROAD_USE=0(일방통행의 가는 방향)이어야 한다. 표준노드링크는 방향별 링크를 이격하여
노드를 따로 두므로 반대 방향 링크는 F_NODE·T_NODE가 뒤집힌 링크가 아니라 LINK_ID ±100이면서 노드 번호가 끝자리만 다르게
뒤집힌 링크(예: 2511034701 2510027304→2510027002와 2511034801 2510027001→2510027303)이다
(ROAD_USE=0 링크 1,559,794개 중 ±100 링크가 있는 것 1,343,652개, 노드가 그대로 뒤집힌 링크가 있는 것 758개)

근거(하나 이상이면 목록에 넣는다)
- 반대방향없음: 반대 방향 링크가 없음
- 반대방향도미사용: 반대 방향 링크가 있으나 ROAD_USE=1이라 양방향 모두 통행불가
- 고속도로본선: 고속국도·도시고속도로의 연결로가 아닌 링크(CONNECT=0). 고속도로 본선은 일방통행이 아니다
- 소통정보: 국가교통정보센터 /trafficInfo(전국 1.5도 타일 16개)에 그 LINK_ID의 통행 속도가 있음
- 진출불가: 그 링크가 아니면 나갈 길이 없는 링크가 있음(진출불가_링크.csv의 원인 "이어지는 링크가 모두 ROAD_USE=1")
- 요금소: 고속도로 영업소 요금 지점 링크(maps/유료도로/요금지점_링크.csv)
고속도로본선·소통정보·진출불가·요금소는 실제 통행의 직접 근거라 `구분` "근거 있음"으로, 반대 방향 조건만 맞는 것은 보행자 전용 등
차량통행이 없는 도로일 수도 있어 "구조만"으로 표시한다.
출력은 docs/15-데이터-문제/표준노드링크/ROAD_USE1_통행가능의심.csv(UTF-8 BOM)이다. 키는 .env의 ITS_API_KEY이며
소통정보가 빈 응답이면(성공 코드와 빈 목록) 중단하므로 다시 실행한다

python -X utf8 scripts/report/road_use_check.py
"""
import json
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, env, http_get
from nodelink import WGS84, read_links

OUT = ROOT / "docs" / "15-데이터-문제" / "표준노드링크"
ITS = "https://openapi.its.go.kr:9443"
RANK = {"101": "고속국도", "102": "도시고속도로", "103": "일반국도", "104": "특별·광역시도", "105": "국가지원지방도",
        "106": "지방도", "107": "시군도", "108": "기타"}


def traffic_speeds():
    key = env("ITS_API_KEY")
    speed = {}
    lon = 125.0
    while lon < 131.0:
        lat = 33.0
        while lat < 39.0:
            q = f"type=all&minX={lon:.3f}&maxX={lon + 1.5:.3f}&minY={lat:.3f}&maxY={lat + 1.5:.3f}&getType=json"
            d = json.loads(http_get(f"{ITS}/trafficInfo?apiKey={key}&{q}", timeout=180, secrets=(key,)))
            for x in (d.get("body") or {}).get("items") or []:
                speed[x["linkId"]] = x.get("speed", "")
            lat += 1.5
        lon += 1.5
    return speed


def main():
    links = read_links(["LINK_ID", "F_NODE", "T_NODE", "ROAD_USE", "ROAD_NAME", "ROAD_RANK", "CONNECT", "LANES", "MAX_SPD", "LENGTH",
                        "REMARK"])
    ends = dict(zip(links.LINK_ID, zip(links.F_NODE, links.T_NODE)))
    use = dict(zip(links.LINK_ID, links.ROAD_USE))
    def reverse_of(lid):
        f, t = ends[lid]
        return [x for x in (str(int(lid) + 100), str(int(lid) - 100))
                if x in use and ends[x][0][:-1] == t[:-1] and ends[x][1][:-1] == f[:-1]]

    ru = links[links.ROAD_USE == "1"].copy()
    speed = traffic_speeds()
    print(f"소통정보 링크 {len(speed):,}")
    if not speed:
        sys.exit("소통정보가 빈 응답이라 중단함. 다시 실행한다")
    stuck = pd.read_csv(OUT / "진출불가_링크.csv", dtype=str).fillna("")
    stuck = stuck[stuck.원인 == "이어지는 링크가 모두 ROAD_USE=1"]
    blocks = {}
    for r in stuck.itertuples():
        for x in r.관련.split(","):
            blocks.setdefault(x, []).append(r.LINK_ID)
    toll = pd.read_csv(ROOT / "maps" / "유료도로" / "요금지점_링크.csv", dtype=str, encoding="utf-8-sig")
    toll_of = {lid: ",".join(sorted(set(g.영업소코드 + " " + g.영업소명))) for lid, g in toll.groupby("LINK_ID")}

    mid = ru.set_index("LINK_ID").geometry.interpolate(0.5, normalized=True).to_crs(WGS84)
    rows = []
    for r in ru.itertuples():
        rev = reverse_of(r.LINK_ID)
        why = []
        if not rev:
            why.append("반대방향없음")
        elif all(use[x] == "1" for x in rev):
            why.append("반대방향도미사용")
        if r.ROAD_RANK in ("101", "102") and r.CONNECT == "0":
            why.append("고속도로본선")
        if r.LINK_ID in speed:
            why.append(f"소통정보 속도 {speed[r.LINK_ID]}km/h")
        if r.LINK_ID in blocks:
            why.append("진출불가(" + ",".join(blocks[r.LINK_ID]) + ")")
        if r.LINK_ID in toll_of:
            why.append("요금소(" + toll_of[r.LINK_ID] + ")")
        if not why:
            continue
        p = mid.loc[r.LINK_ID]
        rows.append({"LINK_ID": r.LINK_ID, "도로명": r.ROAD_NAME, "등급": RANK.get(r.ROAD_RANK, r.ROAD_RANK), "차로수": r.LANES,
                     "제한속도": r.MAX_SPD, "길이_m": round(r.LENGTH, 1), "F_NODE": r.F_NODE, "T_NODE": r.T_NODE,
                     "반대방향링크": ",".join(f"{x}(ROAD_USE={use[x]})" for x in rev),
                     "구분": "근거 있음" if any(w.startswith(("고속도로본선", "소통정보", "진출불가", "요금소")) for w in why) else "구조만",
                     "근거": " / ".join(why),
                     "REMARK": r.REMARK, "위도": round(p.y, 7), "경도": round(p.x, 7)})
    out = pd.DataFrame(rows)
    out.to_csv(OUT / "ROAD_USE1_통행가능의심.csv", index=False, encoding="utf-8-sig")
    rev0 = sum(any(use[x] == "0" for x in reverse_of(lid)) for lid in ru.LINK_ID)
    print(f"ROAD_USE=1 {len(ru):,}개 | 반대 방향 ROAD_USE=0 링크가 있음(일방통행 역방향 형태) {rev0:,} | 통행가능 의심 {len(out):,}")
    for k in ["반대방향없음", "반대방향도미사용", "고속도로본선", "소통정보", "진출불가", "요금소"]:
        print(f"  {k} {out.근거.str.contains(k).sum():,}")
    print("  근거 둘 이상", (out.근거.str.count(" / ") >= 1).sum(), "|", out.구분.value_counts().to_dict())


if __name__ == "__main__":
    main()

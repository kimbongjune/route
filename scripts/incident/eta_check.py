"""로컬 nav-api의 최소시간 경로 시간(duration_s)과 링크 열을 저장하거나, 저장한 값과 비교한다.

- 소통·돌발 폴링 없이 띄운 서버에서 save로 저장하고, TRAFFIC_POLL_SECS·INCIDENT_POLL_SECS를 주고 띄워 배율이 적용된 서버에서
  compare로 비교한다
- 경로마다 거리, 시간, 링크 수, 저장한 경로와 같은 링크 열인지를 출력한다
- 저장 파일은 인자로 받는다

python -X utf8 scripts/incident/eta_check.py save <저장 파일> [로컬 주소]
python -X utf8 scripts/incident/eta_check.py compare <저장 파일> [로컬 주소]
"""
import json
import sys
import urllib.request

P = {"서울역": (37.5547, 126.9707), "강남역": (37.4979, 127.0276), "부산역": (35.1151, 129.0422),
     "강릉역": (37.7640, 128.8996), "인천공항": (37.4492, 126.4510), "수원역": (37.2664, 127.0002),
     "일산 백석역": (37.6431, 126.7880), "판교역": (37.3948, 127.1112)}
PAIRS = [("서울역", "강남역"), ("서울역", "부산역"), ("서울역", "강릉역"), ("인천공항", "서울역"),
         ("수원역", "서울역"), ("일산 백석역", "판교역")]


def route(local, a, b):
    body = json.dumps({"from": {"lat": P[a][0], "lon": P[a][1]}, "to": {"lat": P[b][0], "lon": P[b][1]},
                       "profile": "time"}).encode()
    req = urllib.request.Request(f"{local}/route", data=body, headers={"Content-Type": "application/json"})
    d = json.load(urllib.request.urlopen(req, timeout=120))["data"]
    return {"distance_m": d["distance_m"], "duration_s": d["duration_s"], "links": d["links"]}


def main():
    mode, path = sys.argv[1], sys.argv[2]
    local = sys.argv[3] if len(sys.argv) > 3 else "http://127.0.0.1:3000"
    now = {f"{a}→{b}": route(local, a, b) for a, b in PAIRS}
    if mode == "save":
        json.dump(now, open(path, "w", encoding="utf-8"), ensure_ascii=False)
        for k, r in now.items():
            print(f"{k} {r['distance_m'] / 1000:.1f}km {r['duration_s'] / 60:.1f}분 링크 {len(r['links'])}")
        return
    old = json.load(open(path, encoding="utf-8"))
    for k, r in now.items():
        o = old[k]
        print(f"{k} 거리 {o['distance_m'] / 1000:.1f}→{r['distance_m'] / 1000:.1f}km "
              f"시간 {o['duration_s'] / 60:.1f}→{r['duration_s'] / 60:.1f}분 "
              f"링크 {len(o['links'])}→{len(r['links'])} 같은 경로 {o['links'] == r['links']}")


if __name__ == "__main__":
    main()

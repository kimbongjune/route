"""로컬 nav-api의 최소시간 경로에서 주의운전구간 메시지(driving_notice)와 재난(disaster) 경고를 확인한다.

- 국가교통정보센터 주의운전구간 API(/posIncidentInfo)를 전국 범위로 직접 받아 문안 수, 이벤트 수, 콘텐츠유형별 수를 출력한다
- 재난상황정보 API(/disasterInfo)로 최근 30일 재난 수, 유형별 수, 종료 일시가 없거나 지나지 않은 재난 수를 출력한다
- 서울역→부산역, 부산역→서울역, 서울역→강릉역, 서울역→광주송정역, 서울역→대전역 경로의 driving_notice·disaster 경고를 출력한다
- 서버는 시작할 때와 5분마다 주의운전구간을 받으므로 직접 받은 자료와 시점이 다를 수 있다
- 키는 .env의 ITS_API_KEY이다

python -X utf8 scripts/caution/check_notices.py [로컬 주소. 기본 http://127.0.0.1:3000]
"""
import datetime as dt
import json
import sys
import urllib.request
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import env, http_get

LOCAL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:3000"
ITS = "https://openapi.its.go.kr:9443"
P = {"서울역": (37.5547, 126.9707), "부산역": (35.1151, 129.0422), "강릉역": (37.7640, 128.8996),
     "광주송정역": (35.1374, 126.7930), "대전역": (36.3324, 127.4343)}


def route(a, b):
    body = json.dumps({"from": {"lat": P[a][0], "lon": P[a][1]}, "to": {"lat": P[b][0], "lon": P[b][1]},
                       "profile": "time"}).encode()
    req = urllib.request.Request(f"{LOCAL}/route", data=body, headers={"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(req, timeout=120))["data"]


def main():
    key = env("ITS_API_KEY")
    d = json.loads(http_get(f"{ITS}/posIncidentInfo?apiKey={key}&minX=124&maxX=132&minY=33&maxY=39&getType=json",
                            secrets=(key,)))
    items = d["body"]["items"]
    events = {x["occrrncId"] or f"{x['outbrkType']}|{x['startStdLinkId']}|{x['startX']}|{x['startY']}" for x in items}
    print(f"주의운전구간 문안 {len(items)}건, 이벤트 {len(events)}건, 시작 링크 없는 문안 "
          f"{sum(1 for x in items if not x['startStdLinkId'])}건")
    print("  콘텐츠유형별 문안:", Counter(x["outbrkType"] for x in items).most_common())
    today = dt.datetime.now(dt.timezone(dt.timedelta(hours=9)))
    q = f"category=D&eventType=all&startDate={(today - dt.timedelta(days=30)):%Y%m%d}&endDate={today:%Y%m%d}"
    d = json.loads(http_get(f"{ITS}/disasterInfo?apiKey={key}&{q}&getType=json", secrets=(key,)))
    items = d["body"]["items"] or []
    now = f"{today:%Y%m%d%H%M}"
    print(f"재난 최근 30일 {len(items)}건, 유형별", Counter(x["eventType"] for x in items).most_common(),
          f"끝나지 않은 재난 {sum(1 for x in items if not x['endDate'] or x['endDate'][:12] >= now)}건")
    for a, b in [("서울역", "부산역"), ("부산역", "서울역"), ("서울역", "강릉역"), ("서울역", "광주송정역"), ("서울역", "대전역")]:
        r = route(a, b)
        w = [x for x in r["warnings"] if x["kind"] in ("driving_notice", "disaster")]
        print(f"{a}→{b} {r['distance_m'] / 1000:.1f}km 주의운전·재난 경고 {len(w)}건")
        for x in w:
            print(f"  {x['at_m'] / 1000:7.1f}km {x['kind']} {x['text']}")


if __name__ == "__main__":
    main()

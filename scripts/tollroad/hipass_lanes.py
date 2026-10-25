"""한국도로공사 영업소 하이패스 차로 개폐 현황(data.ex.co.kr trafficOprgPrcd/tolofHipasCrgwOpclPrssList)을 받아 집계한다.

- 키는 .env의 EX_ROAD_API_KEY이다
- 전체 건수, 영업소별 차로 수, 차로유형·차로구분·LCS 상태별 수, 근무일자 범위를 출력한다

python -X utf8 scripts/tollroad/hipass_lanes.py
"""
import json
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import env, http_get


def main():
    key = env("EX_ROAD_API_KEY")
    url = f"https://data.ex.co.kr/openapi/trafficOprgPrcd/tolofHipasCrgwOpclPrssList?key={key}&type=json&numOfRows=99&pageNo=1"
    d = json.loads(http_get(url, secrets=(key,)))
    rows = d.get("lcsTrafficLists", [])
    print(f"전체 {d.get('count')}건, 받은 행 {len(rows)}개")
    print("영업소별 차로:", dict(Counter(f"{r['tolofNm']}({r['tolofCd']})" for r in rows)))
    print("차로유형:", dict(Counter(r["crgwTypeNm"] for r in rows)))
    print("차로구분:", dict(Counter(r["crgwClssNm"] for r in rows)))
    print("LCS 상태:", dict(Counter(r["lcsStatNm"] for r in rows)))
    dates = sorted(r["workDates"] for r in rows)
    print(f"근무일자 {dates[0]} ~ {dates[-1]}")


if __name__ == "__main__":
    main()

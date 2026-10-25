"""로컬 nav-api의 작업 집합과 경로 응답 시간을 잰다(골목 섞기 실험과 골목 파일 방식 비교).

- 작업 집합은 nav-api 프로세스의 WorkingSet(MB)이다
- 구간마다 /route(최소시간)를 두 번 보내 두 번째의 elapsed_ms를, /routes(대안 포함)를 두 번 보내 두 번째의 왕복 시간을 출력한다

python -X utf8 scripts/alley/mix_measure.py
"""
import json, subprocess, sys, time, urllib.request
LOCAL = "http://127.0.0.1:3000"
PAIRS = [
    ("서울역→강남역", (37.5547, 126.9707), (37.4979, 127.0276)),
    ("용산 소월로40길→대사관로6가길", (37.541945, 126.99276), (37.532345, 127.000598)),
    ("수원 팔달구→영통구", (37.2866, 127.0122), (37.2887, 127.0511)),
    ("서울역→부산역", (37.5547, 126.9707), (35.1151, 129.0422)),
]
def post(path, a, b, profile="time"):
    body = json.dumps({"from": {"lat": a[0], "lon": a[1]}, "to": {"lat": b[0], "lon": b[1]}, "profile": profile}).encode()
    req = urllib.request.Request(LOCAL + path, data=body, headers={"Content-Type": "application/json"})
    t = time.time(); d = json.load(urllib.request.urlopen(req, timeout=300)); return d, (time.time() - t) * 1000
ws = subprocess.run(["powershell", "-Command", "[int]((Get-Process nav-api).WorkingSet64/1MB)"], capture_output=True, text=True).stdout.strip()
print("작업 집합 MB", ws)
for name, a, b in PAIRS:
    post("/route", a, b)
    d, _ = post("/route", a, b)
    r = d["data"]
    post("/routes", a, b)
    _, ms = post("/routes", a, b)
    print(f"{name}: /route {r['elapsed_ms']:.1f}ms {r['distance_m']/1000:.2f}km 링크 {len(r['links'])} | /routes(대안 포함) {ms:.0f}ms")

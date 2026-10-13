"""좌표가 없는 과속방지턱 파일의 주소를 브이월드 API로 좌표 변환한다.

대상은 01_원본_파일/서울, 01_원본_파일/대구의 세 파일이다.
  서울특별시 동작구_과속방지턱_20260616.csv는 '위치' 컬럼이 도로명주소 일부라서 앞에 '서울특별시 동작구 '를 붙인다.
  서울특별시_마포구_과속방지턱 현황_20260409.csv는 '위        치' 컬럼이 전체 도로명주소이다.
  대구광역시 수성구_과속방지턱 현황_20260317.csv는 '지번(위치)' 컬럼이 지번이라서 앞에 '대구광역시 수성구 '를 붙인다.
결과는 03_지오코딩/지오코딩_*.csv이며 원본 컬럼 뒤에 변환주소, 변환위도, 변환경도, 변환결과를 추가한다.
사용 API: https://api.vworld.kr/req/address 의 service=address, request=getcoord, version=2.0, crs=epsg:4326
  type=road 또는 parcel로 조회하고 NOT_FOUND이면 반대 유형으로 한 번 더 조회한다.
사용 키: .env의 VWORLD_API_KEY
2026-10-13 실행 결과 4,211건 중 3,859건(91.6%)의 좌표를 얻었으며 실패 352건은 네트워크 재시도 실패 83건, 주소 미발견 269건이다.

실행: python scripts/speedbump/geocode_vworld.py
테스트: python scripts/speedbump/geocode_vworld.py --limit 20 --out-dir <임시 경로>
"""
import argparse
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import SPEEDBUMP_DIR, env, read_csv, write_csv  # noqa: E402

JOBS = [
    # 이름, 입력 파일, 결과 파일, 주소를 만드는 함수, 먼저 조회할 유형, 다음에 조회할 유형
    ("동작구", "서울/서울특별시 동작구_과속방지턱_20260616.csv", "지오코딩_서울특별시 동작구_과속방지턱.csv",
     lambda r: ("서울특별시 동작구 " + r["위치"]).strip() if r["위치"].strip() else "", "road", "parcel"),
    ("마포구", "서울/서울특별시_마포구_과속방지턱 현황_20260409.csv", "지오코딩_서울특별시 마포구_과속방지턱.csv",
     lambda r: r["위        치"].strip(), "road", "parcel"),
    ("수성구", "대구/대구광역시 수성구_과속방지턱 현황_20260317.csv", "지오코딩_대구광역시 수성구_과속방지턱.csv",
     lambda r: ("대구광역시 수성구 " + r["지번(위치)"]).strip() if r["지번(위치)"].strip() else "", "parcel", "road"),
]


def geocode(key, addr, typ):
    """좌표를 얻으면 (위도, 경도, 유형), 얻지 못하면 (None, None, 상태)를 반환한다."""
    q = urllib.parse.urlencode({"service": "address", "request": "getcoord", "version": "2.0",
                                "crs": "epsg:4326", "address": addr, "refine": "true",
                                "simple": "false", "format": "json", "type": typ, "key": key})
    for attempt in range(4):
        try:
            req = urllib.request.Request("https://api.vworld.kr/req/address?" + q,
                                         headers={"User-Agent": "Mozilla/5.0"})
            d = json.loads(urllib.request.urlopen(req, timeout=30).read().decode())["response"]
            if d.get("status") == "OK":
                p = d["result"]["point"]
                return float(p["y"]), float(p["x"]), typ
            code = d.get("error", {}).get("code", "")
            if code in ("INVALID_KEY", "INCORRECT_KEY", "UNAVAILABLE_KEY", "OVER_REQUEST_LIMIT"):
                # 키 오류는 주소 문제가 아니므로 실패로 집계하지 않고 즉시 중단한다
                raise RuntimeError(f"브이월드 키 오류: {code} - {d['error'].get('text', '')}")
            return None, None, d.get("status", "ERR")
        except RuntimeError:
            raise
        except Exception:
            time.sleep(1 + attempt)
    return None, None, "RETRY_FAIL"


def run_job(key, job, in_dir, out_dir, limit):
    name, infile, outfile, addr_fn, first, second = job
    df = read_csv(in_dir / infile)
    if limit:
        df = df.head(limit)
    df["변환주소"] = df.apply(addr_fn, axis=1)
    addrs = sorted(set(df.변환주소[df.변환주소 != ""]))
    print(name, "고유 주소", len(addrs), flush=True)

    def work(a):
        la, lo, st = geocode(key, a, first)
        if la is None and st == "NOT_FOUND":
            la, lo, st = geocode(key, a, second)
        return a, (la, lo, st)

    with ThreadPoolExecutor(max_workers=4) as ex:  # 4개를 넘기면 호출이 몰려 실패가 늘 수 있다
        res = dict(ex.map(work, addrs))
    out = df.변환주소.map(lambda a: res.get(a, (None, None, "주소없음")))
    df["변환위도"] = [("" if la is None else la) for la, _, _ in out]
    df["변환경도"] = [("" if lo is None else lo) for _, lo, _ in out]
    df["변환결과"] = [st for _, _, st in out]
    write_csv(out_dir / outfile, df)
    ok = sum(1 for v in res.values() if v[0] is not None)
    print(name, f"완료: 고유 주소 성공 {ok}/{len(res)}", flush=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--in-dir", default=str(SPEEDBUMP_DIR / "나중" / "01_원본_파일"))
    ap.add_argument("--out-dir", default=str(SPEEDBUMP_DIR / "나중" / "03_지오코딩"))
    ap.add_argument("--limit", type=int, default=0, help="테스트용. 파일마다 앞 N행만 처리")
    a = ap.parse_args()
    key = env("VWORLD_API_KEY")
    try:
        for job in JOBS:
            run_job(key, job, Path(a.in_dir), Path(a.out_dir), a.limit)
    except RuntimeError as e:
        raise SystemExit(str(e))


if __name__ == "__main__":
    main()

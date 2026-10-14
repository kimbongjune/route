"""행안부 과속방지턱 CSV에서 좌표를 사용할 수 없는 행의 주소를 브이월드로 좌표 변환한다.

대상은 maps/과속방지턱정보/행안부/과속방지턱정보.csv에서 아래 두 경우의 행이다.
  1) 좌표 오류: 위도 33~39, 경도 124~132 범위 밖이거나 숫자가 아니거나 0인 행
  2) 대표 좌표: 같은 좌표에 서로 다른 도로명이 4개 이상 등록된 좌표의 행. 구청이나 시군구 대표 좌표로 입력한 것이다.
소재지도로명주소가 있으면 도로명 type=road로, 없으면 소재지지번주소 type=parcel로 조회한다.
NOT_FOUND이면 반대 유형으로, 그래도 실패하면 괄호와 뒤의 설명을 제거한 주소로, 그다음 normalize로 정규화한 주소로 다시 조회한다.
이전 실행에서 찾지 못한 주소를 다시 조회하려면 --retry-not-found를 지정한다.
결과는 maps/과속방지턱정보/행안부/지오코딩_행안부_좌표오류_대표좌표.csv이며 대상 행마다 한 줄이다.
사용 키는 .env의 VWORLD_API_KEY이며 키 오류가 나면 즉시 중단한다.

실행: python scripts/speedbump/geocode_mois_rows.py
  중단 후 다시 실행하면 결과 파일 옆의 `.진행.json`에 저장된 진행분부터 이어서 진행하며
  네트워크 재시도 실패 RETRY_FAIL로 끝난 주소는 다시 조회한다.
테스트: python scripts/speedbump/geocode_mois_rows.py --limit 30 --out <임시 파일>
"""
import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import SPEEDBUMP_DIR, env, read_csv, write_csv  # noqa: E402

LAT, LON = "WGS84위도", "WGS84경도"


def f(x):
    try:
        return float(x)
    except (TypeError, ValueError):
        return None


def valid(la, lo):
    return la is not None and lo is not None and 33 <= la <= 39 and 124 <= lo <= 132


def clean(addr):
    """괄호와 번지 뒤의 설명을 제거한다."""
    a = re.sub(r"\([^)]*\)", " ", addr)
    m = re.match(r"^(.*?\s(?:산\s?)?\d+(?:-\d+)?)(?=\s|$)", a.strip())
    if m:  # 지번: '... 범어동 2253 빌리브범어A 105동 앞 도로' -> '... 범어동 2253'
        a = m.group(1)
    return re.sub(r"\s+", " ", a).strip()


GWANGJU_GU = {"동구", "서구", "남구", "북구", "광산구"}


def normalize(addr):
    """형식이 잘못된 주소를 정규화한 후보들을 반환한다. 원래 주소와 같은 것은 제외.
    - 시도 새 이름(전남광주통합특별시)을 브이월드가 인식하는 이전 이름(광주광역시/전라남도)으로
    - 괄호 설명 제거
    - 도로명과 건물번호 사이 분리: '장원로29길36' -> '장원로29길 36', 번호 뒤 건물명·위치 설명은 제거
    - 동·리·가와 지번 사이 분리: '미아동187-23주변' -> '미아동 187-23'
    번호가 없는 주소는 위치를 특정할 수 없어 후보를 만들지 않는다."""
    a = re.sub(r"\([^)]*\)", " ", addr)
    a = re.sub(r"\s+", " ", a).strip()
    toks = a.split(" ")
    if toks and toks[0] == "전남광주통합특별시":
        toks[0] = "광주광역시" if len(toks) > 1 and toks[1] in GWANGJU_GU else "전라남도"
        a = " ".join(toks)
    out = []
    # 도로명 + 건물번호 (번호 뒤가 '길/로/번길/가길'이면 도로명의 일부이므로 건너뜀)
    for m in re.finditer(r"(로|길)\s?(\d+(?:-\d+)?)(?!\d)(?!\s?(?:번길|번가길|가길|길|로|번지))", a):
        out.append(a[:m.start(1)] + m.group(1) + " " + m.group(2))
    # 동·리·가 + 지번
    for m in re.finditer(r"(동|리|가)\s?((?:산\s?)?\d+(?:-\d+)?)(?!\d)(?!\s?(?:가|길|로|번길|동))", a):
        out.append(a[:m.start(1)] + m.group(1) + " " + m.group(2))
    seen, res = set(), []
    for c in out:
        c = re.sub(r"\s+", " ", c).strip()
        if c and c != addr and c not in seen:
            seen.add(c)
            res.append(c)
    return res


def geocode(key, addr, typ):
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
                raise RuntimeError(f"브이월드 키 오류: {code} - {d['error'].get('text', '')}")
            return None, None, d.get("status", "ERR")
        except RuntimeError:
            raise
        except Exception:
            time.sleep(1 + attempt)
    return None, None, "RETRY_FAIL"


def resolve(key, road, parcel):
    """(주소, 위도, 경도, 결과). 도로명 우선, 실패하면 지번, 그래도 실패하면 정규화한 주소."""
    cands = []
    if road:
        cands += [(road, "road"), (road, "parcel")]
    if parcel:
        cands += [(parcel, "parcel"), (parcel, "road")]
    for a in (road, parcel):
        if a and clean(a) != a:
            cands += [(clean(a), "road" if a is road else "parcel"), (clean(a), "parcel" if a is road else "road")]
    for a in (road, parcel):
        for n in normalize(a) if a else []:
            cands += [(n, "road"), (n, "parcel")]
    seen, last = set(), "NO_ADDRESS"
    for a, t in cands:
        if (a, t) in seen:
            continue
        seen.add((a, t))
        la, lo, st = geocode(key, a, t)
        if la is not None:
            return a, la, lo, "OK_" + t + ("" if a in (road, parcel) else "_정리")
        last = st
        if st == "RETRY_FAIL":
            break
    return (road or parcel or ""), None, None, last


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default=str(SPEEDBUMP_DIR / "행안부" / "과속방지턱정보.csv"))
    ap.add_argument("--out", default=str(SPEEDBUMP_DIR / "행안부" / "지오코딩_행안부_좌표오류_대표좌표.csv"))
    ap.add_argument("--limit", type=int, default=0, help="테스트용. 대상 행 중 앞 N개 주소만")
    ap.add_argument("--workers", type=int, default=4, help="4개를 넘기면 호출이 몰려 실패가 늘 수 있다")
    ap.add_argument("--retry-not-found", action="store_true", help="이전에 주소를 못 찾은 것도 다듬은 주소로 다시 시도")
    a = ap.parse_args()
    key = env("VWORLD_API_KEY")

    df = read_csv(a.src)
    la, lo = df[LAT].map(f), df[LON].map(f)
    df["유효"] = [valid(x, y) for x, y in zip(la, lo)]
    xy = df.groupby([LAT, LON]).agg(도로명수=("도로명", "nunique"), 유효=("유효", "first"))
    rep = xy[(xy.도로명수 >= 4) & xy.유효].index
    df["사유"] = ""
    df.loc[pd.MultiIndex.from_frame(df[[LAT, LON]]).isin(rep), "사유"] = "대표좌표"
    df.loc[~df.유효, "사유"] = "좌표오류"
    targets = df[df.사유 != ""].copy()
    targets["주소쌍"] = list(zip(targets.소재지도로명주소.str.strip(), targets.소재지지번주소.str.strip()))
    pairs = sorted(set(targets.주소쌍))
    if a.limit:
        pairs = pairs[:a.limit]
    print(f"대상 행 {len(targets):,} (좌표오류 {(targets.사유 == '좌표오류').sum():,}, "
          f"대표좌표 {(targets.사유 == '대표좌표').sum():,}) | 고유 주소 {len(pairs):,}", flush=True)

    cache_path = Path(a.out).with_suffix(".진행.json")
    res = {}
    if cache_path.exists():  # 중단된 실행을 이어서 진행한다
        for k, v in json.loads(cache_path.read_text(encoding="utf-8")).items():
            road, parcel = k.split("	")
            res[(road, parcel)] = tuple(v)
        print(f"이전 진행분 {len(res):,}개를 불러옴", flush=True)
    retry = {"RETRY_FAIL"} | ({"NOT_FOUND", "NO_ADDRESS", "ERR"} if a.retry_not_found else set())
    pairs_todo = [p for p in pairs if p not in res or res[p][3] in retry]  # 실패분 재시도
    done = 0

    def save_cache():
        tmp = cache_path.with_suffix(".tmp")
        tmp.write_text(json.dumps({"	".join(k): list(v) for k, v in res.items()}, ensure_ascii=False),
                       encoding="utf-8")
        os.replace(tmp, cache_path)

    def work(p):
        return p, resolve(key, *p)

    try:
        with ThreadPoolExecutor(max_workers=a.workers) as ex:
            for p, out in ex.map(work, pairs_todo):
                res[p] = out
                done += 1
                if done % 200 == 0:
                    save_cache()
                if done % 500 == 0:
                    ok = sum(1 for v in res.values() if v[1] is not None)
                    print(f"{len(res):,} / {len(pairs):,} (성공 {ok:,})", flush=True)
    except RuntimeError as e:
        save_cache()
        raise SystemExit(str(e))
    save_cache()

    targets = targets[targets.주소쌍.map(lambda p: p in res)]  # --limit으로 제외된 주소는 저장하지 않는다
    v = [res[p] for p in targets.주소쌍]
    out = pd.DataFrame({"관리번호": targets.관리번호, "시도명": targets.시도명, "시군구명": targets.시군구명, "사유": targets.사유,
                        "원래위도": targets[LAT], "원래경도": targets[LON], "사용주소": [x[0] for x in v],
                        "변환위도": [("" if x[1] is None else x[1]) for x in v], "변환경도": [("" if x[2] is None else x[2]) for x in v],
                        "변환결과": [x[3] for x in v]}, dtype=object)
    write_csv(a.out, out)
    ok = (out.변환위도 != "").sum()
    print(f"저장: {a.out} | 행 {len(out):,} 중 좌표 얻음 {ok:,} ({ok / max(len(out), 1):.0%})")
    print("결과 상태:", out.변환결과.value_counts().to_dict())


if __name__ == "__main__":
    main()

"""국토지리정보원 국가관심지점정보 시도별 xlsx에서 요금소를 추출한다.

입력: maps/유료도로/poi/*/*.xlsx  좌표는 EPSG:5179 UTM-K이며 열은 A 아이디, B 명칭, C 분류코드, E·F 주소, I·J x·y이다.
출력: maps/유료도로/API_수집/국가관심지점_요금소.csv  컬럼은 id, name, addr, lat, lon, file이다.
분류코드는 관심지점분류설명.xlsx에서 요금소, 톨게이트, 하이패스가 들어간 코드를 사용하며 요금소 403002120000 하나이다.
시도당 수십만 행이라 xlsx는 calamine 엔진으로 읽는다.

python -X utf8 scripts/tollroad/extract_ngii_poi.py
"""
import re
import sys
import time
from pathlib import Path

import geopandas as gpd
import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import TOLLROAD_DIR, write_csv

POI_DIR = TOLLROAD_DIR / "poi"
OUT = TOLLROAD_DIR / "API_수집" / "국가관심지점_요금소.csv"
COLS = ["id", "name", "code", "road_addr", "lot_addr", "x", "y"]


def toll_codes():
    """관심지점분류설명.xlsx에서 요금소, 톨게이트, 하이패스가 들어간 12자리 분류코드를 찾는다."""
    codes = {}
    for ws in pd.read_excel(POI_DIR / "DAEJEON_POI" / "관심지점분류설명.xlsx", sheet_name=None, header=None, dtype=str).values():
        for row in ws.itertuples(index=False):
            vals = [v for v in row if isinstance(v, str)]
            cs = [v for v in vals if re.fullmatch(r"\d{12}", v)]
            if cs and any(re.search("요금소|톨게이트|하이패스", v) for v in vals):
                codes[cs[0]] = " > ".join(v for v in vals if not re.fullmatch(r"\d{12}", v))[:80]
    return codes


def main():
    codes = toll_codes()
    print("분류코드", codes, flush=True)
    found = []
    for f in sorted(POI_DIR.glob("*/*.xlsx")):
        if "분류설명" in f.name:
            continue
        t0 = time.time()
        df = pd.read_excel(f, engine="calamine", usecols="A:C,E:F,I:J", dtype=str)
        df.columns = COLS
        hit = df[df.code.isin(codes)].copy()
        hit["x"], hit["y"] = pd.to_numeric(hit.x, errors="coerce"), pd.to_numeric(hit.y, errors="coerce")
        hit = hit.dropna(subset=["x", "y"])
        hit["file"] = f.name
        found.append(hit)
        print(f.name, len(df), "행", round(time.time() - t0), "초, 누적 요금소", sum(map(len, found)), flush=True)
    df = pd.concat(found, ignore_index=True)
    p = gpd.GeoSeries(gpd.points_from_xy(df.x, df.y), crs="EPSG:5179").to_crs("EPSG:4326")
    df["lat"], df["lon"] = p.y.round(6), p.x.round(6)
    df["addr"] = df.lot_addr.fillna(df.road_addr)
    write_csv(OUT, df, ["id", "name", "addr", "lat", "lon", "file"])
    print("저장:", OUT, len(df), df.code.value_counts().head(6).to_dict())


if __name__ == "__main__":
    main()

"""국토지리정보원 국가관심지점정보 시도별 shp 세트에서 요금소를 추출한다.

입력: maps/poi/DownloadMap(20261024)/<시도>_POI/  TN_POI shp(EPSG:5179 UTM-K, CP949)와 TN_ADRES dbf(도로명·지번 주소)
출력: maps/유료도로/API_수집/국가관심지점_요금소.csv  컬럼은 id, name, addr, lat, lon, file이다.
분류코드는 maps/poi/FOMAT/TN_ASORTCL_v.1.1.xlsx에서 요금소, 톨게이트, 하이패스가 들어간 코드를 사용하며 요금소 403002120000 하나이다.

python -X utf8 scripts/tollroad/extract_ngii_poi.py
"""
import re
import sys
import time
from pathlib import Path

import geopandas as gpd
import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, TOLLROAD_DIR, write_csv

POI_DIR = ROOT / "maps" / "poi" / "DownloadMap(20261024)"
CLASS_XLSX = ROOT / "maps" / "poi" / "FOMAT" / "TN_ASORTCL_v.1.1.xlsx"
OUT = TOLLROAD_DIR / "API_수집" / "국가관심지점_요금소.csv"


def toll_codes():
    """분류체계 xlsx에서 요금소, 톨게이트, 하이패스가 들어간 12자리 분류코드를 찾는다."""
    codes = {}
    for ws in pd.read_excel(CLASS_XLSX, sheet_name=None, header=None, dtype=str).values():
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
    for folder in sorted(POI_DIR.glob("*_POI")):
        shp = next((folder / "TN_POI").glob("*.shp"))
        t0 = time.time()
        poi = gpd.read_file(shp, encoding="cp949", columns=["nf_id", "poi_nm", "poi_cl_dc"])
        poi = poi.set_crs("EPSG:5179", allow_override=True)
        hit = poi[poi.poi_cl_dc.isin(codes)].copy()
        adres = gpd.read_file(next((folder / "TN_ADRES").glob("*.dbf")), encoding="cp949")
        adres = adres.drop_duplicates("nf_id").set_index("nf_id")
        hit["road_addr"] = hit.nf_id.map(adres.doro_nm)
        hit["lot_addr"] = hit.nf_id.map(adres.jibun_nm)
        hit["file"] = folder.name
        found.append(hit)
        print(folder.name, len(poi), "행", round(time.time() - t0), "초, 누적 요금소", sum(map(len, found)), flush=True)
    df = pd.concat(found, ignore_index=True)
    p = df.geometry.to_crs("EPSG:4326")
    df["lat"], df["lon"] = p.y.round(6), p.x.round(6)
    df["id"], df["name"] = df.nf_id, df.poi_nm
    df["addr"] = df.lot_addr.where(df.lot_addr.fillna("") != "", df.road_addr)
    write_csv(OUT, pd.DataFrame(df.drop(columns="geometry")), ["id", "name", "addr", "lat", "lon", "file"])
    print("저장:", OUT, len(df), df.poi_cl_dc.value_counts().head(6).to_dict())


if __name__ == "__main__":
    main()

"""UTIC 교통안전정보(tsdmsOpenData)를 받아 TAAS 사고다발 폴리곤과 도로환경 위험요소 파일을 만든다.

- 호출: http://www.utic.go.kr/guide/tsdmsOpenData.do?key=... (키는 .env의 UTIC_API_KEY, 등록 IP 대역에서만 동작). 응답은 엑셀 7개가 든 zip이다
- 코드 뜻은 UTIC 코드정의서 tsdms_Code.xlsx(https://www.utic.go.kr/upload/tsdms_Code.xlsx)의 M_CD·S_CD 시트이다
- TAAS.xlsx(M_CD SF100~SF104 사고다발지역 폴리곤)는 기존 사고다발 폴리곤과 같은 컬럼으로 maps/주의운전/TAAS_폴리곤.csv에 쓴다.
  유형은 기존 사고다발 유형 이름에 맞춘다(이륜차, 보행자, 보행노인, 화물차, 음주운전). 사고 건수 컬럼이 없어 0으로 둔다
- 위험요소_도로환경별.xlsx(M_CD SF203)는 기타(SF20314)를 빼고 maps/주의운전/위험요소.csv에 쓴다.
  점은 반경 30m 원, 선은 폭 30m(양쪽 15m) 폴리곤으로 바꾸어 기존 사고다발 폴리곤과 같은 컬럼(유형=위험요소 이름)으로 기록한다
- 법규위반·보행자·차종별 위험요소는 기준일이 대부분 2014년이고, 터널·교량·지하차도는 위험 지점이 아니라 시설 목록이라 쓰지 않는다
- 종류별 행 수를 출력한다

python -X utf8 scripts/caution/build_tsdms.py
"""
import io
import re
import sys
import zipfile
from pathlib import Path

import geopandas as gpd
import pandas as pd
from shapely import wkt
from shapely.geometry import LineString, Point, Polygon

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, env, http_get, write_csv

OUT_DIR = ROOT / "maps" / "주의운전"
COLS = ["유형", "id", "이름", "사고건수", "사상자수", "사망자수", "경도", "위도", "폴리곤"]
TAAS_KIND = {"SF100": "이륜차", "SF101": "보행자", "SF102": "보행노인", "SF103": "화물차", "SF104": "음주운전"}
HAZARD_KIND = {
    "SF20300": "합류도로(좌)", "SF20301": "합류도로(우)", "SF20302": "급커브(굽은도로)", "SF20303": "철길 건널목",
    "SF20304": "급경사(내리막)", "SF20305": "도로폭좁아짐", "SF20306": "미끄러운도로", "SF20307": "추락", "SF20308": "낙석도로",
    "SF20309": "침수", "SF20310": "기상-비", "SF20311": "기상-눈", "SF20312": "기상-안개", "SF20313": "기상-결빙",
    "SF20315": "적치장소", "SF20316": "자전거 위험도로", "SF20317": "교량, 대교", "SF20318": "터널", "SF20319": "지하차도",
}
POINT_RADIUS_M = 30
LINE_HALF_WIDTH_M = 15


def ring_text(geom):
    """폴리곤(또는 멀티폴리곤의 가장 큰 폴리곤) 바깥 고리를 "경도 위도,..."로"""
    if geom.geom_type == "MultiPolygon":
        geom = max(geom.geoms, key=lambda g: g.area)
    return ",".join(f"{x:.7f} {y:.7f}" for x, y in geom.exterior.coords)


def frame(df, kinds, kind_col):
    df = df[df.LOCATION_DATA.notna() & df[kind_col].isin(kinds)].copy()
    # 선은 표준 WKT가 아닌 LINE((...))이며 괄호 모양이 MULTILINESTRING과 같다. 읽지 못하는 도형은 뺀다
    text = df.LOCATION_DATA.str.strip().str.replace(r"^LINE\s*\(\(", "MULTILINESTRING((", regex=True)

    def load(t):
        try:
            g = wkt.loads(t)
            return g if not g.is_empty else None
        except Exception:
            pass
        # 닫히지 않은 폴리곤(491행)과 점 하나뿐인 선(40행)은 좌표를 직접 읽어 폴리곤을 닫거나 점으로 한다
        nums = [float(v) for v in re.findall(r"-?\d+\.?\d*", t)]
        pts = list(zip(nums[0::2], nums[1::2]))
        if len(pts) >= 3 and t.startswith("POLYGON"):
            return Polygon(pts + [pts[0]])
        if len(pts) >= 2:
            return LineString(pts)
        if len(pts) == 1:
            return Point(pts[0])
        return None

    geom = gpd.GeoSeries(text.map(load), crs=4326, index=df.index)
    bad = int(geom.isna().sum())
    if bad:
        print(f"  읽지 못한 도형 {bad}행 제외")
    df, geom = df[geom.notna()], geom[geom.notna()]
    # 점과 선은 미터 좌표에서 넓혀 폴리곤으로 바꾼다
    tm = geom.to_crs(5186)
    is_point = tm.geom_type == "Point"
    is_line = tm.geom_type.isin(["LineString", "MultiLineString"])
    tm[is_point] = tm[is_point].buffer(POINT_RADIUS_M)
    tm[is_line] = tm[is_line].buffer(LINE_HALF_WIDTH_M)
    poly = tm.to_crs(4326)
    center = poly.representative_point()
    return pd.DataFrame({
        "유형": df[kind_col].map(kinds), "id": df.SAFEDATA_ID, "이름": df.ADDRESS_JIBUN.fillna("").str.strip(),
        "사고건수": 0, "사상자수": 0, "사망자수": 0, "경도": center.x.round(7), "위도": center.y.round(7),
        "폴리곤": poly.map(ring_text),
    }), geom.geom_type.value_counts().to_dict()


def main():
    key = env("UTIC_API_KEY")
    raw = http_get(f"http://www.utic.go.kr/guide/tsdmsOpenData.do?key={key}", timeout=300, secrets=(key,))
    z = zipfile.ZipFile(io.BytesIO(raw))
    files = {i.filename.encode("cp437").decode("cp949").split("/")[-1]: i for i in z.infolist()}
    read = lambda name: pd.read_excel(io.BytesIO(z.read(files[name])), dtype=str)
    taas, taas_types = frame(read("TAAS.xlsx"), TAAS_KIND, "M_CD")
    write_csv(OUT_DIR / "TAAS_폴리곤.csv", taas, COLS)
    print(f"TAAS 사고다발 폴리곤 {len(taas)}행 {taas_types}:", taas.유형.value_counts().to_dict())
    hazard, hazard_types = frame(read("위험요소_도로환경별.xlsx"), HAZARD_KIND, "S_CD")
    write_csv(OUT_DIR / "위험요소.csv", hazard, COLS)
    print(f"도로환경 위험요소 {len(hazard)}행 {hazard_types}:", hazard.유형.value_counts().to_dict())


if __name__ == "__main__":
    main()

"""표준노드링크 shp, dbf를 geopandas로 읽는다. 좌표계는 EPSG:5186이다."""
import sys
from pathlib import Path

import geopandas as gpd
import pandas as pd
from pyproj import Transformer

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT

# 표준노드링크 판. 모든 스크립트가 이 경로를 사용하며 Rust 임포터는 importer-knl/src/paths.rs의 moct_folder이다
NODE_DIR = ROOT / "maps" / "[2026-10-14]NODELINKDATA"
TM = "EPSG:5186"
WGS84 = "EPSG:4326"
_TO_WGS84 = Transformer.from_crs(TM, WGS84, always_xy=True)
_TO_TM = Transformer.from_crs(WGS84, TM, always_xy=True)


def tm_to_wgs84(x, y):
    """EPSG:5186 좌표를 (위도, 경도)로 바꾼다."""
    lon, lat = _TO_WGS84.transform(x, y)
    return lat, lon


def wgs84_to_tm(lat, lon):
    """(위도, 경도)를 EPSG:5186 좌표로 바꾼다."""
    return _TO_TM.transform(lon, lat)


def read_layer(path, columns=None, geometry=True):
    """shp나 dbf를 CP949로 읽고 문자열 컬럼의 빈 값은 ''로 둔다."""
    df = gpd.read_file(path, columns=columns, encoding="cp949", ignore_geometry=not geometry, engine="pyogrio")
    for c in df.columns:
        if df[c].dtype == object and c != "geometry":
            df[c] = df[c].fillna("")
    return df


def read_links(columns=None, geometry=True):
    return read_layer(NODE_DIR / "MOCT_LINK.shp", columns, geometry)


def read_nodes(columns=None, geometry=True):
    return read_layer(NODE_DIR / "MOCT_NODE.shp", columns, geometry)


def read_table(name, columns=None):
    """TURNINFO, MULTILINK처럼 도형이 없는 dbf를 읽는다."""
    return read_layer(NODE_DIR / f"{name}.dbf", columns, geometry=False)


def points(df, lat="위도", lon="경도", crs=TM):
    """위경도 컬럼으로 점 GeoDataFrame을 만들고 crs로 변환한다."""
    g = gpd.points_from_xy(pd.to_numeric(df[lon]), pd.to_numeric(df[lat]), crs=WGS84)
    return gpd.GeoDataFrame(df, geometry=g).to_crs(crs)


def to_wgs84(geom):
    """EPSG:5186 도형을 WGS84로 바꾼다."""
    return gpd.GeoSeries(geom, crs=TM).to_crs(WGS84)

"""표준노드링크에 없는 휴게소 하이패스 진출입로를 국가기본도 차도링크에서 추출한다.

대상은 행담도 732, 덕평휴게소 220, 진영휴게소 758이며 MOCT_LINK에는 이 휴게소들의 요금소를 통과하는 휴게소→일반도로 진출로가 없다.
입력
  maps/유료도로/국가기본도_차도링크/TN_RODWAY_LINK*.shp  국토지리정보원 국가기본도 차도 링크, EPSG:5179, CP949
  maps/[2026-09-14]NODELINKDATA/MOCT_LINK               접점을 찾을 MOCT 고속도로 링크
방법
  1) 요금소 좌표 반경 RADIUS_M 이내의 국가기본도 링크 중 고속도로 본선이 아닌 것을 수집한다.
     MTRWY_SE=MWI002이고 2차로 이상이면 본선으로 보며 1차로 고속도로 링크는 진출입 램프로 본다.
  2) 요금소 최근접 링크부터 노드 ID BNODE_NFID, ENODE_NFID로 연결된 링크만 남겨 휴게소 도로로 한다.
     요금소에서 KEEP_M 이상 떨어진 링크로는 확장하지 않는다.
  3) 휴게소 도로의 노드 중 국가기본도 고속도로 본선 링크와 공유하는 노드를 본선 접점으로 보고
     접점마다 최근접 MOCT 링크와 거리, 링크 위 위치 0~1을 기록한다.
출력: maps/유료도로/추가링크_휴게소.csv
  컬럼은 영업소코드, 영업소명, LINK_ID, F_NODE, T_NODE, 도로명, 도로등급, 일방통행, 차로수, 길이m, 요금소링크, 접점_F, 접점_T, 좌표이다.
  LINK_ID, F_NODE, T_NODE는 국가기본도 원본 ID 앞에 NGII:를 붙인다. 요금소링크는 요금소가 그 링크 위이면 Y이다.
  접점_F, 접점_T는 F_NODE, T_NODE가 본선 접점일 때 MOCT LINK_ID|거리m|위치이고 좌표는 WGS84 위도,경도를 ;로 연결한다.
코드표가 없어 코드 의미는 자료로 추정하였다. ROAD_SE RDC001은 고속국도, RDC002는 일반국도이고 그 밖은 하위 도로이며
MTRWY_SE MWI002는 고속도로이다. 진영 요금소 도로가 일방통행 1차로인데 OSPS_SE가 OWI001이므로 OWI001을 일방통행으로 추정하였다.
MOCT 그래프에 적재할 때는 접점에서 MOCT 링크를 위치 비율로 분할해 노드를 만들어 연결해야 하며 그 작업은 임포터에서 한다.
"""
import sys
from collections import defaultdict
from pathlib import Path

import geopandas as gpd
import pandas as pd
import shapely

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import TOLLROAD_DIR, write_csv
from nodelink import NODE_DIR, TM, WGS84

NGII_DIR = TOLLROAD_DIR / "국가기본도_차도링크"
UTMK = "EPSG:5179"
RADIUS_M = 900.0     # 국가기본도 후보를 수집하는 반경
KEEP_M = 450.0       # 휴게소 도로에 포함하는 링크의 요금소 최대 거리. 멀리 있는 일반도로까지 확장되지 않게 한다
MOCT_M = 1500.0      # 접점을 찾을 MOCT 고속도로 링크의 범위
TARGETS = [("732", "행담도", 36.9443906, 126.8097252), ("220", "덕평휴게소", 37.2413826, 127.3886334),
           ("758", "진영휴게소", 35.2765681, 128.7170201)]
OUT_COLS = ["영업소코드", "영업소명", "LINK_ID", "F_NODE", "T_NODE", "도로명", "도로등급", "일방통행", "차로수", "길이m", "요금소링크", "접점_F", "접점_T", "좌표"]


def near(path, center, radius, crs):
    """center 주변 radius 사각형에 걸치는 도형을 파일 순서대로 읽고 center까지의 거리 d를 붙인다."""
    box = (center.x - radius, center.y - radius, center.x + radius, center.y + radius)
    g = gpd.read_file(path, bbox=box, encoding="cp949", engine="pyogrio", fid_as_index=True).set_crs(crs, allow_override=True)
    for c in g.columns:
        if g[c].dtype == object and c != "geometry":
            g[c] = g[c].fillna("")
    g["d"] = g.distance(center)
    return g


def main():
    centers = gpd.GeoSeries(gpd.points_from_xy([t[3] for t in TARGETS], [t[2] for t in TARGETS]), crs=WGS84)
    utmk, tm = centers.to_crs(UTMK), centers.to_crs(TM)

    # 대상 주변 MOCT 고속도로 링크
    moct = pd.concat([near(NODE_DIR / "MOCT_LINK.shp", c, MOCT_M, TM) for c in tm])
    moct = moct[~moct.index.duplicated()].sort_index()
    moct = moct[moct.ROAD_RANK.isin(["101", "102"])]

    rows = []
    for (code, name, _, _), c in zip(TARGETS, utmk):
        mine = pd.concat([near(NGII_DIR / f"{base}.shp", c, RADIUS_M, UTMK) for base in ("TN_RODWAY_LINK", "TN_RODWAY_LINK(1)")])
        mine = mine[mine.d <= RADIUS_M].reset_index(drop=True)
        # 고속도로 본선은 MWI002이면서 2차로 이상인 링크이다. 코드표가 없어 차로수로 구분하였다
        hw = (mine.MTRWY_SE == "MWI002") & (mine.CARTRK_CO >= 2)
        hw_nodes = set(mine.BNODE_NFID[hw]) | set(mine.ENODE_NFID[hw])
        local = mine[~hw].reset_index(drop=True)
        # 요금소 최근접 링크부터 노드로 연결된 도로를 찾는다. 본선 링크는 local에 없으므로 본선으로는 확장되지 않는다
        adj = defaultdict(list)
        for i, (b, e) in enumerate(zip(local.BNODE_NFID, local.ENODE_NFID)):
            adj[b].append(i)
            adj[e].append(i)
        start = local.d.idxmin()
        seen, stack = {start}, [start]
        while stack:
            i = stack.pop()
            for n in (local.BNODE_NFID[i], local.ENODE_NFID[i]):
                for j in adj[n]:
                    if j not in seen and local.d[j] <= KEEP_M:
                        seen.add(j)
                        stack.append(j)
        comp = local.loc[sorted(seen, key=lambda i: local.d[i])]
        print(code, name, "휴게소 도로", len(comp), "링크, 요금소 최근접", round(comp.d.iloc[0], 1), "m")

        def snap(node_id, end):
            if node_id not in hw_nodes:
                return ""
            p = gpd.GeoSeries([end], crs=UTMK).to_crs(TM).iloc[0]
            dist = moct.distance(p)
            k = dist.idxmin()
            line = moct.geometry[k]
            return f"{moct.LINK_ID[k]}|{round(dist[k])}|{round(line.project(p) / (line.length or 1), 3)}"

        wgs = comp.geometry.to_crs(WGS84)
        for (_, v), g in zip(comp.iterrows(), wgs):
            pts = shapely.get_coordinates(v.geometry)
            rows.append({"영업소코드": code, "영업소명": name, "LINK_ID": "NGII:" + v.NF_ID,
                         "F_NODE": "NGII:" + v.BNODE_NFID, "T_NODE": "NGII:" + v.ENODE_NFID,
                         "도로명": v.ROAD_NM, "도로등급": v.ROAD_SE, "일방통행": v.OSPS_SE, "차로수": v.CARTRK_CO,
                         "길이m": round(v.geometry.length), "요금소링크": "Y" if v.d == comp.d.iloc[0] else "",
                         "접점_F": snap(v.BNODE_NFID, shapely.Point(pts[0])), "접점_T": snap(v.ENODE_NFID, shapely.Point(pts[-1])),
                         "좌표": ";".join("%.6f,%.6f" % (y, x) for x, y in shapely.get_coordinates(g))})
    out = TOLLROAD_DIR / "추가링크_휴게소.csv"
    write_csv(out, pd.DataFrame(rows, dtype=object), OUT_COLS)
    print("저장:", out, len(rows), "링크")


if __name__ == "__main__":
    main()

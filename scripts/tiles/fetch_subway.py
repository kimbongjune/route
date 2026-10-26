"""행정안전부 전국 지하철 노선·역·출구(Esri Korea 공개 피처 서비스 MOIS_KR_Subway)를 GeoJSON으로 받는다.

- 서비스: https://portal.esrikr.com/arcgis/rest/services/Hosted/MOIS_KR_Subway/FeatureServer
  레이어 0 KR_Sub_Exit(출구 점), 1 KR_Sub_Station(역 면), 2 KR_Sub_Line(노선 선). 원본은 행정안전부 주소정보 기타자료(2026-04-30 기준)
- 한 번에 최대 2,000건이므로 resultOffset으로 나누어 받는다. 좌표는 WGS84(outSR=4326)
- 출력: maps/배경지도/지하철/노선.geojson, 역.geojson, 출구.geojson
- 레이어별 건수를 출력한다

python -X utf8 scripts/tiles/fetch_subway.py
"""
import json
import sys
import urllib.parse
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, http_get

SERVICE = "https://portal.esrikr.com/arcgis/rest/services/Hosted/MOIS_KR_Subway/FeatureServer"
OUT = ROOT / "maps" / "배경지도" / "지하철"
LAYERS = {2: "노선", 1: "역", 0: "출구"}
PAGE = 2000


def fetch(layer):
    features, offset = [], 0
    while True:
        q = urllib.parse.urlencode({"where": "1=1", "outFields": "*", "outSR": 4326, "f": "geojson",
                                    "resultOffset": offset, "resultRecordCount": PAGE, "orderByFields": "objectid"})
        d = json.loads(http_get(f"{SERVICE}/{layer}/query?{q}"))
        got = d.get("features", [])
        features += got
        if len(got) < PAGE:
            return features
        offset += PAGE


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for layer, name in LAYERS.items():
        features = fetch(layer)
        path = OUT / f"{name}.geojson"
        path.write_text(json.dumps({"type": "FeatureCollection", "features": features}, ensure_ascii=False), encoding="utf-8")
        print(f"{name} {len(features)}건 -> {path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()

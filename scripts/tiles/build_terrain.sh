#!/usr/bin/env bash
# 국토지리정보원 수치표고모델(DEM) 90m(브이월드 dsId 30206, 한반도90m_GRS80.img)로 MapLibre raster-dem용 Terrarium PNG 타일을 만든다.
# GDAL 컨테이너에서 실행한다.
#   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W)/maps/배경지도:/d" -v "$(pwd -W)/scripts/tiles:/work:ro" \
#     ghcr.io/osgeo/gdal:ubuntu-small-3.12.4 bash /work/build_terrain.sh
# 1. 웹 메르카토르(EPSG:3857)로 변환한다. 픽셀은 줌 11 타일 해상도 76.437m이고 바다(NoData -9999)는 0m로 한다
# 2. Terrarium 방식으로 높이를 RGB에 넣는다. 높이 = R × 256 + G + B / 256 − 32768
# 3. gdal2tiles로 줌 5~11 XYZ 타일을 만든다. 값이 섞이지 않도록 최근접 리샘플링이다
# 출력: /d/terrain/{z}/{x}/{y}.png
set -euo pipefail
SRC=/d/DEM90/한반도90m_GRS80.img
TMP=/d/terrain_work
OUT=/d/terrain
mkdir -p "$TMP"
started=$(date +%s)

gdalwarp -overwrite -t_srs EPSG:3857 -tr 76.437028 76.437028 -r bilinear -srcnodata -9999 -dstnodata -9999 \
  -te 13775000 3860000 14760000 4700000 -co TILED=YES -co COMPRESS=DEFLATE -wo NUM_THREADS=ALL_CPUS \
  "$SRC" "$TMP/dem_3857.tif"

python3 - "$TMP/dem_3857.tif" "$TMP/terrarium.tif" <<'PY'
import sys
import numpy as np
from osgeo import gdal

src = gdal.Open(sys.argv[1])
band = src.GetRasterBand(1)
h = band.ReadAsArray().astype(np.float64)
h[(h == -9999) | ~np.isfinite(h)] = 0.0
v = h + 32768.0
r = np.floor(v / 256.0)
g = np.floor(v) - r * 256.0
b = np.floor((v - np.floor(v)) * 256.0)
drv = gdal.GetDriverByName("GTiff")
out = drv.Create(sys.argv[2], src.RasterXSize, src.RasterYSize, 3, gdal.GDT_Byte,
                 ["TILED=YES", "COMPRESS=DEFLATE", "PHOTOMETRIC=RGB"])
out.SetGeoTransform(src.GetGeoTransform())
out.SetProjection(src.GetProjection())
for i, a in enumerate((r, g, b), start=1):
    out.GetRasterBand(i).WriteArray(a.astype(np.uint8))
out.FlushCache()
print("높이 최소", float(h.min()), "최대", float(h.max()), "크기", src.RasterXSize, src.RasterYSize)
PY

rm -rf "$OUT"
gdal2tiles --xyz -z 5-11 -r near -w none --processes=8 -x "$TMP/terrarium.tif" "$OUT" >/dev/null
rm -rf "$TMP"
echo "타일 $(find "$OUT" -name '*.png' | wc -l)개, $(du -sh "$OUT" | cut -f1), $(( $(date +%s) - started ))초"

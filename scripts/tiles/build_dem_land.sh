#!/usr/bin/env bash
# 수치표고모델(DEM) 90m에서 높이가 0보다 큰 땅 영역을 면으로 만든다. 시도 경계가 없는 북한 지역을 땅색으로 칠하는 데 사용한다.
# GDAL 컨테이너에서 실행한다.
#   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W)/maps/배경지도:/d" -v "$(pwd -W)/scripts/tiles:/work:ro" \
#     ghcr.io/osgeo/gdal:ubuntu-small-3.12.4 bash /work/build_dem_land.sh
# 3857의 250m 격자로 변환하고 높이가 0보다 큰 칸을 1로 하여 다각형으로 만든다. 바다는 NoData가 아니라 0m로 들어 있다. 출력: /d/dem_land/dem_land.shp
set -euo pipefail
SRC=/d/DEM90/한반도90m_GRS80.img
OUT=/d/dem_land
mkdir -p "$OUT"
gdalwarp -overwrite -t_srs EPSG:3857 -tr 250 250 -r near -srcnodata -9999 -dstnodata -9999 \
  -te 13700000 3800000 14800000 4900000 "$SRC" "$OUT/dem_250.tif" >/dev/null
gdal_calc -A "$OUT/dem_250.tif" --outfile="$OUT/land.tif" --calc="(A>0)" --type=Byte --NoDataValue=0 --overwrite --quiet
rm -f "$OUT"/dem_land.*
gdal_polygonize "$OUT/land.tif" -f "ESRI Shapefile" "$OUT/dem_land.shp" dem_land val >/dev/null
rm -f "$OUT/dem_250.tif" "$OUT/land.tif"
ogrinfo -ro -so "$OUT/dem_land.shp" dem_land | grep "Feature Count"

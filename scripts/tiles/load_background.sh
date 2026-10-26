#!/usr/bin/env bash
# 배경지도 자료를 PostGIS의 emap 스키마에 적재한다. GDAL 컨테이너(compose의 loader)에서 실행한다.
#   MSYS_NO_PATHCONV=1 docker compose run --rm --entrypoint bash loader /work/load_background.sh          전부
#   MSYS_NO_PATHCONV=1 docker compose run --rm --entrypoint bash loader /work/load_background.sh link     하나만
# 이름: river lake station forest dem_land rlink subway link
# - river: 브이월드 실폭하천(N3A_E0032111), lake: 호수·저수지(N3A_E0052114). 5179, CP949
# - station: scripts/tiles/build_stations.py가 만든 maps/배경지도/역사.csv(위경도)
# - forest: 산림청 임상도 1:5000(TB_FGDI_FS_IM5000.gdb). 임상 코드와 이름만 넣는다
# - dem_land: scripts/tiles/build_dem_land.sh가 DEM으로 만든 땅 면(3857)
# - rlink: 국토지리정보원 국가기본도 철도링크(5179, cpg CP949)
# - subway: scripts/tiles/fetch_subway.py가 받은 행정안전부 지하철 노선·역·출구 GeoJSON(4326)
# - link: 표준노드링크 2026-10-14 MOCT_LINK(5186, CP949). 도로선을 이어진 망으로 그리기 위해 사용한다
# 모두 타일용 3857로 변환하여 bg_ 테이블을 새로 만들고 공간 색인을 만든다
set -euo pipefail

BG=/maps/배경지도
PG="PG:host=${PGHOST} user=${PGUSER} dbname=${PGDATABASE} password=${PGPASSWORD}"
COMMON=(-overwrite -t_srs EPSG:3857 -lco GEOMETRY_NAME=geom -lco SPATIAL_INDEX=GIST -lco FID=fid --config PG_USE_COPY YES)

load() { # 이름
  local t0
  t0=$(date +%s)
  case "$1" in
    river) ogr2ogr -f PostgreSQL "$PG" "$BG/실폭하천/N3A_E0032111.shp" -nln emap.bg_river "${COMMON[@]}" \
             -oo ENCODING=CP949 -makevalid -nlt PROMOTE_TO_MULTI -select UFID ;;
    lake) ogr2ogr -f PostgreSQL "$PG" "$BG/호수저수지/N3A_E0052114.shp" -nln emap.bg_lake "${COMMON[@]}" \
             -oo ENCODING=CP949 -makevalid -nlt PROMOTE_TO_MULTI -select NAME,SERV,MARA -lco PRECISION=NO ;;
    station) ogr2ogr -f PostgreSQL "$PG" "$BG/역사.csv" -nln emap.bg_station "${COMMON[@]}" \
             -oo X_POSSIBLE_NAMES=lon -oo Y_POSSIBLE_NAMES=lat -oo KEEP_GEOM_COLUMNS=NO -s_srs EPSG:4326 -nlt POINT ;;
    forest) ogr2ogr -f PostgreSQL "$PG" "$BG/임상도/TB_FGDI_FS_IM5000.gdb" TB_FGDI_FS_IM5000 -nln emap.bg_forest "${COMMON[@]}" \
             -makevalid -nlt PROMOTE_TO_MULTI -select FRTP_CD,FRTP_NM ;;
    dem_land) ogr2ogr -f PostgreSQL "$PG" "$BG/dem_land/dem_land.shp" -nln emap.bg_dem_land "${COMMON[@]}" -nlt PROMOTE_TO_MULTI ;;
    rlink) ogr2ogr -f PostgreSQL "$PG" "$BG/국가기본도_철도링크/TN_RLROAD_LINK.shp" -nln emap.bg_rlink "${COMMON[@]}" -nlt PROMOTE_TO_MULTI ;;
    subway) for pair in 노선:subway_line 역:subway_station 출구:subway_exit; do
              ogr2ogr -f PostgreSQL "$PG" "$BG/지하철/${pair%%:*}.geojson" -nln "emap.bg_${pair##*:}" "${COMMON[@]}"
            done ;;
    link) ogr2ogr -f PostgreSQL "$PG" "/maps/[2026-10-14]NODELINKDATA/MOCT_LINK.shp" -nln emap.bg_link "${COMMON[@]}" \
             -oo ENCODING=CP949 -nlt PROMOTE_TO_MULTI -select LINK_ID,LANES,ROAD_RANK,ROAD_TYPE,ROAD_NAME,CONNECT ;;
    *) echo "모르는 이름: $1"; exit 1 ;;
  esac
  echo "$1 $(( $(date +%s) - t0 ))초"
}

if [ "$#" -gt 0 ]; then
  for n in "$@"; do load "$n"; done
else
  for n in river lake station forest dem_land rlink subway link; do load "$n"; done
fi

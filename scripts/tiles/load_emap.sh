#!/usr/bin/env bash
# 도로명주소 전자지도 shp를 PostGIS의 emap 스키마에 적재한다. GDAL 컨테이너(compose의 loader)에서 실행한다.
#   docker compose run --rm loader                 전국 16개 시도
#   docker compose run --rm loader 서울특별시 경기도   시도 폴더명 지정
# 좌표계는 5179(prj 없음)로 지정하고 타일용 3857로 변환하여 넣으며 인코딩은 CP949이다.
# 같은 테이블에 시도를 이어 넣고 마지막에 공간 색인을 만든다.
set -euo pipefail

EMAP=/maps/도로명주소\ 전자지도
PG="PG:host=${PGHOST} user=${PGUSER} dbname=${PGDATABASE} password=${PGPASSWORD}"
# shp 이름과 테이블 이름
LAYERS="TL_SPBD_BULD:buld TL_SPRD_MANAGE:sprd_manage TL_SPRD_RW:sprd_rw TL_SCCO_CTPRVN:scco_ctprvn TL_SCCO_SIG:scco_sig TL_SCCO_EMD:scco_emd TL_SCCO_LI:scco_li"

if [ "$#" -gt 0 ]; then
  SIDOS=("$@")
else
  mapfile -t SIDOS < <(ls "$EMAP" | grep -v '\.zip$')
fi

ogrinfo "$PG" -q -sql "CREATE SCHEMA IF NOT EXISTS emap" >/dev/null
started=$(date +%s)
for sido in "${SIDOS[@]}"; do
  dir=$(find "$EMAP/$sido" -maxdepth 1 -type d -regex '.*/[0-9]+' | head -1)
  if [ -z "$dir" ]; then echo "시도 폴더 없음: $sido"; exit 1; fi
  for pair in $LAYERS; do
    shp=${pair%%:*}; table=${pair##*:}
    if [ ! -f "$dir/$shp.shp" ]; then echo "  $sido $shp 없음"; continue; fi
    t0=$(date +%s)
    ogr2ogr -f PostgreSQL "$PG" "$dir/$shp.shp" \
      -nln "emap.$table" -append -makevalid -nlt PROMOTE_TO_MULTI \
      -s_srs EPSG:5179 -t_srs EPSG:3857 \
      -lco GEOMETRY_NAME=geom -lco FID=gid -lco SPATIAL_INDEX=NONE \
      -gt 100000 --config PG_USE_COPY YES --config SHAPE_ENCODING CP949
    echo "  $sido $shp -> emap.$table $(( $(date +%s) - t0 ))초"
  done
done

for pair in $LAYERS; do
  table=${pair##*:}
  t0=$(date +%s)
  ogrinfo "$PG" -q -sql "CREATE INDEX IF NOT EXISTS ${table}_geom_idx ON emap.${table} USING GIST (geom)" >/dev/null
  ogrinfo "$PG" -q -sql "ANALYZE emap.${table}" >/dev/null
  count=$(ogrinfo "$PG" -q -sql "SELECT count(*) AS n FROM emap.${table}" | grep -oE 'n \(Integer(64)?\) = [0-9]+' | grep -oE '[0-9]+$')
  echo "emap.$table 행 $count, 색인 $(( $(date +%s) - t0 ))초"
done
echo "전체 $(( $(date +%s) - started ))초"

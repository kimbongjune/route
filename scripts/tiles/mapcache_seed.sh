#!/usr/bin/env bash
# MapCache 전국 시딩. 레이어별로 미리 만들 줌 범위를 데이터 범위(PostGIS ST_Extent)만큼 mapserver 컨테이너의 mapcache_seed로 만든다.
# 그 위 줌은 요청이 올 때 만들어 캐시한다. 이미 있는 타일은 건너뛴다.
#   bash scripts/tiles/mapcache_seed.sh            기본 범위 전체
#   bash scripts/tiles/mapcache_seed.sh v_buld 14 15   한 레이어만
set -euo pipefail
cd "$(dirname "$0")/../.."
THREADS=4
# 레이어:줌 시작:줌 끝
DEFAULT="v_sig:6:12 v_land_extra:6:12 v_emd:10:14 v_rw:13:14 v_buld:14:15 v_water_low:6:11 v_water:12:13 v_station:12:13 v_forest_low:11:12 v_link_z7:7:9 v_link_low:10:11 v_link:12:13 v_rail2:8:13 v_subway:9:13"

extent() { # 뷰 이름 → "minx,miny,maxx,maxy"(EPSG:3857)
  docker compose exec -T postgis psql -U route -d route -Atc \
    "select concat_ws(',', floor(st_xmin(e)), floor(st_ymin(e)), ceil(st_xmax(e)), ceil(st_ymax(e))) from (select st_extent(geom) e from emap.$1) t"
}

seed() {
  local t0 e
  t0=$(date +%s)
  e=$(extent "$1")
  MSYS_NO_PATHCONV=1 docker compose exec -T -u www-data mapserver \
    mapcache_seed -c /etc/mapcache/mapcache.xml -t "$1" -g GoogleMapsCompatible -z "$2,$3" -e "$e" -n "$THREADS" -q
  echo "$1 z$2-$3 $(( $(date +%s) - t0 ))초"
}

started=$(date +%s)
if [ "$#" -eq 3 ]; then
  seed "$1" "$2" "$3"
else
  for spec in $DEFAULT; do
    IFS=: read -r t z0 z1 <<< "$spec"
    seed "$t" "$z0" "$z1"
  done
fi
echo "시딩 끝 $(( $(date +%s) - started ))초"
MSYS_NO_PATHCONV=1 docker compose exec -T mapserver sh -c 'du -sh /var/cache/mapcache; for d in /var/cache/mapcache/*/; do echo "$d $(find "$d" -type f | wc -l)개"; done'

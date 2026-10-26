#!/usr/bin/env bash
# GeoWebCache 전국 시딩. 레이어별로 미리 만들 줌 범위를 REST로 요청하고 끝날 때까지 진행 상황을 출력한다.
# 그 위 줌은 요청이 올 때 만들어 캐시한다. 비밀번호는 .env에서 읽는다.
#   bash scripts/tiles/gwc_seed.sh            기본 범위 전체
#   bash scripts/tiles/gwc_seed.sh v_buld 14 15   한 레이어만
set -euo pipefail
cd "$(dirname "$0")/../.."
GS=http://127.0.0.1:8080/geoserver
PW=$(grep '^GEOSERVER_ADMIN_PASSWORD=' .env | cut -d= -f2-)
THREADS=4
# 레이어:줌 시작:줌 끝
DEFAULT="v_sig:6:12 v_emd:10:14 v_rw:13:14 v_buld:14:15 v_water_low:6:11 v_water:12:13 v_station:12:13 v_forest_low:11:12 v_link_z7:7:9 v_link_low:10:11 v_link:12:13 v_rail2:8:13 v_subway:9:13"

seed() {
  curl -s -o /dev/null -w "seed $1 z$2-$3 -> %{http_code}\n" -u "admin:$PW" -X POST -H "Content-Type: application/json" \
    "$GS/gwc/rest/seed/emap:$1.json" \
    -d "{\"seedRequest\":{\"name\":\"emap:$1\",\"gridSetId\":\"EPSG:900913\",\"zoomStart\":$2,\"zoomStop\":$3,\"format\":\"application/vnd.mapbox-vector-tile\",\"type\":\"seed\",\"threadCount\":$THREADS}}"
}

curl -s -o /dev/null -u "admin:$PW" "$GS/rest/about/version.json" || { echo "GeoServer에 연결할 수 없음: $GS"; exit 1; }

if [ "$#" -eq 3 ]; then
  seed "$1" "$2" "$3"
else
  for spec in $DEFAULT; do
    IFS=: read -r t z0 z1 <<< "$spec"
    seed "$t" "$z0" "$z1"
  done
fi

started=$(date +%s)
while true; do
  st=$(curl -s -u "admin:$PW" "$GS/gwc/rest/seed.json")
  if echo "$st" | grep -q '"long-array-array":\[\]'; then break; fi
  # 항목은 [완료 타일, 전체 타일, 남은 초, 작업 번호, 상태(1 실행, 2 완료)]
  echo "$(( $(date +%s) - started ))초 $(echo "$st" | python -X utf8 -I -c "
import sys, json
rows = [r for r in json.load(sys.stdin)['long-array-array'] if r[4] == 1]
print(len(rows), '스레드, 전체 타일', sorted(set(r[1] for r in rows)), '남은 초 최대', max((r[2] for r in rows), default=0))")"
  sleep 30
done
echo "시딩 끝 $(( $(date +%s) - started ))초"
docker compose exec -T geoserver sh -c 'du -sh /opt/geoserver/data_dir/gwc/emap_* | sort -h; for d in /opt/geoserver/data_dir/gwc/emap_*; do echo "$d $(find "$d" -type f | wc -l)개"; done'

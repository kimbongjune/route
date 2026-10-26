#!/usr/bin/env bash
# GeoServer에 emap 워크스페이스, PostGIS 저장소, 뷰 레이어(v_buld, v_road, v_road_major, v_rw, v_sig, v_emd와 배경지도 v_water_low, v_water, v_station),
# 벡터 타일(GeoWebCache) 설정을 REST API로 만든다. EXTRA_LAYERS 환경 변수로 레이어를 더할 수 있다.
# 이미 있는 항목은 POST가 실패하므로(409 등) 다시 실행해도 된다. 비밀번호는 .env에서 읽는다.
#   bash scripts/tiles/geoserver_setup.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
GS=http://127.0.0.1:8080/geoserver
PW=$(grep '^GEOSERVER_ADMIN_PASSWORD=' .env | cut -d= -f2-)
PGPW=$(grep '^POSTGRES_PASSWORD=' .env | cut -d= -f2-)
# 레이어 이름:줌 시작:줌 끝. 건물은 14부터, 실폭도로는 13부터, 도로구간은 10부터, 고속도로·대로만 든 v_road_major는 7~12, 시군구는 6~12, 읍면동은 10~14
# 배경지도(background_views.sql): 물은 낮은 줌용 6~11과 12~18, 역 12~18, 산림은 낮은 줌용 9~12와 13~16
LAYERS="v_buld:14:18 v_road:10:18 v_road_major:7:12 v_rw:13:18 v_sig:6:12 v_emd:10:14 v_water_low:6:11 v_water:12:18 v_station:12:18 v_forest_low:9:12 v_forest:13:16 v_land_extra:6:12 v_rail2:8:18 v_subway:9:18 v_link_z7:7:9 v_link_low:10:11 v_link:12:18 ${EXTRA_LAYERS:-}"

req() { # 메서드 URL 콘텐츠타입 본문
  code=$(curl -s -o /tmp/gs_body -w "%{http_code}" -u "admin:$PW" -X "$1" -H "Content-Type: $3" "$2" -d "$4") \
    || { echo "GeoServer에 연결할 수 없음: $GS"; exit 1; }
  echo "$1 ${2#"$GS"/} -> $code"
}

req POST "$GS/rest/workspaces" application/json '{"workspace":{"name":"emap"}}'

req POST "$GS/rest/workspaces/emap/datastores" application/json "{\"dataStore\":{\"name\":\"postgis\",\"connectionParameters\":{\"entry\":[
  {\"@key\":\"dbtype\",\"\$\":\"postgis\"},{\"@key\":\"host\",\"\$\":\"postgis\"},{\"@key\":\"port\",\"\$\":\"5432\"},
  {\"@key\":\"database\",\"\$\":\"route\"},{\"@key\":\"schema\",\"\$\":\"emap\"},{\"@key\":\"user\",\"\$\":\"route\"},
  {\"@key\":\"passwd\",\"\$\":\"$PGPW\"},{\"@key\":\"Expose primary keys\",\"\$\":\"false\"},{\"@key\":\"max connections\",\"\$\":\"20\"}]}}}"

for spec in $LAYERS; do
  IFS=: read -r t z0 z1 <<< "$spec"
  req POST "$GS/rest/workspaces/emap/datastores/postgis/featuretypes" application/json \
    "{\"featureType\":{\"name\":\"$t\",\"nativeName\":\"$t\",\"srs\":\"EPSG:3857\",\"enabled\":true}}"
done

# GWC 레이어: 벡터 타일 형식, 웹 메르카토르 격자, 레이어별 줌 범위, 메타타일 없음, 타일 경계에서 선이 끊기지 않도록 여백(gutter) 16픽셀
for spec in $LAYERS; do
  IFS=: read -r t z0 z1 <<< "$spec"
  req PUT "$GS/gwc/rest/layers/emap:$t.xml" application/xml \
    "<GeoServerLayer><name>emap:$t</name><enabled>true</enabled>
     <mimeFormats><string>application/vnd.mapbox-vector-tile</string></mimeFormats>
     <gridSubsets><gridSubset><gridSetName>EPSG:900913</gridSetName><zoomStart>$z0</zoomStart><zoomStop>$z1</zoomStop></gridSubset></gridSubsets>
     <metaWidthHeight><int>1</int><int>1</int></metaWidthHeight><expireCache>0</expireCache><expireClients>0</expireClients><gutter>16</gutter>
     </GeoServerLayer>"
done

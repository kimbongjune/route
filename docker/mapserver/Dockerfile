# 벡터 타일 서버. MapServer가 PostGIS 뷰에서 MVT를 만들고 같은 Apache의 MapCache가 디스크에 캐시한다
# MapServer는 /mapserver, MapCache는 /mapcache 주소이다. PostGIS 접속은 PGHOST·PGUSER·PGPASSWORD·PGDATABASE 환경 변수로 한다
FROM camptocamp/mapserver:8.6
RUN apt-get update && apt-get install -y --no-install-recommends libapache2-mod-mapcache mapcache-tools \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /var/cache/mapcache && chown www-data /var/cache/mapcache
ENV MAPSERVER_BASE_PATH=/mapserver
# 이미지 기본 설정은 모든 주소를 MapServer로 보내므로 /mapserver만 보내도록 한 설정으로 덮는다
COPY mapserver.conf /etc/apache2/conf-enabled/mapserver.conf
COPY mapcache.conf /etc/apache2/conf-enabled/mapcache.conf
COPY mapcache.xml /etc/mapcache/mapcache.xml
COPY emap.map /etc/mapserver/emap.map

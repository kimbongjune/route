-- 배경지도 타일용 테이블과 뷰. scripts/tiles/load_background.sh로 bg_ 테이블을 적재한 뒤 실행한다.
--   docker compose exec -T postgis psql -U route -d route -v ON_ERROR_STOP=1 -f - < scripts/tiles/background_views.sql
-- 낮은 줌용은 면적으로 거르고 단순화한 테이블을 미리 만든다(뷰에서 매번 단순화하면 타일 생성이 느리다)

-- 물: 실폭하천과 호수·저수지. 줌 12 이상은 전부, 6~11은 면적 10만㎡ 이상을 20m로 단순화.
-- 큰 강 면은 꼭짓점이 18만 개, 구멍이 수백 개라 브라우저가 면을 삼각형으로 나누는 계산이 무거워 꼭짓점 256개 이하 조각으로 나눈다
DROP TABLE IF EXISTS emap.bg_water_low CASCADE;
CREATE TABLE emap.bg_water_low AS
SELECT kind, name, ST_Multi(ST_Subdivide(ST_CollectionExtract(ST_MakeValid(geom), 3), 256))::geometry(MultiPolygon, 3857) AS geom FROM (
  SELECT 'river'::text AS kind, NULL::text AS name, ST_SimplifyPreserveTopology(geom, 20) AS geom
  FROM emap.bg_river WHERE ST_Area(geom) >= 100000
  UNION ALL
  SELECT 'lake', name, ST_SimplifyPreserveTopology(geom, 20)
  FROM emap.bg_lake WHERE ST_Area(geom) >= 100000
) w;
CREATE INDEX bg_water_low_geom_idx ON emap.bg_water_low USING GIST (geom);
ANALYZE emap.bg_water_low;

DROP TABLE IF EXISTS emap.bg_water CASCADE;
CREATE TABLE emap.bg_water AS
SELECT kind, name, ST_Multi(ST_Subdivide(ST_CollectionExtract(ST_MakeValid(geom), 3), 256))::geometry(MultiPolygon, 3857) AS geom FROM (
  SELECT 'river'::text AS kind, NULL::text AS name, geom FROM emap.bg_river
  UNION ALL
  SELECT 'lake', name, geom FROM emap.bg_lake
) w;
CREATE INDEX bg_water_geom_idx ON emap.bg_water USING GIST (geom);
ANALYZE emap.bg_water;

CREATE OR REPLACE VIEW emap.v_water_low AS SELECT kind, name, geom FROM emap.bg_water_low;
CREATE OR REPLACE VIEW emap.v_water AS SELECT kind, name, geom FROM emap.bg_water;

-- 역: 도시광역철도 역사(같은 이름 300m 안은 한 역). 이름이 "역"으로 끝나지 않으면 "역"을 더한다
CREATE OR REPLACE VIEW emap.v_station AS
SELECT (CASE WHEN name LIKE '%역' THEN name ELSE name || '역' END)::varchar AS name, lines, geom FROM emap.bg_station;

-- 산림: 산림청 임상도 1:5000의 임상 1 침엽수림, 2 활엽수림, 3 혼효림, 4 죽림(0 무립목지/비산림은 뺀다).
-- 줌 13 이상은 전부, 9~12는 면적 5만㎡ 이상을 25m로 단순화
DROP TABLE IF EXISTS emap.bg_forest_low CASCADE;
CREATE TABLE emap.bg_forest_low AS
SELECT frtp_cd AS cls, ST_Multi(ST_SimplifyPreserveTopology(geom, 25))::geometry(MultiPolygon, 3857) AS geom
FROM emap.bg_forest WHERE frtp_cd IN ('1', '2', '3', '4') AND ST_Area(geom) >= 50000;
CREATE INDEX bg_forest_low_geom_idx ON emap.bg_forest_low USING GIST (geom);
ANALYZE emap.bg_forest_low;

CREATE OR REPLACE VIEW emap.v_forest_low AS SELECT cls, geom FROM emap.bg_forest_low;
CREATE OR REPLACE VIEW emap.v_forest AS
SELECT frtp_cd AS cls, geom FROM emap.bg_forest WHERE frtp_cd IN ('1', '2', '3', '4');

-- 시도 경계 밖의 땅(북한 등). DEM 땅 면(bg_dem_land)에서 시도 경계를 빼고, 해안의 좁은 조각을 없애기 위해 300m 안쪽으로
-- 줄였다가 다시 늘린 뒤(열림) 50㎢ 이상인 면만 남긴다
DROP TABLE IF EXISTS emap.bg_land_extra CASCADE;
CREATE TABLE emap.bg_land_extra AS
WITH sido AS (SELECT ST_Union(geom) AS g FROM emap.scco_ctprvn),
diff AS (SELECT ST_Difference(ST_Union(d.geom), (SELECT g FROM sido)) AS g FROM emap.bg_dem_land d),
opened AS (SELECT ST_Buffer(ST_Buffer(g, -300), 300) AS g FROM diff)
SELECT ST_Multi(p.geom)::geometry(MultiPolygon, 3857) AS geom
FROM opened, LATERAL ST_Dump(opened.g) AS p
WHERE ST_Area(p.geom) >= 50000000;
CREATE INDEX bg_land_extra_geom_idx ON emap.bg_land_extra USING GIST (geom);
CREATE OR REPLACE VIEW emap.v_land_extra AS SELECT geom FROM emap.bg_land_extra;

-- 철도(국가기본도 철도링크): 철도 구분 RRC001·RRC002만 넣는다. 코드 정의는 자료에 없어 노선 이름으로 보았으며
-- RRC001에는 경부고속철도·호남고속철도와 경부선 등, RRC002에는 경부선·호남선 등 일반철도 노선이 들어 있다.
-- RRC004(지하철·경전철의 지상 구간)는 지하철 노선(v_subway)으로 그리고, 레일바이크 등(RRC005·006·999)은 뺀다
CREATE OR REPLACE VIEW emap.v_rail2 AS
SELECT rlway_nm AS name, rlroad_se AS cls, geom FROM emap.bg_rlink WHERE rlroad_se IN ('RRC001', 'RRC002');

-- 지하철 노선(행정안전부 주소정보 기타자료, Esri Korea 공개 피처 서비스): 노선 32개
CREATE OR REPLACE VIEW emap.v_subway AS SELECT sub_nm AS name, geom FROM emap.bg_subway_line;

-- 도로(표준노드링크): 줌 12 이상은 링크 전부. 7~11은 도로등급 101~106을 등급·도로명별로 합치고 20m로 단순화
DROP TABLE IF EXISTS emap.bg_link_low CASCADE;
CREATE TABLE emap.bg_link_low AS
SELECT road_rank AS rank, NULLIF(road_name, '') AS name,
       ST_Multi(ST_SimplifyPreserveTopology(ST_LineMerge(ST_CollectionExtract(ST_Collect(geom), 2)), 20))::geometry(MultiLineString, 3857) AS geom
FROM emap.bg_link WHERE road_rank IN ('101', '102', '103', '104', '105', '106')
GROUP BY road_rank, road_name, ST_SnapToGrid(ST_Centroid(geom), 20000);
CREATE INDEX bg_link_low_geom_idx ON emap.bg_link_low USING GIST (geom);
ANALYZE emap.bg_link_low;
CREATE OR REPLACE VIEW emap.v_link_low AS SELECT rank, name, geom FROM emap.bg_link_low;
CREATE OR REPLACE VIEW emap.v_link AS
SELECT road_rank AS rank, NULLIF(road_name, '') AS name, lanes, geom FROM emap.bg_link;

-- 도로 줌 7~9: 도로등급 101~103만 등급·도로명별로 합치고 80m로 단순화(줌 7~9 타일이 200~330KB여서 나누었다)
DROP TABLE IF EXISTS emap.bg_link_z7 CASCADE;
CREATE TABLE emap.bg_link_z7 AS
SELECT road_rank AS rank, NULLIF(road_name, '') AS name,
       ST_Multi(ST_SimplifyPreserveTopology(ST_LineMerge(ST_CollectionExtract(ST_Collect(geom), 2)), 80))::geometry(MultiLineString, 3857) AS geom
FROM emap.bg_link WHERE road_rank IN ('101', '102', '103')
GROUP BY road_rank, road_name, ST_SnapToGrid(ST_Centroid(geom), 50000);
CREATE INDEX bg_link_z7_geom_idx ON emap.bg_link_z7 USING GIST (geom);
ANALYZE emap.bg_link_z7;
CREATE OR REPLACE VIEW emap.v_link_z7 AS SELECT rank, name, geom FROM emap.bg_link_z7;

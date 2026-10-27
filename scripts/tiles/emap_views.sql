-- 도로명주소 전자지도 타일용 뷰. scripts/tiles/load_emap.sh로 전자지도를 적재한 뒤 실행한다.
--   docker compose exec -T postgis psql -U route -d route -v ON_ERROR_STOP=1 -f - < scripts/tiles/emap_views.sql
-- 타일에 필요한 컬럼만 남긴다(13 문서 "자체 벡터 타일"). 배경지도 뷰는 background_views.sql이다

CREATE OR REPLACE VIEW emap.v_buld AS
 SELECT NULLIF(buld_nm::text, ''::text) AS name,
    gro_flo_co::integer AS floors,
    geom
   FROM emap.buld;

CREATE OR REPLACE VIEW emap.v_road AS
 SELECT gid,
    rn AS name,
    roa_cls_se AS cls,
    road_bt::real AS width,
    wdr_rd_cd AS admin,
    geom
   FROM emap.sprd_manage;

CREATE OR REPLACE VIEW emap.v_road_major AS
 SELECT gid,
    rn AS name,
    roa_cls_se AS cls,
    road_bt::real AS width,
    wdr_rd_cd AS admin,
    geom
   FROM emap.sprd_manage
  WHERE roa_cls_se::text = ANY (ARRAY['1'::character varying, '2'::character varying]::text[]);

CREATE OR REPLACE VIEW emap.v_rw AS
 SELECT gid,
    geom
   FROM emap.sprd_rw;

CREATE OR REPLACE VIEW emap.v_sig AS
 SELECT gid,
    sig_cd AS code,
    sig_kor_nm AS name,
    geom
   FROM emap.scco_sig;

CREATE OR REPLACE VIEW emap.v_emd AS
 SELECT gid,
    emd_cd AS code,
    emd_kor_nm AS name,
    geom
   FROM emap.scco_emd;

# route

공공 데이터(표준노드링크, 도로명주소 전자지도, 국가관심지점 등)로 만든 자체 경로 탐색 엔진(routedb)과 내비게이션 서버(nav-api), 웹 화면이다. 설계와 구현 기록은 `docs/`에 있으며 `docs/13-routedb-구현-기록.md`가 전체 기록이다.

## 구성

| 구성 요소 | 위치 | 역할 |
|---|---|---|
| routedb | `rust/routedb` | 그래프 임포터, CCH 경로 탐색, 골목 파일, 지오코더 색인 |
| nav-api | `rust/nav/api` | 경로·검색·경고 API, 실시간 수집, 타일 프록시, 웹 정적 파일(3000번 포트) |
| 웹 화면 | `rust/nav/web` | MapLibre 지도와 경로 화면 |
| PostGIS | `compose.yaml`의 `postgis` | 전자지도·배경지도 데이터(5432번 포트) |
| MapServer | `compose.yaml`의 `mapserver`, 설정은 `docker/mapserver` | 벡터 타일 생성(MapServer)과 캐시(MapCache)(8080번 포트) |

데이터(`maps/`)는 저장소에 포함하지 않는다. 원본 목록과 받는 곳은 `docs/체크리스트.md` 0단계와 각 데이터 문서(06~12, 14)에 있다.

## 준비

- Rust(1.98 이상), Docker(Compose 포함), Python 3.13(`pip install -r scripts/requirements.txt`)
- 저장소 루트의 `.env`

| 키 | 용도 |
|---|---|
| `VWORLD_API_KEY` | 브이월드 배경 타일(필수) |
| `POSTGRES_PASSWORD` | PostGIS 비밀번호(필수) |
| `ITS_API_KEY` | 국가교통정보센터 소통·돌발·주의운전구간·재난 정보 |
| `TRAFFIC_POLL_SECS`, `INCIDENT_POLL_SECS` | 소통정보·돌발정보 수집 간격(초). 없으면 수집하지 않는다 |
| `UTIC_API_KEY` | 경찰청 UTIC 돌발정보·도로위험상황예보(등록 IP 대역에서만 동작) |
| `EX_ROAD_API_KEY`, `DATA_GO_KR_API_KEY`, `KAKAO_REST_API_KEY` | 데이터 수집·확인 스크립트 |

## 1. 경로 데이터 만들기

`maps/routedb`에 그래프 파일, CCH 파일, 골목 파일, 검색 색인을 만든다.

```
cd rust
cargo build --release -p importer-knl -p routedb-geocode
./target/release/importer-knl.exe        # moct.rdb, moct.cch-struct, moct.cch-metric(-time, -free)
./target/release/importer-alley.exe      # alley.rdb(그래프 파일 다음에 실행)
./target/release/importer-geocode.exe    # geocode.idx
```

도커로 만들 때는 `docker compose run --rm importer`, `docker compose run --rm importer importer-alley`, `docker compose run --rm importer importer-geocode`이다.

## 2. 지도 데이터 적재

PostGIS를 띄우고 전자지도와 배경지도를 넣은 뒤 타일용 뷰를 만든다. Git Bash에서는 경로 변환을 막기 위해 `MSYS_NO_PATHCONV=1`을 앞에 둔다.

```
docker compose up -d postgis
docker compose run --rm loader                                   # 전자지도 전국(시도 폴더명을 주면 그 시도만)
docker compose exec -T postgis psql -U route -d route -v ON_ERROR_STOP=1 -f - < scripts/tiles/emap_views.sql
MSYS_NO_PATHCONV=1 docker compose run --rm --entrypoint bash loader /work/load_background.sh
docker compose exec -T postgis psql -U route -d route -v ON_ERROR_STOP=1 -f - < scripts/tiles/background_views.sql
```

배경지도 적재 전에 필요한 파일(`역사.csv`, DEM 땅 면, 지하철 노선)과 산 음영 타일(`maps/배경지도/terrain`)은 `scripts/tiles/`의 `build_stations.py`, `build_dem_land.sh`, `fetch_subway.py`, `build_terrain.sh`로 만든다. 각 파일 첫머리에 실행 방법이 있다.

## 3. 타일 서버와 캐시

```
docker compose up -d mapserver
bash scripts/tiles/mapcache_seed.sh              # 레이어별 기본 줌 범위를 전국으로 미리 만든다
bash scripts/tiles/mapcache_seed.sh v_buld 14 15 # 한 레이어만
```

미리 만들지 않은 줌은 요청이 올 때 MapServer가 만들고 MapCache가 볼륨 `mapcache`에 저장한다. 캐시를 다른 서버로 옮길 때는 이 볼륨을 함께 옮긴다.

## 4. 서버 실행

로컬에서 실행하면 데이터 폴더는 `ROUTE_MAPS_DIR`(없으면 저장소의 `maps`), 주소는 `NAV_ADDR`(없으면 127.0.0.1:3000), 타일 서버는 `MAPSERVER_URL`(없으면 http://127.0.0.1:8080)이다. `.env`는 서버가 직접 읽는다.

```
cd rust
cargo run --release -p nav-api
```

도커로 전체를 띄울 때는 저장소 루트에서 실행한다. `api`는 `./maps`를 읽기 전용으로 마운트하고 `.env`를 환경 변수로 받는다.

```
docker compose up -d          # api, postgis, mapserver
docker compose down
```

브라우저에서 http://localhost:3000 을 열고 "자체 지도"를 켜면 자체 벡터 타일 지도가 나온다.

```
curl -X POST http://127.0.0.1:3000/route -H "Content-Type: application/json" -d '{"from":{"lat":37.5547,"lon":126.9707},"to":{"lat":37.4979,"lon":127.0276},"profile":"time"}'
curl "http://127.0.0.1:3000/geocode?q=서울역&size=3&lat=37.5665&lon=126.978"
curl -o tile.pbf "http://127.0.0.1:3000/vtiles/buld/14/13972/6346"
```

## 5. 테스트

```
cd rust
cargo test --workspace
```

## 서버 사양

요청이 없을 때 nav-api 약 1.15GB, MapServer 약 0.24GB, PostGIS 약 0.24GB를 사용한다(2026-10-27 측정). 디스크는 PostGIS 약 21GB, 경로·검색 파일 3.7GB, 타일 캐시 6.9GB(`mapcache_seed.sh` 기본 범위 전국, 4,888초)가 필요하다.

# 빌드 단계. 워크스페이스에서 nav-api, importer-knl, importer-geocode만 release로 빌드한다
FROM rust:1.98-bookworm AS builder
WORKDIR /app
COPY rust/ rust/
RUN cargo build --release --manifest-path rust/Cargo.toml -p nav-api -p importer-knl -p routedb-geocode

# 내비 API. 그래프 파일은 /maps 볼륨에서 읽고 정적 파일은 이미지에 넣는다
FROM debian:bookworm-slim AS api
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/rust/target/release/nav-api /usr/local/bin/nav-api
COPY rust/nav/web /app/web
ENV ROUTE_MAPS_DIR=/maps \
    NAV_WEB_DIR=/app/web \
    NAV_ADDR=0.0.0.0:3000
EXPOSE 3000
CMD ["nav-api"]

# 임포터. /maps 볼륨의 원본 데이터로 그래프 파일과 CCH, 지오코더 색인을 만들고 종료한다
FROM debian:bookworm-slim AS importer
COPY --from=builder /app/rust/target/release/importer-knl /usr/local/bin/importer-knl
COPY --from=builder /app/rust/target/release/importer-geocode /usr/local/bin/importer-geocode
ENV ROUTE_MAPS_DIR=/maps
CMD ["importer-knl"]

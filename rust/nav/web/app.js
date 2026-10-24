// 지도, 출발·도착 입력, 지오코딩 검색, 경로 요청과 경로선 표시.
// 배경은 nav-api가 프록시하는 브이월드 2D 타일이다

const TILE_URL = `${location.origin}/tiles/base/{z}/{x}/{y}`;
const SEOUL_STATION = [126.9707, 37.5547];

const map = new maplibregl.Map({
  container: "map",
  style: {
    version: 8,
    sources: {
      vworld: {
        type: "raster",
        tiles: [TILE_URL],
        tileSize: 256,
        minzoom: 6,
        maxzoom: 19,
        attribution: "국토교통부 브이월드",
      },
    },
    layers: [{ id: "vworld", type: "raster", source: "vworld" }],
  },
  center: SEOUL_STATION,
  zoom: 14,
  maxZoom: 19,
  minZoom: 6,
  attributionControl: { compact: true },
});
map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), "top-right");

const el = (id) => document.getElementById(id);
const status = el("status");
const results = el("results");
const summary = el("summary");
const instructionsList = el("instructions");
const routeButton = el("route-button");

// 출발·도착 지점. {lat, lon, name}이며 마커와 함께 둔다
const points = { from: null, to: null };
const markers = { from: null, to: null };
// 지도 클릭이 채울 칸. 출발이 비어 있으면 출발, 아니면 도착
let searchTarget = null;
let route = null;

function setStatus(text) {
  status.textContent = text;
}

function setPoint(kind, point) {
  points[kind] = point;
  el(`${kind}-input`).value = point.name;
  if (markers[kind]) markers[kind].remove();
  const dot = document.createElement("div");
  dot.className = `marker ${kind}`;
  markers[kind] = new maplibregl.Marker({ element: dot, draggable: true })
    .setLngLat([point.lon, point.lat])
    .addTo(map);
  markers[kind].on("dragend", () => {
    const { lng, lat } = markers[kind].getLngLat();
    points[kind] = { lat, lon: lng, name: `${lat.toFixed(5)}, ${lng.toFixed(5)}` };
    el(`${kind}-input`).value = points[kind].name;
    requestRoute();
  });
  routeButton.disabled = !(points.from && points.to);
}

// 지도 클릭: 출발이 없으면 출발, 있으면 도착
map.on("click", (event) => {
  const { lng, lat } = event.lngLat;
  const kind = points.from ? "to" : "from";
  setPoint(kind, { lat, lon: lng, name: `${lat.toFixed(5)}, ${lng.toFixed(5)}` });
  hideResults();
  if (points.from && points.to) requestRoute();
});

// 지오코딩 검색. 입력이 멈추고 300ms 뒤에 /geocode를 부른다
let searchTimer = null;
for (const kind of ["from", "to"]) {
  const input = el(`${kind}-input`);
  input.addEventListener("input", () => {
    searchTarget = kind;
    clearTimeout(searchTimer);
    const q = input.value.trim();
    if (q.length < 2) {
      hideResults();
      return;
    }
    searchTimer = setTimeout(() => search(q), 300);
  });
  input.addEventListener("focus", () => {
    searchTarget = kind;
  });
}

async function search(q) {
  const response = await fetch(`/geocode?q=${encodeURIComponent(q)}&size=8`);
  const body = await response.json();
  if (body.code !== "OK") {
    setStatus(`검색 실패: ${body.message}`);
    return;
  }
  showResults(body.data.items);
}

function showResults(items) {
  results.innerHTML = "";
  if (items.length === 0) {
    hideResults();
    setStatus("검색 결과 없음");
    return;
  }
  for (const item of items) {
    const li = document.createElement("li");
    li.innerHTML = `<span class="title"></span><span class="addr"></span>`;
    li.querySelector(".title").textContent = item.title;
    li.querySelector(".addr").textContent = item.address;
    li.addEventListener("click", () => {
      setPoint(searchTarget, { lat: item.lat, lon: item.lon, name: item.title });
      hideResults();
      map.flyTo({ center: [item.lon, item.lat], zoom: Math.max(map.getZoom(), 14) });
      if (points.from && points.to) requestRoute();
    });
    results.appendChild(li);
  }
  results.hidden = false;
}

function hideResults() {
  results.hidden = true;
  results.innerHTML = "";
}

// 현재 위치를 출발지로
el("from-here").addEventListener("click", () => {
  if (!navigator.geolocation) {
    setStatus("이 브라우저는 위치를 지원하지 않음");
    return;
  }
  setStatus("현재 위치 확인 중");
  navigator.geolocation.getCurrentPosition(
    (position) => {
      const { latitude: lat, longitude: lon } = position.coords;
      setPoint("from", { lat, lon, name: "현재 위치" });
      map.flyTo({ center: [lon, lat], zoom: 15 });
      setStatus("현재 위치를 출발지로 정함");
      if (points.to) requestRoute();
    },
    (error) => setStatus(`위치를 얻지 못함: ${error.message}`),
    { enableHighAccuracy: true, timeout: 10000 },
  );
});

routeButton.addEventListener("click", requestRoute);
el("profile").addEventListener("change", () => {
  if (points.from && points.to) requestRoute();
});

// 경로 요청과 표시
async function requestRoute() {
  if (!(points.from && points.to)) return;
  setStatus("경로 탐색 중");
  const body = {
    from: { lat: points.from.lat, lon: points.from.lon },
    to: { lat: points.to.lat, lon: points.to.lon },
    profile: el("profile").value,
  };
  const response = await fetch("/route", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const result = await response.json();
  if (result.code !== "OK") {
    setStatus(`${result.message} (${result.code})`);
    clearRoute();
    return;
  }
  route = result.data;
  drawRoute(route);
  const minutes = Math.round(route.duration_s / 60);
  const km = (route.distance_m / 1000).toFixed(1);
  summary.textContent = `${km}km · ${minutes}분 · 안내 ${route.instructions.length}곳 · 경고 ${route.warnings.length}건 · ${route.elapsed_ms.toFixed(0)}ms`;
  summary.hidden = false;
  instructionsList.innerHTML = "";
  for (const instruction of route.instructions) {
    const li = document.createElement("li");
    li.textContent = instruction.text;
    const dist = document.createElement("span");
    dist.className = "dist";
    dist.textContent = formatDistance(instruction.at_m);
    li.appendChild(dist);
    li.addEventListener("click", () => map.flyTo({ center: [instruction.lon, instruction.lat], zoom: 16 }));
    instructionsList.appendChild(li);
  }
  instructionsList.hidden = false;
  setStatus(`${profileName(route.profile)} 경로`);
}

function formatDistance(m) {
  return m >= 1000 ? `${(m / 1000).toFixed(1)}km` : `${Math.round(m)}m`;
}

function profileName(profile) {
  return { time: "최소시간", distance: "최단거리", free: "무료도로" }[profile] ?? profile;
}

function routeGeoJson(route) {
  return {
    type: "Feature",
    properties: {},
    geometry: { type: "LineString", coordinates: route.geometry.map(([lat, lon]) => [lon, lat]) },
  };
}

function drawRoute(route) {
  const data = routeGeoJson(route);
  if (map.getSource("route")) {
    map.getSource("route").setData(data);
  } else {
    map.addSource("route", { type: "geojson", data });
    map.addLayer({
      id: "route-casing",
      type: "line",
      source: "route",
      layout: { "line-join": "round", "line-cap": "round" },
      paint: { "line-color": "#1b4f9c", "line-width": 9, "line-opacity": 0.6 },
    });
    map.addLayer({
      id: "route-line",
      type: "line",
      source: "route",
      layout: { "line-join": "round", "line-cap": "round" },
      paint: { "line-color": "#2a7de1", "line-width": 5 },
    });
  }
  const bounds = data.geometry.coordinates.reduce(
    (b, c) => b.extend(c),
    new maplibregl.LngLatBounds(data.geometry.coordinates[0], data.geometry.coordinates[0]),
  );
  map.fitBounds(bounds, { padding: { top: 40, right: 40, bottom: 40, left: 380 }, maxZoom: 16 });
}

function clearRoute() {
  route = null;
  if (map.getSource("route")) {
    map.getSource("route").setData({ type: "FeatureCollection", features: [] });
  }
  summary.hidden = true;
  instructionsList.hidden = true;
}

map.on("load", () => setStatus("출발과 도착을 검색하거나 지도를 클릭"));
map.on("error", (event) => setStatus(`지도 오류: ${event.error?.message ?? "알 수 없음"}`));

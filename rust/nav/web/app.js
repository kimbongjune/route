// 지도, 출발·도착 입력, 지오코딩 검색, 경로 요청과 경로선 표시.
// 배경은 nav-api가 프록시하는 브이월드 2D 타일이고 "자체 지도"를 켜면 style-day.json의 전자지도 벡터 타일 스타일로 바꾼다

const TILE_URL = `${location.origin}/tiles/base/{z}/{x}/{y}`;
const SEOUL_STATION = [126.9707, 37.5547];
// 자체 지도 스타일. 소스와 레이어를 style-day.json에서 읽고 {origin}을 이 서버 주소로 바꾼다. 화면 버전과 같이 올린다
const STYLE_VERSION = "13";
const EMAP_STYLE = JSON.parse(
  (await (await fetch(`style-day.json?v=${STYLE_VERSION}`)).text()).replaceAll("{origin}", location.origin),
);

const map = new maplibregl.Map({
  container: "map",
  style: {
    version: 8,
    glyphs: EMAP_STYLE.glyphs,
    sources: {
      vworld: {
        type: "raster",
        tiles: [TILE_URL],
        tileSize: 256,
        minzoom: 6,
        maxzoom: 19,
        attribution: "국토교통부 브이월드",
      },
      ...EMAP_STYLE.sources,
    },
    // 브이월드 래스터와 자체 지도 레이어를 모두 넣고 체크박스로 보이는 쪽을 바꾼다. 기본은 브이월드이다
    layers: [
      { id: "vworld", type: "raster", source: "vworld" },
      ...EMAP_STYLE.layers.map((layer) => ({ ...layer, layout: { ...(layer.layout ?? {}), visibility: "none" } })),
    ],
  },
  localIdeographFontFamily: "'Malgun Gothic', 'Apple SD Gothic Neo', sans-serif",
  center: SEOUL_STATION,
  zoom: 14,
  maxZoom: 19,
  minZoom: 6,
  attributionControl: { compact: true },
});
map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), "top-right");

// "자체 지도": 브이월드 래스터를 숨기고 전자지도 스타일 레이어를 보인다
document.getElementById("own-tiles").addEventListener("change", (event) => {
  const own = event.target.checked;
  map.setLayoutProperty("vworld", "visibility", own ? "none" : "visible");
  for (const layer of EMAP_STYLE.layers) {
    map.setLayoutProperty(layer.id, "visibility", own ? "visible" : "none");
  }
});

const el = (id) => document.getElementById(id);
const status = el("status");
const results = el("results");
const summary = el("summary");
const candidatesList = el("candidates");
// /routes가 준 후보 경로. 목록에서 고르면 그 경로를 쓴다
let candidates = [];
const instructionsList = el("instructions");
const routeButton = el("route-button");

// 출발·도착 지점. {lat, lon, name}이며 마커와 함께 관리한다
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
    nameByAddress(kind, lat, lng);
    requestRoute();
  });
  routeButton.disabled = !(points.from && points.to);
}

// 역지오코딩. 좌표로 정한 지점의 이름을 300m 안의 가장 가까운 도로명주소로 바꾼다. 없으면 좌표 그대로 둔다
async function nameByAddress(kind, lat, lon) {
  const response = await fetch(`/reverse?lat=${lat}&lon=${lon}`);
  const body = await response.json();
  const point = points[kind];
  if (body.code !== "OK" || !point || point.lat !== lat || point.lon !== lon) return;
  point.name = body.data.title;
  el(`${kind}-input`).value = point.name;
}

// 지도 클릭: 출발이 없으면 출발, 있으면 도착. 시뮬레이션 중이면 가짜 위치를 그 지점으로 옮겨 이탈을 확인한다
map.on("click", (event) => {
  const { lng, lat } = event.lngLat;
  if (simulationTimer !== null) {
    simulationJump = { lat, lon: lng };
    setStatus("시뮬레이션 위치를 옮김");
    return;
  }
  const kind = points.from ? "to" : "from";
  setPoint(kind, { lat, lon: lng, name: `${lat.toFixed(5)}, ${lng.toFixed(5)}` });
  nameByAddress(kind, lat, lng);
  hideResults();
  if (points.from && points.to) requestRoute();
});

// 지오코딩 검색. 입력이 멈추고 300ms 뒤에 /geocode를 부른다. 지도 중심을 같이 보내 같은 점수의 장소는 가까운 순으로 받는다
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
  const center = map.getCenter();
  const response = await fetch(
    `/geocode?q=${encodeURIComponent(q)}&size=8&lat=${center.lat.toFixed(6)}&lon=${center.lng.toFixed(6)}`,
  );
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
      const { latitude: lat, longitude: lon, heading } = position.coords;
      setPoint("from", { lat, lon, name: "현재 위치", heading });
      map.flyTo({ center: [lon, lat], zoom: 15 });
      setStatus("현재 위치를 출발지로 정함");
      if (points.to) requestRoute();
    },
    (error) => setStatus(`위치를 얻지 못함: ${error.message}`),
    { enableHighAccuracy: true, timeout: 10000 },
  );
});

routeButton.addEventListener("click", requestRoute);

// POST /route. 출발지에 방위각이 있으면 함께 보낸다
async function fetchRoute(from, to, profile) {
  const body = {
    from: { lat: from.lat, lon: from.lon, ...(Number.isFinite(from.heading) ? { heading: from.heading } : {}) },
    to: { lat: to.lat, lon: to.lon },
    profile,
  };
  const response = await fetch("/route", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return response.json();
}

// 경로 요청과 표시
// 세 프로파일의 후보를 한 번에 받아 비교 목록을 보이고 가장 빠른 후보를 먼저 고른다
async function requestRoute() {
  if (!(points.from && points.to)) return;
  setStatus("경로 탐색 중");
  const body = { from: { lat: points.from.lat, lon: points.from.lon }, to: { lat: points.to.lat, lon: points.to.lon } };
  if (Number.isFinite(points.from.heading)) body.from.heading = points.from.heading;
  const response = await fetch("/routes", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const result = await response.json();
  if (result.code !== "OK") {
    setStatus(`${result.message} (${result.code})`);
    clearRoute();
    return;
  }
  candidates = result.data.candidates;
  const index = Math.max(0, candidates.findIndex((c) => c.fastest));
  showCandidates(index);
  selectCandidate(index, true);
}

function selectCandidate(index, fit) {
  route = candidates[index];
  drawRoute(route, fit);
  prepareTracking(route);
  showRouteInfo(route);
  candidatesList.querySelectorAll("li").forEach((li, i) => li.classList.toggle("selected", i === index));
  setStatus(`${route.profiles.map(profileName).join("·")} 경로`);
}

// 후보 비교 목록. 시간, 거리, 통행료와 가장 빠름·짧음·저렴 표시
function showCandidates(selected) {
  candidatesList.innerHTML = "";
  candidates.forEach((c, i) => {
    const li = document.createElement("li");
    const tags = [];
    if (c.fastest) tags.push("가장 빠름");
    if (c.shortest) tags.push("가장 짧음");
    if (c.cheapest) tags.push("최소 요금");
    const toll = c.toll.unknown.length ? `통행료 ${c.toll.won.toLocaleString()}원+미확인` : c.toll.won > 0 ? `통행료 ${c.toll.won.toLocaleString()}원` : "무료";
    li.innerHTML = `<span class="name"></span><span>${Math.round(c.duration_s / 60)}분</span><span>${(c.distance_m / 1000).toFixed(1)}km</span><span>${toll}</span>`;
    li.querySelector(".name").textContent = c.profiles.map(profileName).join("·");
    for (const tag of tags) {
      const span = document.createElement("span");
      span.className = "tag best";
      span.textContent = tag;
      li.appendChild(span);
    }
    if (i === selected) li.classList.add("selected");
    li.addEventListener("click", () => selectCandidate(i, false));
    candidatesList.appendChild(li);
  });
  candidatesList.hidden = candidates.length === 0;
}

// 요약과 안내문 목록
function showRouteInfo(route) {
  const minutes = Math.round(route.duration_s / 60);
  const km = (route.distance_m / 1000).toFixed(1);
  const toll = route.toll && route.toll.won > 0 ? ` · 통행료 ${route.toll.won.toLocaleString()}원` : "";
  const unknown = route.toll && route.toll.unknown.length ? ` · 요금 미확인 ${route.toll.unknown.length}곳` : "";
  summary.textContent = `${km}km · ${minutes}분${toll}${unknown} · 안내 ${route.instructions.length}곳 · 경고 ${route.warnings.length}건 · ${route.elapsed_ms.toFixed(0)}ms`;
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

// 경로선을 그린다. fit이면 경로 전체가 보이게 화면을 맞춘다
function drawRoute(route, fit) {
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
  if (!fit) return;
  const bounds = data.geometry.coordinates.reduce(
    (b, c) => b.extend(c),
    new maplibregl.LngLatBounds(data.geometry.coordinates[0], data.geometry.coordinates[0]),
  );
  map.fitBounds(bounds, { padding: { top: 40, right: 40, bottom: 40, left: 380 }, maxZoom: 16 });
}

function clearRoute() {
  route = null;
  stopTracking();
  el("drive-button").disabled = true;
  el("simulate-button").disabled = true;
  if (map.getSource("route")) {
    map.getSource("route").setData({ type: "FeatureCollection", features: [] });
  }
  summary.hidden = true;
  instructionsList.hidden = true;
  candidates = [];
  candidatesList.hidden = true;
  candidatesList.innerHTML = "";
}

map.on("load", () => setStatus("출발과 도착을 검색하거나 지도를 클릭"));
map.on("error", (event) => setStatus(`지도 오류: ${event.error?.message ?? "알 수 없음"}`));

// ---------------------------------------------------------------
// 주행 추적. GPS 위치나 시뮬레이션 위치를 경로 위 가장 가까운 점으로 스냅하고 진행 거리를 센다
// ---------------------------------------------------------------

const SIMULATION_KMH = 60;
// 경로에서 이만큼 떨어진 위치가 OFF_ROUTE_COUNT회 연속이면 이탈이다. Mapbox 기본값 50m를 따른다
const OFF_ROUTE_M = 50;
const OFF_ROUTE_COUNT = 3;
// 최대 진행 거리보다 이만큼 뒤인 위치가 OFF_ROUTE_COUNT회 연속이면 역방향 이탈이다
const BACKTRACK_M = 50;
// 재탐색은 마지막 요청 지점에서 이만큼 움직였을 때만 다시 보낸다
const REROUTE_MIN_MOVE_M = 50;

// 이탈 판정 상태. 연속 횟수, 최대 진행 거리, 재탐색 중 여부, 마지막 재탐색 지점
let offCount = 0;
let backCount = 0;
let maxAtM = 0;
let rerouting = false;
let lastReroute = null;
// 시뮬레이션 진행 거리와 지도 클릭으로 옮긴 가짜 위치
let simulationTravelled = 0;
let simulationJump = null;

function distanceM(a, b) {
  const m = metersPerDegree(a.lat);
  return Math.hypot((a.lon - b.lon) * m.lon, (a.lat - b.lat) * m.lat);
}

// 경로 좌표 열(경도, 위도)과 꼭짓점마다 출발점부터의 거리(m). 좌표 열의 길이를 경로 거리에 맞춰 비례시킨다
let track = null;
let watchId = null;
let simulationTimer = null;
let positionMarker = null;

function metersPerDegree(lat) {
  return { lon: 111320 * Math.cos((lat * Math.PI) / 180), lat: 110950 };
}

// 경로 좌표 열과 누적 거리를 만든다
function buildTrack(route) {
  const coords = route.geometry.map(([lat, lon]) => [lon, lat]);
  const cumulative = [0];
  for (let i = 1; i < coords.length; i++) {
    const m = metersPerDegree((coords[i][1] + coords[i - 1][1]) / 2);
    const dx = (coords[i][0] - coords[i - 1][0]) * m.lon;
    const dy = (coords[i][1] - coords[i - 1][1]) * m.lat;
    cumulative.push(cumulative[i - 1] + Math.hypot(dx, dy));
  }
  const scale = cumulative[cumulative.length - 1] > 0 ? route.distance_m / cumulative[cumulative.length - 1] : 1;
  return { coords, cumulative: cumulative.map((c) => c * scale), route };
}

function prepareTracking(route) {
  track = buildTrack(route);
  stopTracking();
  el("drive-button").disabled = false;
  el("simulate-button").disabled = false;
}

// 추적 중에 경로를 새 것으로 바꾼다. 말한 안내와 이탈 상태를 처음으로 되돌리고 시뮬레이션은 새 경로의 처음부터 간다
function replaceRoute(data) {
  // 재탐색 응답은 /route 한 건이라 후보 목록의 profiles가 없다. 다음 재탐색이 같은 프로파일을 쓰도록 넣는다
  if (!data.profiles) data.profiles = [data.profile];
  route = data;
  track = buildTrack(route);
  drawRoute(route, false);
  showRouteInfo(route);
  spoken = new Set();
  shown = new Set();
  lastSnapped = null;
  lastFix = null;
  offCount = 0;
  backCount = 0;
  maxAtM = 0;
  simulationTravelled = 0;
  simulationJump = null;
}

// 이탈 재탐색. 현재 위치와 진행 방위각을 출발로, 도착지와 프로파일은 그대로 요청한다
async function reroute(lat, lon) {
  if (rerouting) return;
  if (lastReroute && distanceM(lastReroute, { lat, lon }) < REROUTE_MIN_MOVE_M) return;
  rerouting = true;
  lastReroute = { lat, lon };
  speak("경로를 이탈하여 재탐색합니다");
  setStatus("경로 이탈, 재탐색 중");
  try {
    const result = await fetchRoute({ lat, lon, heading }, points.to, route.profiles[0]);
    if (result.code !== "OK") {
      setStatus(`재탐색 실패: ${result.message}`);
      return;
    }
    replaceRoute(result.data);
    setStatus(`${profileName(route.profile)} 경로 (재탐색)`);
  } catch (error) {
    setStatus(`재탐색 실패: ${error.message}`);
  } finally {
    rerouting = false;
  }
}

// 위치를 경로 위 가장 가까운 점으로 스냅한다. 결과는 경로 위 점, 출발점부터의 거리, 경로에서 떨어진 거리
function snapToRoute(lat, lon) {
  const { coords, cumulative } = track;
  const m = metersPerDegree(lat);
  let best = null;
  for (let i = 1; i < coords.length; i++) {
    const [x0, y0] = coords[i - 1];
    const [x1, y1] = coords[i];
    const ax = (lon - x0) * m.lon;
    const ay = (lat - y0) * m.lat;
    const bx = (x1 - x0) * m.lon;
    const by = (y1 - y0) * m.lat;
    const len2 = bx * bx + by * by;
    const t = len2 > 0 ? Math.max(0, Math.min(1, (ax * bx + ay * by) / len2)) : 0;
    const dist = Math.hypot(ax - t * bx, ay - t * by);
    if (!best || dist < best.dist) {
      best = {
        dist,
        lon: x0 + t * (x1 - x0),
        lat: y0 + t * (y1 - y0),
        at_m: cumulative[i - 1] + t * (cumulative[i] - cumulative[i - 1]),
      };
    }
  }
  return best;
}

// 출발점부터 거리 d인 경로 위 점과 그 방위각
function pointAt(d) {
  const { coords, cumulative } = track;
  let i = 1;
  while (i < cumulative.length - 1 && cumulative[i] < d) i++;
  const span = cumulative[i] - cumulative[i - 1];
  const t = span > 0 ? Math.max(0, Math.min(1, (d - cumulative[i - 1]) / span)) : 0;
  const [x0, y0] = coords[i - 1];
  const [x1, y1] = coords[i];
  const m = metersPerDegree(y0);
  const bearing = (Math.atan2((x1 - x0) * m.lon, (y1 - y0) * m.lat) * 180) / Math.PI;
  return { lon: x0 + t * (x1 - x0), lat: y0 + t * (y1 - y0), bearing: (bearing + 360) % 360 };
}

function showPosition(lat, lon) {
  if (!positionMarker) {
    const dot = document.createElement("div");
    dot.className = "position";
    // 마커 element는 MapLibre가 위치를 transform으로 옮기므로 화살표 회전은 안쪽 요소에 준다
    const wrap = document.createElement("div");
    wrap.appendChild(dot);
    positionMarker = new maplibregl.Marker({ element: wrap, rotationAlignment: "map" }).setLngLat([lon, lat]).addTo(map);
  } else {
    positionMarker.setLngLat([lon, lat]);
  }
  return positionMarker.getElement().firstChild;
}

// 안내 시점(m). 안내 지점 앞 이 거리들에서 한 번씩 말하며 마지막은 "잠시 후"이다. 고속도로 출구는 더 멀리서 시작한다
const CITY_STAGES = [300, 50];
const EXIT_STAGES = [1000, 500, 50];
// 경고 알림 거리. 과속카메라는 티맵 일반도로 기본값과 같은 600m에서 한 번, 150m에서 "잠시 후"로 한 번 더 말하고 나머지는 300m에서 한 번이다
const CAMERA_STAGES = [600, 150];
const CAMERA_AHEAD_M = CAMERA_STAGES[0];
const WARNING_AHEAD_M = 300;
// 이 거리 안에 이어지는 과속방지턱은 한 구간으로 묶어 한 번만 알린다
const BUMP_GROUP_M = 100;
// 과속카메라 앞에서 현재 속도가 제한속도를 넘으면 경고음을 낸다
const SPEEDING_AHEAD_M = 300;
// 음성으로 읽는 회전 말. 보통 내비게이션 문구를 따른다
const TURN_SPEECH = {
  straight: "직진",
  slight_left: "왼쪽 방향",
  slight_right: "오른쪽 방향",
  left: "좌회전",
  right: "우회전",
  u_turn: "유턴",
  exit_left: "왼쪽 출구",
  exit_right: "오른쪽 출구",
};

// 음성용 거리. 1km 미만은 50m 단위, 그 위는 소수 한 자리 킬로미터
function spokenDistance(m) {
  if (m < 1000) return `${Math.max(50, Math.round(m / 50) * 50)}미터`;
  const km = Math.round(m / 100) / 10;
  return `${Number.isInteger(km) ? km : km.toFixed(1)}킬로미터`;
}

// 안내 지점 하나의 음성 문장. 교차로 이름이 있으면 "~에서", 출구는 방면을 덧붙인다
function speechFor(step, instruction, ahead, last) {
  const turn = step.clock ? `${step.clock}시 방향` : (TURN_SPEECH[instruction.turn] ?? "직진");
  const where = isIntersectionName(step.node_name) ? `${step.node_name}에서 ` : "";
  const head = last ? "잠시 후 " : `${spokenDistance(ahead)} 앞 `;
  let text = `${head}${where}${turn}입니다`;
  if (instruction.turn.startsWith("exit") && step.to_name) {
    text += `. ${step.to_name} 방면`;
    if (instruction.guide_line) text += `, ${instruction.guide_line} 유도선`;
    text += "입니다";
  }
  return text;
}

// 서버의 is_intersection_name과 같은 규칙. 마지막 낱말이 숫자나 "숫자-숫자"로 끝나면 지번이다
function isIntersectionName(name) {
  if (!name) return false;
  const tail = name.split(" ").pop().replace(/^[^0-9-]+/, "");
  return !(/^[0-9-]+$/.test(tail) && /[0-9]/.test(tail));
}

// 경고의 음성 문장. 사고다발 구간은 화면으로만 알린다. 카메라의 마지막 단계는 "잠시 후"이다
function warningSpeech(w, ahead, last = false) {
  const head = last ? "잠시 후" : `${spokenDistance(ahead)} 앞`;
  if (w.kind === "speed_camera") {
    const limit = w.text.match(/제한속도 (\d+)/);
    return `${head} 과속 단속 카메라입니다${limit ? `. 제한 속도 ${limit[1]}` : ""}`;
  }
  if (w.kind === "camera") {
    const limit = w.text.match(/제한속도 (\d+)/);
    return `${head} ${w.text.split(",")[0]}입니다${limit ? `. 제한 속도 ${limit[1]}` : ""}`;
  }
  if (w.kind === "speed_bump") return "과속방지턱입니다";
  if (w.kind === "toll") {
    const won = w.text.match(/통행료 (\d+)원/);
    return won ? `전방에 요금소가 있습니다. 통행료 ${Number(won[1]).toLocaleString()}원` : "전방에 요금소가 있습니다";
  }
  return null;
}
// 진행 방향을 위로 두는 카메라의 기울기. 브이월드 래스터 타일은 글자가 같이 돌아가므로 기본은 북쪽이 위이고 체크하면 돈다
const HEADING_UP_PITCH = 45;
// 진행 방위각. 5m 넘게 움직이기 전에는 없다
let heading = null;

// 말한 안내와 경고. 안내는 "번호:단계", 경고는 "w번호", 과속은 "s번호". 화면에 띄운 경고는 shown이다
let spoken = new Set();
let shown = new Set();
let lastSnapped = null;
let lastFix = null;
// 현재 속도(km/h). 위치 간 거리와 시간으로 잰다
let speedKmh = 0;

// 지금 말하는 문장의 종류. 회전 안내는 경고만 끊고 안내는 뒤에 이어 말한다.
// 끊긴 경고는 말한 기록에서 지워 회전 안내가 끝난 뒤 그때 거리로 다시 말한다
let speakingKind = null;
let speakingWarningKey = null;

function speak(text, kind = "turn", key = null) {
  if (!el("voice").checked || !("speechSynthesis" in window)) return;
  if (speechSynthesis.speaking && speakingKind === "warning") {
    if (speakingWarningKey) spoken.delete(speakingWarningKey);
    speakingWarningKey = null;
    speechSynthesis.cancel();
  }
  const utterance = new SpeechSynthesisUtterance(text);
  utterance.lang = "ko-KR";
  utterance.rate = 1.05;
  utterance.onstart = () => {
    speakingKind = kind;
    speakingWarningKey = key;
  };
  utterance.onend = () => {
    speakingKind = null;
    if (speakingWarningKey === key) speakingWarningKey = null;
  };
  speechSynthesis.speak(utterance);
}

// 경고는 말하는 중이면 미룬다. 말했으면 true. key는 끊겼을 때 다시 말하기 위한 말한 기록의 키이다
function speakIfIdle(text, key = null) {
  if (!el("voice").checked || !("speechSynthesis" in window)) return true;
  if (speechSynthesis.speaking) return false;
  speak(text, "warning", key);
  return true;
}

// 짧은 경고음. 과속 때 두 번 울린다
function beep() {
  try {
    const ctx = new (window.AudioContext || window.webkitAudioContext)();
    for (const start of [0, 0.25]) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.frequency.value = 880;
      gain.gain.value = 0.2;
      osc.connect(gain).connect(ctx.destination);
      osc.start(ctx.currentTime + start);
      osc.stop(ctx.currentTime + start + 0.15);
    }
  } catch (_) {
    // 소리를 낼 수 없는 환경이면 넘어간다
  }
}

// 경고 문구를 짧게. 사고다발 구간은 유형만 보인다
function alertText(w) {
  if (w.kind === "accident_zone") return `사고다발 구간 (${w.text.split(" ")[0]})`;
  return w.text;
}

function showAlert(kind, text) {
  const div = document.createElement("div");
  div.className = `alert ${kind}`;
  div.textContent = text;
  el("alerts").prepend(div);
  setTimeout(() => div.remove(), 8000);
}

// 위치 하나를 처리한다. 스냅한 진행 거리로 다음 안내와 남은 거리·시간을 보이고 안내 시점과 경고를 말한다
function onPosition(lat, lon) {
  const snapped = snapToRoute(lat, lon);
  const dot = showPosition(lat, lon);
  const off = snapped.dist > OFF_ROUTE_M;
  dot.classList.toggle("off", off);
  const { route } = track;

  // 이탈 판정. 경로에서 50m 넘게 떨어진 위치나 최대 진행 거리보다 50m 넘게 뒤인 위치가 3회 연속이면 재탐색한다
  offCount = off ? offCount + 1 : 0;
  if (!off) maxAtM = Math.max(maxAtM, snapped.at_m);
  backCount = !off && snapped.at_m < maxAtM - BACKTRACK_M ? backCount + 1 : 0;
  if (offCount >= OFF_ROUTE_COUNT || backCount >= OFF_ROUTE_COUNT) reroute(lat, lon);
  const remainingM = Math.max(0, route.distance_m - snapped.at_m);
  const remainingS = (remainingM / route.distance_m) * route.duration_s;

  // 다음 안내. 지난 안내는 목록에서 흐리게, 다음 안내는 굵게
  const nextIndex = route.instructions.findIndex((i) => i.at_m > snapped.at_m + 5);
  const next = nextIndex >= 0 ? route.instructions[nextIndex] : null;
  for (const [i, li] of [...instructionsList.children].entries()) {
    li.classList.toggle("done", nextIndex < 0 || i < nextIndex);
    li.classList.toggle("current", i === nextIndex);
  }
  if (next) {
    const ahead = next.at_m - snapped.at_m;
    el("banner-dist").textContent = formatDistance(ahead);
    el("banner-text").textContent = next.text;
    const stages = next.turn.startsWith("exit") ? EXIT_STAGES : CITY_STAGES;
    // 아직 말하지 않은 단계 중 남은 거리 안에 든 것들. 가장 가까운 단계로 한 번만 말하고 나머지는 지난 것으로 처리한다
    const due = stages.filter((s) => ahead <= s && !spoken.has(`${nextIndex}:${s}`));
    if (due.length > 0) {
      due.forEach((s) => spoken.add(`${nextIndex}:${s}`));
      const stage = due[due.length - 1];
      speak(speechFor(route.steps[nextIndex], next, ahead, stage === stages[stages.length - 1]));
    }
  } else {
    el("banner-dist").textContent = formatDistance(remainingM);
    el("banner-text").textContent = "목적지";
  }
  el("banner").hidden = false;
  el("next-instruction").textContent = next ? `${formatDistance(next.at_m - snapped.at_m)} 앞 ${next.text}` : "곧 도착";
  el("remaining").textContent =
    `남은 거리 ${formatDistance(remainingM)} · ${Math.max(1, Math.round(remainingS / 60))}분` +
    (off ? ` · 경로에서 ${Math.round(snapped.dist)}m 벗어남` : "") +
    (speedKmh > 0 ? ` · ${Math.round(speedKmh)}km/h` : "");
  el("progress").hidden = false;

  // 현재 속도. 1초 넘게 지난 두 위치의 진행 거리로 잰다
  const now = Date.now();
  if (lastFix && now - lastFix.time >= 1000) {
    speedKmh = ((snapped.at_m - lastFix.at_m) / ((now - lastFix.time) / 1000)) * 3.6;
  }
  if (!lastFix || now - lastFix.time >= 1000) lastFix = { time: now, at_m: snapped.at_m };

  // 근접 알림. 카메라는 600m와 150m, 나머지는 300m 앞에서 한 번씩. 말하는 중이면 미루고 지나치기 전까지 다시 시도한다
  route.warnings.forEach((w, i) => {
    if (w.at_m === undefined || w.at_m === null) return;
    const ahead = w.at_m - snapped.at_m;
    const isCamera = w.kind === "speed_camera" || w.kind === "camera";
    const stages = isCamera ? CAMERA_STAGES : [WARNING_AHEAD_M];
    // 아직 말하지 않은 단계 중 거리 안에 든 것. 여럿이면 가장 가까운 단계로 한 번만 말한다
    const due = stages.filter((s) => ahead <= s && !spoken.has(`w${i}:${s}`));
    if (ahead >= -20 && due.length) {
      const stage = due[due.length - 1];
      const key = `w${i}:${stage}`;
      // 이어지는 방지턱은 한 구간으로 묶는다
      let grouped = false;
      if (w.kind === "speed_bump") {
        for (let j = i + 1; j < route.warnings.length && route.warnings[j].at_m - w.at_m <= BUMP_GROUP_M; j++) {
          if (route.warnings[j].kind === "speed_bump") {
            spoken.add(`w${j}:${WARNING_AHEAD_M}`);
            shown.add(`w${j}`);
            grouped = true;
          }
        }
      }
      if (!shown.has(`w${i}`)) {
        shown.add(`w${i}`);
        const kind = { speed_bump: "bump", accident_zone: "accident", toll: "toll" }[w.kind] ?? "camera";
        showAlert(kind, `${formatDistance(Math.max(0, ahead))} 앞 ${grouped ? "과속방지턱 연속 구간" : alertText(w)}`);
      }
      // 음성은 말하는 중이면 미루고 지나치기 전까지 다시 시도한다. 회전 안내가 끊으면 다시 말한다
      const last = isCamera && stage === CAMERA_STAGES[CAMERA_STAGES.length - 1];
      const speech = grouped ? "과속방지턱 연속 구간입니다" : warningSpeech(w, Math.max(0, ahead), last);
      if (!speech || speakIfIdle(speech, key)) due.forEach((s) => spoken.add(`w${i}:${s}`));
    }
    // 과속카메라 앞에서 제한속도를 넘으면 경고음과 함께 한 번 알린다
    if (w.kind === "speed_camera" && ahead >= 0 && ahead <= SPEEDING_AHEAD_M && !spoken.has(`s${i}`)) {
      const limitKmh = Number((w.text.match(/제한속도 (\d+)/) ?? [])[1]);
      if (limitKmh && speedKmh > limitKmh) {
        spoken.add(`s${i}`);
        beep();
        showAlert("camera", `제한 속도 ${limitKmh} 초과 (현재 ${Math.round(speedKmh)}km/h)`);
        speakIfIdle("제한 속도를 초과했습니다");
      }
    }
  });

  // 진행 방위각. 5m 넘게 움직였을 때만 바꾼다
  if (lastSnapped && snapped.at_m - lastSnapped.at_m > 5) {
    const m = metersPerDegree(lat);
    heading = ((Math.atan2((snapped.lon - lastSnapped.lon) * m.lon, (snapped.lat - lastSnapped.lat) * m.lat) * 180) / Math.PI + 360) % 360;
  }
  lastSnapped = snapped;
  const headingUp = el("heading-up").checked;
  const camera = headingUp
    ? { bearing: heading ?? 0, pitch: HEADING_UP_PITCH, padding: { top: Math.round(map.getContainer().clientHeight * 0.4), bottom: 0, left: 0, right: 0 } }
    : { bearing: 0, pitch: 0, padding: { top: 0, bottom: 0, left: 0, right: 0 } };
  map.easeTo({ center: [lon, lat], zoom: Math.max(map.getZoom(), 16), duration: 900, ...camera });
  // rotationAlignment가 map이라 마커가 지도와 함께 돌므로 화살표는 진행 방위각만큼만 돌린다
  dot.style.transform = `rotate(${heading ?? 0}deg)`;
}

function startDriving() {
  if (!track) return;
  if (!navigator.geolocation) {
    setStatus("이 브라우저는 위치를 지원하지 않음");
    return;
  }
  stopTracking();
  setStatus("GPS 추적 중");
  watchId = navigator.geolocation.watchPosition(
    (position) => onPosition(position.coords.latitude, position.coords.longitude),
    (error) => setStatus(`위치를 얻지 못함: ${error.message}`),
    { enableHighAccuracy: true, maximumAge: 1000, timeout: 15000 },
  );
  showStopButton(true);
}

// 경로를 따라 SIMULATION_KMH로 움직이는 가짜 위치. 좌표를 최대 5m 무작위로 흔든다
function startSimulation() {
  if (!track) return;
  stopTracking();
  setStatus(`시뮬레이션 ${SIMULATION_KMH}km/h`);
  simulationTravelled = 0;
  const stepM = (SIMULATION_KMH / 3.6) * 1;
  speak("안내를 시작합니다");
  simulationTimer = setInterval(() => {
    // 지도 클릭으로 옮긴 위치가 있으면 거기 멈춰 있는 것으로 하여 이탈 판정과 재탐색을 거친다
    if (simulationJump) {
      onPosition(simulationJump.lat, simulationJump.lon);
      return;
    }
    simulationTravelled += stepM;
    if (simulationTravelled >= track.route.distance_m) {
      const end = pointAt(track.route.distance_m);
      onPosition(end.lat, end.lon);
      stopTracking();
      setStatus("도착");
      speak("목적지에 도착했습니다");
      return;
    }
    const p = pointAt(simulationTravelled);
    const jitter = metersPerDegree(p.lat);
    onPosition(p.lat + ((Math.random() - 0.5) * 10) / jitter.lat, p.lon + ((Math.random() - 0.5) * 10) / jitter.lon);
  }, 1000);
  showStopButton(true);
}

function stopTracking() {
  spoken = new Set();
  shown = new Set();
  lastSnapped = null;
  lastFix = null;
  speedKmh = 0;
  heading = null;
  offCount = 0;
  backCount = 0;
  maxAtM = 0;
  lastReroute = null;
  simulationJump = null;
  el("banner").hidden = true;
  el("alerts").innerHTML = "";
  if ("speechSynthesis" in window) speechSynthesis.cancel();
  speakingKind = null;
  if (watchId !== null) {
    navigator.geolocation.clearWatch(watchId);
    watchId = null;
  }
  if (simulationTimer !== null) {
    clearInterval(simulationTimer);
    simulationTimer = null;
  }
  showStopButton(false);
}

function showStopButton(on) {
  el("stop-button").hidden = !on;
  el("drive-button").hidden = on;
  el("simulate-button").hidden = on;
}

el("drive-button").addEventListener("click", startDriving);
el("simulate-button").addEventListener("click", startSimulation);
el("stop-button").addEventListener("click", () => {
  stopTracking();
  map.easeTo({ pitch: 0, bearing: 0, padding: { top: 0, bottom: 0, left: 0, right: 0 } });
  setStatus("추적 정지");
});

// 자체 지도에서 줌을 바꿀 때 화면이 다 그려지기까지의 시간을 잰다. 브라우저 콘솔(http://localhost:3000)에 붙여 넣어 실행한다.
// - 서울역 줌 14에서 13·…·7·6(대한민국 전역)으로 한 단계씩 줄인 뒤 7·…·14로 늘리며, 단계마다 jumpTo 뒤 idle 이벤트까지의 ms를 출력한다
// - 타일 주소에 실행마다 다른 값을 더해 브라우저 캐시를 쓰지 않는다(서버 GeoWebCache는 그대로 사용한다)
// - 새로고침한 페이지에서 한 번씩 실행한다. 앞 실행의 타일이 지도 메모리에 남아 있으면 빨라진다
(async () => {
  const map = window.navMap;
  const bust = String(Date.now());
  map.setTransformRequest((url) =>
    /\/(vtiles|terrain)\//.test(url) ? { url: `${url}${url.includes("?") ? "&" : "?"}b=${bust}` } : { url });
  const own = document.getElementById("own-tiles");
  if (!own.checked) own.click();
  const idle = () => new Promise((r) => map.once("idle", r));
  map.jumpTo({ center: [126.9707, 37.5547], zoom: 14 });
  await idle();
  const step = async (zooms) => {
    const each = [];
    const t0 = performance.now();
    for (const z of zooms) {
      const t = performance.now();
      map.jumpTo({ zoom: z });
      await idle();
      each.push(Math.round(performance.now() - t));
    }
    return `${Math.round(performance.now() - t0)}ms [${each.join(", ")}]`;
  };
  const out = await step([13, 12, 11, 10, 9, 8, 7, 6]);
  const back = await step([7, 8, 9, 10, 11, 12, 13, 14]);
  console.log(`줌 14→6 ${out}, 줌 6→14 ${back}, 작업 스레드 ${maplibregl.getWorkerCount()}`);
})();

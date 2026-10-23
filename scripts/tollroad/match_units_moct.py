"""영업소를 표준노드링크(MOCT) 요금소 노드에 매칭한다. MOCT가 기준이고 국가관심지점 요금소 POI와 도로공사 영업소 좌표는 보조·검증용이다.

입력
  ETC_75_04_01_168858.csv          영업소 마스터. 가동 Y, 가상 N인 646건
  API_수집/국가관심지점_요금소.csv  국토지리정보원 국가관심지점 요금소 864건(extract_ngii_poi.py)
  도로공사 영업소 위치정보 API       locationinfoUnit, 실행 시 호출. 좌표가 있는 영업소 398건
  MOCT_NODE.dbf/.shp                요금소 노드. 이름에 TG, 요금소, 톨게이트가 포함된 노드. 상호, 정류장, 앞사거리는 제외
참조 좌표
  영업소마다 도로공사 API 좌표와, 영업소명을 정규화한 이름과 같은 이름의 국가 POI를 참조 좌표로 둔다.
  POI 이름은 괄호, 한국도로공사, 요금징수소·톨게이트·요금소·영업소·TG, 하이패스를 제거하고 분기점을 JC로 바꿔 비교하며
  같은 이름이 없으면 영업소명을 포함하는 POI 이름을 쓴다. 같은 이름의 POI가 1km 이내면 한 요금소이고,
  요금소가 여러 곳이면 도로공사 좌표 300m 이내의 요금소를 선택하며 USER_POI 표의 좌표로 선택한 영업소도 있다.
순서
  1) 영업소명을 정규화한 이름(괄호, 본선/지선, 상/하/입구/출구, 방위, 숫자 제거, JCT를 JC로)으로 같은 이름의 요금소 노드를 찾는다.
     같은 이름의 노드가 1km 이내에 있으면 한 요금소다. 상·하행, 방향별 노드가 한 요금소에 속한다.
  2) 같은 이름의 요금소가 여러 곳이면 도로공사 좌표, 국가 POI 순으로 300m 이내의 요금소를 선택하고, 요금소가 두 곳이고
     영업소명에 남·북·동·서가 있으면 그 방위의 요금소를 선택한다. 선택하지 못하면 '동명 미해결'.
     이름이 다른 요금소 노드는 USER_NODE 표에 노드를 직접 지정하였다.
  3) 이름이 일치하지 않으면 참조 좌표 100m 이내에 요금소 노드가 있을 때 그 요금소로 한다. 기준 'MOCT노드(참조위치)'.
  4) 그것도 없으면 국가 POI 좌표를 그대로 사용한다. 기준 '국가POI'. 한 요금소의 POI가 100m 넘게 떨어져 있으면 지점마다 행을 둔다.
  5) MOCT 노드도 국가 POI도 없는 영업소는 '없음'이다. 도로공사 좌표는 브이월드 위성사진으로 요금소 위치임을 확인한
     USER_API 표의 영업소에만 기준 '도로공사'로 쓴다. 영업소명과 다른 이름의 POI는 ALIAS 표로 잇는다.
검증: MOCT 요금소 노드와 참조 좌표의 거리. 100m 이내 '일치', 300m 이내 '근접', 그 밖 '불일치', 참조 좌표 없음 '참조없음'.
제외: 무료화 4건, 구간 지정 2건(용마터널·청라하늘대교), IC 구간 요금 13건, 요금 자료 없는 4건.
출력: maps/유료도로/영업소_요금지점.csv
  컬럼: 영업소코드, 영업소명, 운영기관코드, 기준, NODE_IDS, LINK_IDS, 노드명, 위도, 경도, 참조출처, 참조명, 참조위도, 참조경도, 참조거리m, 검증, 비고
  LINK_IDS는 분기점 출구·입구처럼 방향별 램프 링크를 직접 지정한 영업소(USER_FIX)에만 있다.

python -X utf8 scripts/tollroad/match_units_moct.py
"""
import math
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import TOLLROAD_DIR, env, get_json, read_csv, write_csv
from nodelink import read_nodes, tm_to_wgs84, wgs84_to_tm

CLUSTER_M = 1000.0  # 같은 이름의 노드나 POI를 한 요금소로 보는 거리. 방향별 요금소가 수백 m 떨어진 경우가 있다: 노포, 시화, 북장성
PICK_M = 300.0      # 같은 이름의 요금소 중 참조 좌표로 선택하는 거리
ADOPT_M = 100.0     # 이름이 일치하지 않아도 참조 좌표에서 이 거리 이내의 요금소 노드를 사용한다
SPLIT_M = 60.0      # 운영기관이 다른 영업소가 한 요금소의 노드를 공유할 때 각자 갖는 노드의 거리
POINT_M = 100.0     # 한 요금소 안에서 POI 지점을 묶는 거리
EXCLUDE = {
    "023": "무료화(범안로 2026-09-01)", "024": "무료화(범안로 2026-09-01)", "095": "무료화(백양터널 2025-01)",
    "093": "징수 폐지(개금)", "030": "구간 처리(용마터널)", "032": "구간 처리(청라하늘대교)",
    "829": "IC 구간 요금(서울문산)", "602": "IC 구간 요금(천안논산)", "612": "IC 구간 요금(천안논산)",
    "618": "IC 구간 요금(천안논산)", "608": "IC 구간 요금(천안논산)", "622": "IC 구간 요금(대구부산)",
    "629": "IC 구간 요금(대구부산)", "802": "IC 구간 요금(봉담송산)", "812": "IC 구간 요금(봉담송산)",
    "840": "IC 구간 요금(옥산오창)", "850": "IC 구간 요금(옥산오창)", "871": "IC 구간 요금(서부내륙)",
    "882": "IC 구간 요금(서부내륙)",
    "670": "요금 자료 없음(TCS_AJ에 670 출발·도착 0건). 군위JC 양방향 요금은 650 군위출구JC에 있음",
    "690": "요금 자료 없음(TCS_AJ에 690 출발·도착 0건). 화산JC 양방향 요금은 680 화산출구JC에 있음",
    "089": "요금소 없음(정왕IC만 있음, 사용자 확인 2026-10-16). TCS_AJ에도 0건",
    "950": "요금 자료 없음(TCS_AJ에 950 출발·도착 0건). 영업소 목록에만 있고 MOCT·국가 POI·도로공사 좌표 어디에도 요금소 없음",
}
# 지도에서 직접 확인한 결과(2026-10-16). POI = 국가 POI 좌표 사용, MOCT = MOCT 노드 사용, 둘다 = 요금소가 두 곳이라 둘 다 사용.
# 지도 확인 정보와 MOCT 노드·링크 위치를 대조해 결정했다. 근거는 비고에 있다.
USER_CHECK = {
    "641": ("POI", "도로공사 영업소 위치정보 LOD(2019)도 641 울산을 35.557287,129.249217(국가 POI 울산요금소 자리)로 둠. 요금소는 범서읍 굴화리 울산JC 100m 앞. MOCT에는 거기 요금소 노드가 없고 울산고속도로 본선 링크 1960027727·1960027802가 5~15m. MOCT 울산TG 노드(7.4km 서쪽)는 옛 자리"),
    "833": ("MOCT", "서용인(서)톨게이트 37.2786991,127.2069725 옆. 국가 POI 서용인요금소는 835 서용인 자리"),
    "834": ("MOCT", "서용인(동)톨게이트 37.2621478,127.1791798 옆. 국가 POI 서용인요금소는 835 서용인 자리"),
    "219": ("POI", "MOCT 신림TG 노드는 신림IC에서 떨어진 자리. 국가 POI 신림요금소가 요금소"),
    "255": ("둘다", "서안산IC 램프에 요금소가 둘. 하이패스전용(37.3467950,126.7967691) = MOCT 서안산TG 노드 2170008201·02, 램프 2170016700~17000. 일반(국가 POI 서안산요금소) = MOCT 속성변화점 2170076401·02, 램프 2170238100~238500. 영업소 목록에 서안산은 255 하나라 둘 다 255"),
    "143": ("POI", "국가 POI 장지요금소가 정확한 요금소 위치. MOCT 노드는 410m 떨어짐"),
    "145": ("둘다", "산인JC에 요금소가 둘. 모곡리(35.2717304,128.4714970) = MOCT 산인TG 노드. 신산리(국가 POI 산인요금소) = MOCT 남해고속도로제1지선 램프 3900765100(39m). 영업소 목록에 산인은 145 하나라 둘 다 145"),
    "190": ("MOCT", "동서울톨게이트 37.5172142,127.2211459 는 MOCT 동서울TG 노드에 닿는 중부고속도로 본선 링크(2270001003·2270000903) 위. 같은 링크라 그대로"),
    "150": ("MOCT", "북부산톨게이트 35.2214010,128.9414999 는 MOCT 북부산TG 노드에 닿는 남해고속도로 본선 링크(1410203503·1410203401) 위. 같은 링크라 그대로"),
    "548": ("MOCT", "요금소는 무안공항톨게이트 자리 하나(MOCT 북무안TG 노드, 무안광주고속도로 본선). 북무안IC 램프에는 요금소 없음(사용자 확인). 547 무안공항과 같은 요금소"),
    "547": ("MOCT", "548 북무안과 같은 본선 요금소(MOCT 북무안TG 노드). 사용자 확인"),
    "225": ("MOCT", "서진주톨게이트 35.1856510,128.0449630 는 MOCT 서진주TG 노드의 통영대전고속도로 램프 링크(3810032400·32500) 위. 같은 링크"),
    "243": ("MOCT", "해인사톨게이트 35.7114846,128.1740974 는 MOCT 해인사TG 노드의 광주대구고속도로 램프 링크(3980060101·60000) 위. 같은 링크"),
    "279": ("MOCT", "곤양톨게이트 35.0542183,127.9633235 는 MOCT 곤양TG 노드의 남해고속도로 램프 링크(3840018100·18200) 위. 같은 링크"),
    "290": ("MOCT", "축동톨게이트 35.0887809,128.0405863 는 MOCT 축동TG 노드의 남해고속도로 램프 링크(3840028400·28300) 위. 같은 링크"),
    "507": ("MOCT", "함평톨게이트 35.0974081,126.4863211 는 MOCT 함평TG 노드의 서해안고속도로 램프 링크(3400020100·20200) 위. 같은 링크"),
    "240": ("MOCT", "제천톨게이트 두 곳(37.1339415,128.1469397 / 37.1314311,128.1382176) 모두 MOCT 제천TG 노드(각 2개)의 중앙고속도로 램프 링크 위. 둘 다 240"),
    "068": ("MOCT", "김포톨게이트 37.5928076,126.7676168 는 MOCT 김포TG 노드의 수도권제1순환 본선 링크(2320002000·1900) 위. 같은 링크"),
    "212": ("MOCT", "북수원톨게이트 37.3259969,126.9882305 는 MOCT 북수원TG 노드의 영동고속도로 램프 링크(2000004200·4100) 위. 같은 링크"),
    "208": ("MOCT", "서안동톨게이트 36.5883990,128.6212749 는 MOCT 서안동TG 노드의 중앙고속도로 램프 링크(3540966201·966101) 위. 같은 링크"),
    "221": ("MOCT", "생초톨게이트 35.4851817,127.8290330 는 MOCT 생초TG 노드의 통영대전고속도로 램프 링크(3950038400·38500) 위. 같은 링크"),
    "119": ("MOCT", "판암톨게이트 36.3178318,127.4683739 는 MOCT 판암TG 노드의 통영대전고속도로 램프 링크(1830006101·6201) 위. 같은 링크"),
    "724": ("MOCT", "서함양 하이패스 요금소 두 곳: 아래쪽 35.5405553,127.7091478 = 고속도로 진입, 위쪽 35.5423753,127.7113897 = 진출(사용자 로드뷰 확인). 각각 MOCT 서함양하이패스TG 노드(37m·3m)의 광주대구고속도로 램프 링크 위. 둘 다 724"),
    "508": ("MOCT", "무안톨게이트 35.0077658,126.4974609 는 MOCT 무안TG 노드의 서해안고속도로 램프 링크(3390023700·23800) 위. 같은 링크"),
    "732": ("없음", "행담도 요금소는 휴게소에서 서해안고속도로로 나가는 램프 위(사용자 확인)인데 국가 POI 행담IC(하이패스IC) 36.943161,126.809349 는 본선 교차점이고 MOCT_LINK에는 서해안 본선만 있어 공공 자료로는 요금 지점을 정할 수 없음"),
    "220": ("없음", "덕평 요금소는 휴게소에서 영동고속도로로 나가는 도로 위(사용자 로드뷰 확인)인데 국가 POI에 없고 그 도로가 MOCT_LINK에 없어 공공 자료로는 요금 지점을 정할 수 없음"),
    "164": ("POI", "국가 POI 무주요금소가 정확한 요금소 위치. MOCT 노드는 301m 떨어짐"),
    "327": ("없음", "서영천 영업소명의 요금소 노드와 국가 POI가 없음. 금호대창하이패스TG 노드가 그 자리로 보이나 이름이 달라 근거 없이 지정하지 않음"),
    "033": ("POI", "MOCT 고양TG 노드는 서울문산선 820 고양본선 자리. 수도권제1순환 고양TG는 국가 POI 고양요금소(주교동)"),
    "035": ("POI", "MOCT 양주TG 노드는 구리포천 879 양주 자리. 수도권제1순환 양주(개)는 국가 POI 양주요금소(장흥면)"),
}
# 분기점 출구·입구 영업소: 지도에서 확인한 요금소 좌표와 MOCT 램프 링크의 진행 방향으로 구분했다(2026-10-16).
# 출구JC = 그 영업소 노선에서 진출하는 램프, 입구JC = 진입하는 램프. 화도는 상·하행 본선 노드.
USER_FIX = {
    # 근거: TCS_AJ 요금표에서 각 영업소가 어느 운영기관 요금의 출발/도착으로 사용되는지(= 어느 방향 차량이 통과하는지)와,
    #       지도에서 확인한 요금소 좌표가 MOCT의 어느 램프·본선 링크 위에 있는지(2026-10-16).
    "710": {"links": ["3860500505"], "lat": 35.517362, "lon": 128.798290, "name": "밀양(상) 요금소(아래쪽)",
            "note": "함양울산→중앙(대구부산) 램프. TCS: 710은 도로공사 요금의 도착(함양울산에서 나감). 사용자 좌표에서 1.4m"},
    "720": {"links": ["3860500404"], "lat": 35.517831, "lon": 128.798358, "name": "밀양(하) 요금소(위쪽)",
            "note": "중앙(대구부산)→함양울산 램프. TCS: 720은 대구부산(02) 요금의 도착(대구부산에서 나와 함양울산으로). 사용자 좌표에서 2.1m"},
    "650": {"links": ['3610348400', '3610347101'], "lat": 36.1552292, "lon": 128.5608239, "name": "군위JC 요금소(양방향)",
            "note": "TCS: 650이 도로공사·상주영천(45) 요금 모두에서 출발·도착으로 쓰임 = 양방향 요금소, 670은 요금 0건. 군위(상) = 상주영천→중앙 램프(36.1552292,128.5608239 에서 1.3m), 군위(하) = 중앙→상주영천 램프(36.1553938,128.5591180 에서 2.7m, 사용자 로드뷰 확인) 둘 다 650"},
    "680": {"links": ['3570351801', '3570352001'], "lat": 36.0253175, "lon": 128.8863057, "name": "화산JC 요금소(양방향)",
            "note": "TCS: 680이 도로공사·상주영천(45) 요금 모두의 도착으로 쓰임 = 양방향 요금소, 690은 요금 0건. 화산(하)(위쪽) = 상주영천→익산포항 램프(36.0253175,128.8863057 에서 1.0m), 화산(상)(아래쪽) = 익산포항→상주영천 램프(36.0247101,128.8873645 에서 0.3m, 사용자 로드뷰 확인) 둘 다 680"},
    "667": {"links": ["2340288600"], "lat": 37.3737112, "lon": 127.3247383, "name": "경기광주JC 요금소",
            "note": "광주원주→중부·제2중부 램프 하나만(사용자 항공사진 확인 2026-10-16). 중부→광주원주 램프에는 요금소 없음. 옆 램프 두 곳은 660 초월"},
    "660": {"links": ["2340288700", "2340288800"], "lat": 37.373500, "lon": 127.324942, "name": "초월TG(진출)·초월 요금소(진입)",
            "note": "경기광주JC 옆. 광주원주→성남이천로 진출 램프(7.2m), 오른쪽 흰 건물 37.3731923,127.3256632 = 성남이천로→광주원주 진입 램프(4.4m, 사용자 항공사진 확인). 중부→광주원주 JC 램프는 뺌"},
    "226": {"links": ["2760464700", "2760464600"], "lat": 36.2795347, "lon": 127.6730830, "name": "금강휴게소 하이패스",
            "note": "휴게소에서 경부고속도로(금강2교 방향)로 나가는 램프(0.9m) + 반대 방향 램프(15m, 사용자가 반대쪽도 있다고 함). 사용자 좌표 2026-10-16"},
    "758": {"links": [], "lat": 35.2765681, "lon": 128.7170201, "name": "진영휴게소 하이패스",
            "note": "요금소(35.2765681,128.7170201)는 고속도로에서 휴게소로 들어온 차가 나가는 일방통행 1차로 길 위이고, 지나면 진영분기점교·오척천교 쪽 분기점 램프로 합류함(사용자 확인 2026-10-16). MOCT에는 분기점 램프 3851176701만 있고 휴게소를 거치는 이 길은 없음. 램프 전체에 붙이면 휴게소 안 들른 차에도 요금이 붙어서 안 붙임"},
    "657": {"links": ['2500180600'], "lat": 37.7573782, "lon": 127.7417815, "name": "동산 요금소",
            "note": "서울양양 본선 양양(조양IC) 방향 링크만. 요금소가 양양 방향에만 있음(사용자 확인). 통행기록(TCS_18)도 657은 남양주·화도·남춘천에서 들어와 춘천·동홍천·양양으로 나가는 동쪽 방향 기록뿐이고 서쪽 방향 기록은 없음"},
    "668": {"links": ['2510716000'], "lat": 37.3888806, "lon": 127.8852067, "name": "신평JC 요금소",
            "note": "광주원주→중앙 램프 하나(사용자 좌표 3.6m). 반대 방향 램프에는 요금소 없음(사용자 확인 2026-10-16). TCS에 양방향이 있는 건 원톨링 가상 경계로 봄"},
    "600": {"links": ["3580206401", "3580056902"], "lat": 36.3661245, "lon": 128.2393881, "name": "낙동JC 요금소",
            "note": "TCS: 600이 도로공사·상주영천(45) 요금 모두에서 출발·도착 = 양방향. 중부내륙→당진영덕 램프(0.5m)와 당진영덕→중부내륙 램프(2.3m) 둘 다"},
    "770": {"links": ['1961195401'], "lat": 35.4992746, "lon": 129.2221240, "name": "울주(동)톨게이트",
            "note": "함양울산 본선 동쪽(울주JC) 방향 링크(사용자 좌표 2.5m). TCS: 770은 부산울산(08) 요금의 출발 = 함양울산에서 울산포항으로 가는 차"},
    "780": {"links": ['1961195501'], "lat": 35.4995512, "lon": 129.2223277, "name": "울주(서)톨게이트",
            "note": "함양울산 본선 서쪽 방향 링크(사용자 좌표 14.2m). TCS: 780은 부산울산(08) 요금의 도착 = 울산포항에서 함양울산으로 온 차"},
    "750": {"links": ["1450595900"], "lat": 35.2823319, "lon": 129.2123570, "name": "기장동분기점톨게이트",
            "note": "부산외곽순환 본선 동쪽(기장JC, 종점) 방향 링크. TCS: 750은 부산울산(08) 요금의 출발 = 부산외곽에서 울산포항으로 가는 차. 사용자 좌표 0.6m"},
    "760": {"links": ["1450596200"], "lat": 35.2812516, "lon": 129.2201926, "name": "기장분기점 요금소",
            "note": "울산포항→부산외곽순환 램프. TCS: 760은 부산울산(08) 요금의 도착 = 울산포항에서 부산외곽으로 온 차. 사용자 좌표 1.0m"},
    # MOCT 요금소 노드에 램프·본선이 같이 닿던 곳: 사용자가 2026-10-16 지도에서 본 요금소 좌표에 가장 가까운 링크(같은 종류)만 남김
    "820": {"links": ["2180384401", "2180383601"], "lat": 37.6296413, "lon": 126.8521904, "name": "고양톨게이트(서울문산 본선)",
            "note": "서울문산 본선 링크(5.5m·6.6m). 사용자 좌표. 821 고양지선은 같은 자리 램프로 추정"},
    "821": {"links": ['2180384901', '2180384601'], "lat": 37.6316798, "lon": 126.8512101, "name": "고양 지선 요금소(작은 구조물)",
            "note": "고양톨게이트 북쪽 230m 작은 구조물(37.6316798,126.8512101). 국가관심지점 POI에 '고양톨게이트'(요금소)로 27m에 있음 = 요금소 맞음. 그 옆 서울문산선 램프 2개(11.7m·12.9m)에 붙임. 지선 요금소로 봄"},
    "227": {"links": ["3070049206", "3070049306"], "lat": 36.0028094, "lon": 126.7903929, "name": "군산톨게이트",
            "note": "요금소 좌표에서 3.8m·8.2m인 링크 두 개(군산IC 램프 양방향). 사용자 확인"},
    "742": {"links": ["2430291401", "2430291501"], "lat": 37.1078528, "lon": 127.1189357, "name": "남사진위톨게이트",
            "note": "경부선 램프 두 개(10.8m·13.3m). 사용자 확인"},
    "202": {"links": ["3690020700", "3690020800"], "lat": 36.0479767, "lon": 128.5146044, "name": "다부톨게이트",
            "note": "중앙선 램프 두 개(5.3m·6.7m). 사용자 확인"},
    "048": {"links": ["1870494000", "1870494100"], "lat": 36.3743762, "lon": 127.4037609, "name": "대화톨게이트",
            "note": "천변도시고속도로 본선 두 방향(3.2m·8.8m). 사용자 확인"},
    "816": {"links": ['2334310520'], "lat": 37.1880072, "lon": 126.7877873, "name": "마도(동)톨게이트(아래)",
            "note": "수도권제2순환 본선 아래쪽 방향 링크(사용자 좌표 37.1880072,126.7877873 에서 1.3m)"},
    "806": {"links": ['2334310531'], "lat": 37.1881892, "lon": 126.7883788, "name": "마도(서)톨게이트(위)",
            "note": "수도권제2순환 본선 위쪽 방향 링크(사용자 좌표 37.1881892,126.7883788 에서 0.4m)"},
    "805": {"links": ['2334310571', '2334310561', '2334310560', '2334310570'], "lat": 37.1881228, "lon": 126.7880466, "name": "마도톨게이트(가운데)",
            "note": "국가관심지점 POI에 '마도요금소' 8점이 이 자리(37.1881228,126.7880466) 25m 안에 있음. 통행기록(TCS_18)에 805가 마도동·마도서와 따로 잡혀 마도IC 드나드는 차의 요금소. 그 자리에서 갈라지는 램프 4개가 실제 도로임(사용자 확인 2026-10-16)"},
    "038": {"links": ["2220045504", "2220045401"], "lat": 37.6522544, "lon": 127.1220488, "name": "불암산톨게이트",
            "note": "수도권제1순환 본선 두 방향(7.9m·10.1m). 사용자 확인"},
    "084": {"links": ["2450078200", "2450078300"], "lat": 37.3276697, "lon": 127.0584171, "name": "서수지톨게이트",
            "note": "용인서울 본선 두 방향(2.7m·9.0m). 사용자 확인"},
    "097": {"links": ['1730625700', '1730627100'], "lat": 37.5697958, "lon": 126.6681265, "name": "인천공항톨게이트(신공항TG)",
            "note": "인천공항고속도로 본선 두 방향. 요금소가 모든 차선을 덮음(사용자 확인 2026-10-16). 사용자 좌표 37.5697958,126.6681265"},
    "698": {"links": ["4180204100", "4180206000", "4180208100"], "lat": 35.1376925, "lon": 128.7762441, "name": "진해톨게이트(램프)",
            "note": "진해산업로에서 들어오는 램프(35.1376925,128.7762441, 7m)와 진해산업로로 나가는 램프(35.1389596,128.7751745, 8.7m·8.9m). 사용자 확인"},
    "699": {"links": ["4180183903", "4180184000", "4180206400", "4180206300"], "lat": 35.1389661, "lon": 128.7756638, "name": "진해톨게이트(본선)",
            "note": "남해고속도로제3지선 본선 두 방향(6~9m). 사용자 확인"},
    "777": {"links": ["1510620701", "1510620801"], "lat": 35.9345081, "lon": 128.6383444, "name": "파군재톨게이트",
            "note": "대구외곽순환 램프 두 개(3.2m·4.2m). 사용자 확인"},
    "852": {"links": ["2141054600", "2141053800", "2141054700", "2141054800"], "lat": 36.9249554, "lon": 126.9194443, "name": "평택호톨게이트(하이패스전용) 위·아래",
            "note": "권관교차로 위(36.9249554,126.9194443) 램프 2개와 아래(36.9236470,126.9192176) 램프 2개. 사용자 확인"},
    "500": {"links": ['2370189700', '2370190000'], "lat": 37.251294, "lon": 127.592879, "name": "남여주TG",
            "note": "남여주 요금소 자리(6.5m)의 링크 2개. 요금소 하나(사용자 확인)"},
    "149": {"links": ['3851254500', '3850592700', '3851254600', '3850592600'], "lat": 35.223533, "lon": 128.899614, "name": "동김해TG",
            "note": "동김해 요금소 자리(2~18m)의 링크 4개. 요금소 하나(사용자 확인)"},
    "828": {"links": ['2290653501', '2290653401'], "lat": 37.825057, "lon": 126.779874, "name": "산단TG",
            "note": "산단 요금소 자리(37.8255998,126.7794460)에서 11m 안 링크 2개. 요금소 하나(사용자 확인)"},
    "071": {"links": ['2220254900', '2220253301', '2220254000', '2220253900', '2220255001', '2220253200'], "lat": 37.614127, "lon": 127.205295, "name": "이패TG",
            "note": "이패 요금소 자리(3~9m)의 링크 6개(수석호평도시고속도로 본선 요금소). 요금소 하나(사용자 확인)"},
    "047": {"links": ['1870031700', '1870031800', '1870032500', '1870032400'], "lat": 36.364183, "lon": 127.398713, "name": "한밭TG",
            "note": "천변도시고속도로 본선 두 방향. 요금소가 모든 차선을 덮음(사용자 확인 2026-10-16). 한밭 요금소 36.3643786,127.3987284"},
    "648": {"same_as": "647", "note": "도로공사 영업소 위치정보 LOD(2019)에 648 해운대 35.216727,129.205848, 647 해운대송정 35.216388,129.206388 로 같은 요금소 자리. MOCT 해운대TG 노드를 647과 같이 씀"},
    "740": {"nodes": ["2220309400"], "note": "화도상분기점톨게이트 37.6294591,127.3182634 는 이 노드의 본선 링크 2220779600 위(1.0m). 이 본선을 따라가면 서울양양행 램프가 갈라짐. TCS: 740은 도로공사 요금의 도착 = 수도권제2순환에서 서울양양으로 나가는 차"},
    "790": {"nodes": ["2220309800"], "note": "화도하분기점톨게이트 37.6294428,127.3179743 는 이 노드의 본선 링크 2220779400 위(0.2m). 이 본선 바로 뒤에서 서울양양 램프가 합류. TCS: 790은 서울춘천(10) 요금의 도착 = 서울양양에서 수도권제2순환으로 온 차"},
}
# 영업소명과 다른 이름의 국가 POI 요금소. 근거는 POI 주소와 도로 이름이다.
ALIAS = {
    "052": (["천마"], "천마산터널의 요금소는 국가 POI 천마요금소(부산 서구 암남동)"),
    "090": (["감만", "영도"], "부산항대교의 요금소는 국가 POI 감만요금소·영도요금소(남구 감만동 254-16)"),
    "094": (["광안대교"], "광안대로의 요금소는 국가 POI 광안대교요금소(해운대구 재송동)"),
    "267": (["주암"], "주암송광사의 요금소는 국가 POI 주암요금소(순천시 주암면 요곡리). 도로공사 API 좌표에서 39m"),
}
# 도로공사 API 좌표를 브이월드 위성사진으로 확인한 영업소. 국가 POI와 MOCT 요금소 노드가 없어 이 좌표를 요금 지점으로 쓴다.
USER_API = {
    "735": "남양평 하이패스IC 램프 위",
    "741": "평택고덕 요금소 사무실 옆 램프",
    "331": "영일만 본선 요금소 사무실 옆 본선",
    "763": "두물머리 하이패스IC 램프 위",
}
# 같은 이름의 국가 POI 요금소가 여러 곳인 영업소: 이 좌표에서 300m 이내의 POI 요금소를 참조 좌표로 쓴다. 근거는 POI 주소이다.
USER_POI = {
    "052": (35.079987, 129.022296, "국가 POI 천마요금소(부산 서구 암남동). 인천 부평구 천마요금소는 다른 도로"),
    "033": (37.657237, 126.822152, "국가 POI 고양요금소(덕양구 주교동)가 수도권제1순환 고양TG. 성사동 고양톨게이트는 820 고양본선"),
    "035": (37.679471, 126.918086, "국가 POI 양주요금소(장흥면 삼하리)가 수도권제1순환 양주(개). 회암동 양주요금소는 879 양주"),
    "879": (37.840867, 127.08168, "국가 POI 양주요금소(양주시 회암동)가 구리포천 양주. 장흥면 양주요금소는 035 양주(개)"),
}
# 영업소명과 이름이 다른 MOCT 요금소 노드를 직접 지정한 영업소
USER_NODE = {
    "021": (["2000022802", "2000022803"], "장안요금소 노드. 국가 POI 장안요금소(수원시 장안구 송죽동)와 같은 자리"),
    "082": (["2040022101", "2040022102"], "금토TG 노드. 085 금토와 같은 요금소이며 도로공사 API 좌표는 요금소 사무실(86m)"),
    "027": (["1940018101", "1940018102"], "예전TG 노드. 애전 영업소의 요금소 노드가 예전TG로 표기됨"),
    "081": (["1140109702", "1140109703"], "신월요금소 노드. 국가 POI 신월여의지하도로요금소와 같은 자리"),
    "681": (["2330152401", "2330152402"], "장안TG 노드(화성시 장안면, 평택시흥고속도로). 부산 장안읍과 수원 장안구의 장안 요금소는 다른 영업소"),
    "729": (["2300247800"], "신둔하이패스IC 노드"),
    "754": (["3850325001"], "김해가야하이패스IC 노드"),
    "756": (["3850368402"], "한림하이패스IC 노드"),
    "868": (["2980124601", "2980124602"], "예산예당호휴게소하이패스TG 노드"),
    "895": (["2360327600", "2360327700"], "내촌ICTG 노드. 국가 POI 내촌요금소(포천시 내촌면)와 같은 자리. 홍천군 내촌면 내촌요금소는 서울양양선"),
    "746": (["1450248100"], "기장일광TG(서측) 노드. 도로공사 API 노선이 부산외곽순환선이고 부산외곽순환고속도로 램프 위의 노드는 이것뿐"),
    "751": (["1450248100"], "기장일광TG(서측) 노드. 746 기장일광입구와 같은 램프 요금소"),
    "646": (["1450023501", "1450023502", "1450248400"], "기장일광TG·기장일광TG(동측) 노드. 울산포항고속도로 쪽 요금소. 서측 노드는 746·751"),
}
NOISE = re.compile(r"앞|정류장|사거리|삼거리|교차로|택배|회차|은행|보험|타이어|주차|물류센터")


def node_base(n):
    n = re.sub(r"\(.*?\)", "", n.replace(" ", ""))
    n = re.split(r"TG|요금소|톨게이트", n)[0]
    return re.sub(r"^한국도로공사", "", n.replace("하이패스", ""))


def poi_base(n):
    """국가 POI 이름에서 괄호, 한국도로공사, 요금징수소·톨게이트·요금소·영업소·TG, 하이패스를 제거한다."""
    n = re.sub(r"\(.*?\)", "", n.replace(" ", "")).replace("JCT", "JC").replace("분기점", "JC").replace("T/G", "TG")
    n = re.sub(r"^한국도로공사", "", n)
    n = re.sub(r"(요금징수소|톨게이트|요금소|영업소|TG)+$", "", n)
    return n.replace("하이패스", "")


def unit_variants(name):
    n0 = name.replace(" ", "").replace("JCT", "JC").replace("하이패스", "")
    out = [re.sub(r"[()]", "", n0)]  # 괄호 안을 이름에 이어 쓴 형태: 불갑산(서울)->불갑산서울, 오포(세종)->오포세종
    n = re.sub(r"\(.*?\)", "", n0)
    out.append(n)
    n = re.sub(r"(본선|지선)$", "", n)
    out.append(n)
    m = re.fullmatch(r"(.+)JC(남|북|동|서)", n)  # 고양JC남 -> 고양남JC, 고양남
    if m:
        out += [m.group(1) + m.group(2) + "JC", m.group(1) + m.group(2)]
    n = re.sub(r"(상|하|입구|출구|동|서|남|북|\d)(?=JC$)", "", n)
    out.append(n)
    out.append(re.sub(r"(상|하|입구|출구|동|서|남|북|\d)$", "", n))
    out.append(re.sub(r"(터널|대교|지하도로)$", "", out[-1]))
    seen, res = set(), []
    for v in out:
        if len(v) >= 2 and v not in seen:
            seen.add(v)
            res.append(v)
    return res


def fetch_unit_xy(key):
    """도로공사 영업소 위치정보 API에서 좌표가 있는 영업소를 읽는다. 한 페이지 최대 99건이다."""
    pages, page = [], 1
    while True:
        d = get_json(f"https://data.ex.co.kr/openapi/locationinfo/locationinfoUnit?key={key}&type=json&numOfRows=99&pageNo={page}",
                     secrets=(key,), timeout=120)
        pages.append(pd.DataFrame(d.get("list", []), columns=["unitCode", "xValue", "yValue"], dtype=object))
        if page >= int(d.get("pageSize") or 1):
            break
        page += 1
    df = pd.concat(pages).fillna("")
    df = df[(df.xValue.str.strip() != "") & (df.yValue.str.strip() != "")]
    df = df.assign(unitCode=df.unitCode.str.strip()).drop_duplicates("unitCode", keep="last")
    return {c: (float(y), float(x)) for c, x, y in zip(df.unitCode, df.xValue, df.yValue)}


def group_points(pts, radius):
    """(x, y) 목록을 radius 이내로 이어진 것끼리 묶는다."""
    groups = []
    for i, (x, y) in enumerate(pts):
        hit = [g for g in groups if any(math.hypot(x - pts[j][0], y - pts[j][1]) <= radius for j in g)]
        for g in hit:
            groups.remove(g)
        groups.append([i] + [j for g in hit for j in g])
    return groups


def main():
    units = read_csv(TOLLROAD_DIR / "ETC_75_04_01_168858.csv")
    units = units[(units.가동영업소여부.str.strip() == "Y") & (units.가상영업소여부.str.strip() == "N")]
    api_xy = fetch_unit_xy(env("EX_ROAD_API_KEY"))
    print("도로공사 위치정보 API 좌표 있는 영업소", len(api_xy))

    # 국가 POI 요금소. 쉼표로 이어진 이름은 각각 비교한다
    poi = read_csv(TOLLROAD_DIR / "API_수집" / "국가관심지점_요금소.csv")
    poi["xy"] = [wgs84_to_tm(float(a), float(b)) for a, b in zip(poi.lat, poi.lon)]
    poi["bases"] = [{poi_base(p) for p in n.split(",")} - {""} for n in poi.name]
    poi_by_base = defaultdict(list)
    for i, bs in enumerate(poi.bases):
        for b in bs:
            poi_by_base[b].append(i)
    print("국가 POI 요금소", len(poi), "이름 종류", len(poi_by_base))

    def poi_match(code, name):
        """영업소명과 같은 이름의 POI 행 번호와 일치 방식. 같은 이름이 없으면 한쪽이 다른 쪽을 포함하는 이름이 하나뿐일 때 그 POI"""
        vs = unit_variants(name)
        if code in ALIAS:
            return [i for b in ALIAS[code][0] for i in poi_by_base.get(b, [])], "이름표"
        for v in vs:
            if v in poi_by_base:
                return poi_by_base[v], "이름"
        for v in vs:
            found = [b for b in poi_by_base if len(v) >= 2 and v in b]
            if found:
                return [i for b in found for i in poi_by_base[b]], "이름 포함"
        return [], ""

    # 요금소 노드
    all_nodes = read_nodes(["NODE_ID", "NODE_NAME", "NODE_TYPE"])
    all_nodes = all_nodes[all_nodes.geometry.notna()]
    node_xy = {i: (n, g.x, g.y) for i, n, g in zip(all_nodes.NODE_ID, all_nodes.NODE_NAME, all_nodes.geometry)}
    nodes = all_nodes[all_nodes.NODE_NAME.str.contains("TG|요금소|톨게이트") & ~all_nodes.NODE_NAME.str.contains(NOISE)]
    nodes = nodes.reset_index(drop=True)
    nodes["base"] = nodes.NODE_NAME.map(node_base)
    X, Y = nodes.geometry.x.to_numpy(), nodes.geometry.y.to_numpy()
    ids, names = list(nodes.NODE_ID), list(nodes.NODE_NAME)
    print("요금소 노드", len(nodes))

    # 이름별 요금소. 같은 이름의 노드가 CLUSTER_M 이내에 있으면 한 요금소로 병합한다
    clusters = {}
    for base, idxs in nodes.groupby("base", sort=False).indices.items():
        clusters[base] = [[idxs[k] for k in g] for g in group_points([(X[i], Y[i]) for i in idxs], CLUSTER_M)]

    def near_nodes(p, r):
        d = np.hypot(X - p[0], Y - p[1])
        idx = np.flatnonzero(d <= r)
        return [(d[i], i) for i in idx[np.lexsort((idx, d[idx]))]]

    def cluster_of(i):
        return next(g for g in clusters[nodes.base[i]] if i in g)

    def cluster_dist(g, p):
        return min(math.hypot(p[0] - X[j], p[1] - Y[j]) for j in g)

    rows, stat = [], Counter()
    refs_of = {}   # 영업소코드 -> [(x, y, 출처, 이름)]
    for u in units.itertuples():
        code, name, op = u.영업소코드.strip(), u.영업소명, u.고속도로운영기관구분코드.strip()
        base = {"영업소코드": code, "영업소명": name, "운영기관코드": op}
        if code in EXCLUDE:
            rows.append({**base, "기준": "제외", "비고": EXCLUDE[code]})
            stat["제외"] += 1
            continue

        # 참조 좌표: 도로공사 API 좌표와 같은 이름의 국가 POI. POI 요금소가 여러 곳이면 API 좌표 PICK_M 이내의 요금소만 남긴다
        refs, note = [], ""
        api = api_xy.get(code)
        if api:
            refs.append((*wgs84_to_tm(*api), "도로공사", f"도로공사 API {code}"))
        pidx, how_poi = poi_match(code, name)
        if code in ALIAS:
            note = ALIAS[code][1]
        poi_groups = [[pidx[k] for k in g] for g in group_points([poi.xy[i] for i in pidx], CLUSTER_M)]
        if len(poi_groups) > 1:
            anchor = wgs84_to_tm(*USER_POI[code][:2]) if code in USER_POI else (refs[0][:2] if api else None)
            picked = [g for g in poi_groups if anchor and min(math.hypot(poi.xy[i][0] - anchor[0], poi.xy[i][1] - anchor[1]) for i in g) <= PICK_M]
            if len(picked) == 1:
                poi_groups = picked
                if code in USER_POI:
                    note = USER_POI[code][2]
            else:
                note = f"같은 이름의 국가 POI 요금소 {len(poi_groups)}곳: " + "; ".join(
                    "%s(%.5f,%.5f)" % (poi.name[g[0]], float(poi.lat[g[0]]), float(poi.lon[g[0]])) for g in poi_groups)
                poi_groups = []
        for g in poi_groups:
            for i in g:
                refs.append((*poi.xy[i], "국가POI", poi.name[i]))
        refs_of[code] = refs

        chosen, how = None, ""
        for v in unit_variants(name):
            groups = clusters.get(v)
            if not groups:
                continue
            if len(groups) == 1:
                chosen, how = groups[0], "MOCT노드(이름)"
            else:
                picks = []
                for src in ("도로공사", "국가POI"):
                    rs = [r for r in refs if r[2] == src]
                    picks = [g for g in groups if rs and min(cluster_dist(g, r[:2]) for r in rs) <= PICK_M]
                    if len(picks) == 1:
                        break
                side = re.search(r"(남|북|동|서)(?=JC$|$)", re.sub(r"\(.*?\)", "", name.replace("JCT", "JC")))
                by_side = False
                if len(picks) != 1 and len(groups) == 2 and side:
                    # 요금소가 두 곳이고 영업소명에 방위가 있으면 그 방위의 요금소
                    axis = 1 if side.group(1) in ("남", "북") else 0
                    order = sorted(groups, key=lambda g: sum((X, Y)[axis][j] for j in g) / len(g))
                    picks, by_side = [order[1] if side.group(1) in ("북", "동") else order[0]], True
                if len(picks) == 1:
                    chosen, how = picks[0], "MOCT노드(이름, 동명은 방위로 선택)" if by_side else "MOCT노드(이름, 동명은 참조 좌표로 선택)"
                else:
                    note = (note + "; " if note else "") + f"같은 이름의 요금소 노드 {len(groups)}곳: " + "; ".join(
                        "%s(%.5f,%.5f)" % ((names[g[0]],) + tm_to_wgs84(X[g[0]], Y[g[0]])) for g in groups)
                    how = "동명 미해결"
            break
        if chosen is None and how != "동명 미해결":
            for r in sorted(refs, key=lambda r: r[2] != "국가POI"):
                nn = near_nodes(r[:2], ADOPT_M)
                if nn:
                    chosen, how = cluster_of(nn[0][1]), "MOCT노드(참조위치)"
                    break

        if chosen is not None:
            lat, lon = tm_to_wgs84(sum(X[j] for j in chosen) / len(chosen), sum(Y[j] for j in chosen) / len(chosen))
            row = {**base, "기준": how, "NODE_IDS": ";".join(sorted(ids[j] for j in chosen)),
                   "노드명": ";".join(sorted({names[j] for j in chosen})), "위도": round(lat, 6), "경도": round(lon, 6), "비고": note}
            if refs:
                d, r = min(((cluster_dist(chosen, r[:2]), r) for r in refs), key=lambda t: t[0])
                rlat, rlon = tm_to_wgs84(r[0], r[1])
                row.update({"참조출처": r[2], "참조명": r[3], "참조위도": round(rlat, 6), "참조경도": round(rlon, 6), "참조거리m": round(d),
                            "검증": "일치" if d <= 100 else "근접" if d <= 300 else "불일치"})
            else:
                row["검증"] = "참조없음"
            rows.append(row)
        elif poi_groups:
            # 한 요금소의 POI 지점을 POINT_M 이내로 묶어 지점마다 행을 둔다
            pts = [poi.xy[i] for g in poi_groups for i in g]
            pidx_all = [i for g in poi_groups for i in g]
            for g in group_points(pts, POINT_M):
                lat, lon = tm_to_wgs84(sum(pts[k][0] for k in g) / len(g), sum(pts[k][1] for k in g) / len(g))
                pname = ";".join(sorted({poi.name[pidx_all[k]] for k in g}))
                rows.append({**base, "기준": "국가POI", "위도": round(lat, 6), "경도": round(lon, 6),
                             "참조출처": "국가POI", "참조명": pname, "참조위도": round(lat, 6), "참조경도": round(lon, 6),
                             "검증": "MOCT 요금소 노드 없음", "비고": (note + "; " if note else "") + f"국가 POI {how_poi} 일치 {len(g)}점"})
            how = "국가POI"
        elif api and code in USER_API:
            rows.append({**base, "기준": "도로공사", "위도": api[0], "경도": api[1], "참조출처": "도로공사", "참조명": f"도로공사 API {code}",
                         "참조위도": api[0], "참조경도": api[1], "검증": "위성사진 확인", "비고": "도로공사 API 좌표를 브이월드 위성사진으로 확인: " + USER_API[code]})
            how = "도로공사"
        else:
            if api and how != "동명 미해결":
                note = (note + "; " if note else "") + "도로공사 API 좌표만 있음"
            rows.append({**base, "기준": "없음", "검증": "위치 없음", "비고": note})
            how = how or "없음"
        stat[how] += 1
        if how not in ("동명 미해결",) and rows[-1].get("검증") not in ("일치", None, "MOCT 요금소 노드 없음"):
            stat["검증:" + rows[-1]["검증"]] += 1

    # 한 요금소의 노드를 여러 영업소가 공유하는 경우
    #  - 운영기관이 다르면 같은 요금소일 수 없으므로 각 영업소는 자기 참조 좌표 SPLIT_M 이내의 노드만 갖는다.
    #  - 그래도 노드가 없거나, 같은 운영기관인데 한쪽은 일치하고 한쪽은 불일치이면 불일치 쪽은 국가 POI 좌표로 되돌리고 POI가 없으면 '없음'이다.
    fixed = set(USER_CHECK) | set(USER_FIX) | set(USER_NODE)
    claims = defaultdict(list)
    for r in rows:
        if r.get("NODE_IDS") and r["영업소코드"] not in fixed:
            claims[r["NODE_IDS"]].append(r)
    nid_idx = {n: i for i, n in enumerate(ids)}
    for key, rs in claims.items():
        if len(rs) < 2:
            continue
        others = lambda r: ", ".join(f"{x['영업소코드']} {x['영업소명']}" for x in rs if x is not r)
        multi_op = len({r["운영기관코드"] for r in rs}) > 1
        for r in rs:
            kp = wgs84_to_tm(float(r["참조위도"]), float(r["참조경도"])) if r.get("참조위도") else None
            keep = None
            if multi_op and kp:
                keep = [n for n in key.split(";") if math.hypot(X[nid_idx[n]] - kp[0], Y[nid_idx[n]] - kp[1]) <= SPLIT_M]
            if keep:
                r["NODE_IDS"] = ";".join(keep)
                r["노드명"] = ";".join(sorted({names[nid_idx[n]] for n in keep}))
                r["비고"] = (r["비고"] + "; " if r["비고"] else "") + f"다른 운영기관 영업소({others(r)})와 같은 요금소라 참조 좌표 {SPLIT_M:.0f}m 이내 노드만"
                continue
            bad = r["검증"] == "불일치" and any(x["검증"] in ("일치", "근접") for x in rs if x is not r)
            if multi_op or bad:
                note = f"이름이 같은 MOCT 노드({r['노드명']})는 {others(r)} 위치라서 제외"
                pois = [x for x in refs_of.get(r["영업소코드"], []) if x[2] == "국가POI"]
                if pois:
                    lat, lon = tm_to_wgs84(sum(p[0] for p in pois) / len(pois), sum(p[1] for p in pois) / len(pois))
                    r.update({"기준": "국가POI(동명 MOCT 노드 제외)", "NODE_IDS": "", "노드명": "", "위도": round(lat, 6), "경도": round(lon, 6),
                              "참조출처": "국가POI", "참조명": ";".join(sorted({p[3] for p in pois})), "참조위도": round(lat, 6), "참조경도": round(lon, 6),
                              "참조거리m": "", "검증": "MOCT 요금소 노드 없음", "비고": (r["비고"] + "; " if r["비고"] else "") + note + ", 국가 POI 좌표로"})
                else:
                    r.update({"기준": "없음", "NODE_IDS": "", "노드명": "", "위도": "", "경도": "", "참조거리m": "",
                              "검증": "위치 없음", "비고": (r["비고"] + "; " if r["비고"] else "") + note + ", 국가 POI도 없음"})
                stat["되돌림"] += 1
            else:
                r["비고"] = (r["비고"] + "; " if r["비고"] else "") + f"같은 요금소를 {others(r)}와 같이 씀"

    # 이름이 다른 요금소 노드를 직접 지정한 영업소
    for r in rows:
        code = r["영업소코드"]
        if code in USER_NODE:
            nids, note = USER_NODE[code]
            lat, lon = tm_to_wgs84(sum(node_xy[n][1] for n in nids) / len(nids), sum(node_xy[n][2] for n in nids) / len(nids))
            for k in ("NODE_IDS", "LINK_IDS", "참조출처", "참조명", "참조위도", "참조경도", "참조거리m"):
                r.pop(k, None)
            r.update({"기준": "MOCT노드(지정)", "NODE_IDS": ";".join(nids), "노드명": ";".join(sorted({node_xy[n][0] for n in nids})),
                      "위도": round(lat, 6), "경도": round(lon, 6), "검증": "노드 지정", "비고": note})
            stat["노드 지정"] += 1

    # 사용자가 지도에서 확인한 결과 반영
    out = []
    for r in rows:
        code = r["영업소코드"]
        if code not in USER_CHECK:
            out.append(r)
            continue
        how, note = USER_CHECK[code]
        base = {k: r[k] for k in ("영업소코드", "영업소명", "운영기관코드")}
        if how in ("MOCT", "둘다"):
            out.append({**r, "검증": "사용자 확인(MOCT)", "비고": note})
        if how in ("POI", "둘다"):
            pois = [x for x in refs_of.get(code, []) if x[2] == "국가POI"]
            for g in group_points([p[:2] for p in pois], POINT_M):
                lat, lon = tm_to_wgs84(sum(pois[k][0] for k in g) / len(g), sum(pois[k][1] for k in g) / len(g))
                out.append({**base, "기준": "국가POI(사용자 확인)" if how == "POI" else "국가POI(사용자 확인, MOCT와 같이)",
                            "위도": round(lat, 6), "경도": round(lon, 6), "참조출처": "국가POI", "참조명": ";".join(sorted({pois[k][3] for k in g})),
                            "참조위도": round(lat, 6), "참조경도": round(lon, 6), "검증": "사용자 확인(국가POI)", "비고": note})
        if how == "없음":
            out.append({**base, "기준": "없음", "검증": "위치 없음", "비고": note})
        stat["사용자확인:" + how] += 1
    rows = out

    # 분기점 출구·입구 영업소는 링크나 노드를 직접 지정한다
    out = []
    for r in rows:
        code = r["영업소코드"]
        if code not in USER_FIX:
            out.append(r)
            continue
        f = USER_FIX[code]
        base = {k: r[k] for k in ("영업소코드", "영업소명", "운영기관코드")}
        if "same_as" in f:  # 다른 영업소와 같은 요금소
            src = next(x for x in rows if x["영업소코드"] == f["same_as"] and x.get("NODE_IDS"))
            f = {"nodes": src["NODE_IDS"].split(";"), "note": f["note"]}
            r = {**r, "노드명": src["노드명"], "위도": src["위도"], "경도": src["경도"]}
        if "nodes" in f:
            out.append({**base, "기준": "MOCT노드(사용자 확인, 방향 구분)", "NODE_IDS": ";".join(f["nodes"]), "노드명": r.get("노드명", ""),
                        "위도": r.get("위도", ""), "경도": r.get("경도", ""), "검증": "사용자 확인(MOCT)", "비고": f["note"]})
        elif f["links"]:
            out.append({**base, "기준": "링크지정(사용자 확인, 방향 구분)", "LINK_IDS": ";".join(f["links"]), "위도": f["lat"], "경도": f["lon"],
                        "참조명": f["name"], "검증": "사용자 확인(링크)", "비고": f["note"]})
        else:
            out.append({**base, "기준": "없음", "참조명": f["name"], "검증": "위치 없음", "비고": f["note"] + ". 공공 자료로는 요금 지점을 정할 수 없음"})
    df = pd.DataFrame(out, dtype=object)
    # 같은 코드가 여러 행이던 직접 지정 영업소는 한 행으로 둔다
    df = df[~(df.영업소코드.isin(USER_FIX.keys()) & df.영업소코드.duplicated())].reset_index(drop=True)

    cols = ["영업소코드", "영업소명", "운영기관코드", "기준", "NODE_IDS", "LINK_IDS", "노드명", "위도", "경도",
            "참조출처", "참조명", "참조위도", "참조경도", "참조거리m", "검증", "비고"]
    path = TOLLROAD_DIR / "영업소_요금지점.csv"
    write_csv(path, df, cols)
    print("영업소", len(units), dict(stat))
    print("기준별 행", df.기준.value_counts().to_dict())
    print("저장:", path)


if __name__ == "__main__":
    main()

"""교통사고다발지역 CSV 14개의 건수, 연도, 중복과 Riskarea 좌표계를 확인한다.

python -X utf8 scripts/accident/stats.py
"""
import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, read_csv

DIR = ROOT / "maps" / "전국교통사고다발지역표준데이터"
FILES = ["12_25_bicycle", "12_25_oldman", "12_25_child", "12_25_schoolzone", "13_25_freezing", "15_25_tmzon",
         "17_25_lg", "17_25_motorcycle", "17_25_violt", "19_25_drunk", "19_25_pedstrians", "19_25_truck"]
BASE_COLS = ["사고다발지fid", "사고다발지id", "법정동코드", "지점코드", "시도시군구명", "지점명", "사고건수", "사상자수",
             "사망자수", "중상자수", "경상자수", "부상신고자수", "경도", "위도", "다발지역폴리곤"]


def enc(path):
    raw = path.read_bytes()
    try:
        raw.decode("utf-8-sig")
        return "utf-8-sig" if raw.startswith(b"\xef\xbb\xbf") else "utf-8"
    except UnicodeDecodeError:
        return "cp949"


def dist_m(lat1, lon1, lat2, lon2):
    lat = np.radians((lat1 + lat2) / 2)
    return np.hypot((lon1 - lon2) * 111320 * np.cos(lat), (lat1 - lat2) * 110950)


def main():
    for name in FILES:
        p = DIR / f"{name}.csv"
        df = read_csv(p)
        years = df.사고다발지id.str[:4].value_counts()
        latest = years.index.max()
        extra = [c for c in df.columns if c not in BASE_COLS]
        print(f"{name}: {len(df):,}건 {enc(p)} 배치 {years.index.min()}~{latest} 최신 배치 {years[latest]}건 사고건수 최소 {df.사고건수.astype(int).min()}",
              f"추가 컬럼 {extra}" if extra else "", df.법규위반.value_counts().head(2).to_dict() if "법규위반" in df else "")

    # 폴리곤 형식
    df = read_csv(DIR / "19_25_pedstrians.csv")
    g = json.loads(df.다발지역폴리곤[0])
    xs, ys = zip(*g["coordinates"][0])
    print("보행자 첫 행 폴리곤:", g["type"], "점", len(xs), "폭 약", round(dist_m(min(ys), min(xs), min(ys), max(xs))), "m")
    # 같은 지점코드가 여러 해에 걸쳐 같은 위치인지
    df["위도"], df["경도"] = df.위도.astype(float), df.경도.astype(float)
    multi = df.groupby("지점코드").filter(lambda x: len(x) >= 2).groupby("지점코드", sort=False)
    xy = multi.apply(lambda x: len(set(zip(x.위도, x.경도))), include_groups=False)
    first2 = multi.head(2).groupby("지점코드", sort=False)
    a, b = first2.nth(0).set_index("지점코드"), first2.nth(1).set_index("지점코드")
    d = dist_m(a.위도, a.경도, b.위도, b.경도)[xy > 1]
    print("보행자: 지점코드가 2행 이상", len(xy), "그중 좌표가 같은 것", (xy == 1).sum(), "좌표 차이 중앙값 약", round(d.median() / 1000, 1), "km")

    # 이전 표준 CSV: 관리번호 앞 4자리에서 1을 뺀 값이 사고연도인지
    std = read_csv(DIR / "전국교통사고다발지역표준데이터.csv")
    ok = (std.사고지역관리번호.str[:4].astype(int) - 1 == std.사고연도.astype(int)).sum()
    print("전국교통사고다발지역표준데이터.csv", f"{len(std):,}건, 관리번호 앞 4자리 − 1 = 사고연도인 행", f"{ok:,}",
          "유형", std.사고유형구분.value_counts(sort=False).to_dict(), "연도", std.사고연도.value_counts().sort_index().to_dict())
    print("  폴리곤 첫 행 앞부분:", std.사고다발지역폴리곤정보[0][:60])

    risk = read_csv(DIR / "Riskarea.csv")
    print("Riskarea.csv", f"{len(risk):,}건 연도", risk.연도코드.value_counts().sort_index().to_dict())
    xs, ys = risk.중심점utmkx좌표.astype(float), risk.중심점utmky좌표.astype(float)
    print("  중심점 x", f"{xs.min():,.0f}~{xs.max():,.0f}", "y", f"{ys.min():,.0f}~{ys.max():,.0f}")
    print("  폴리곤 앞부분:", risk.사고위험지역폴리곤[0][:50])
    print("  사고위험지역명 예:", risk.사고위험지역명[0])
    print("  사고분석유형명 예:", list(risk.사고분석유형명[:3]))
    print("  링크 ID 비슷한 컬럼:", [c for c in risk.columns if "link" in c.lower() or "링크" in c])


if __name__ == "__main__":
    main()

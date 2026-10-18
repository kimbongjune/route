"""행안부 과속방지턱정보.csv 원본의 분포와 문제점을 집계한다.

python -X utf8 scripts/speedbump/stats.py
"""
import sys
from pathlib import Path

import numpy as np
import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import SPEEDBUMP_DIR, read_csv

SRC = SPEEDBUMP_DIR / "행안부" / "과속방지턱정보.csv"
CODES = SPEEDBUMP_DIR / "행안부" / "개방자치단체코드_영업상태코드.xlsx"
LAT, LON = "WGS84위도", "WGS84경도"
SKIP = ("관리번호", "데이터갱신시점", "최종수정시점")  # 행마다 다를 수밖에 없는 컬럼
OLD_PREFIX = {"인천광역시": ["인천"], "전남광주통합특별시": ["전남", "광주"]}


def num(x):
    try:
        return float(x)
    except (TypeError, ValueError):
        return None


def coord_problem(df):
    la, lo = df[LAT].map(num).astype(float), df[LON].map(num).astype(float)
    return pd.Series(np.select(
        [la.isna() | lo.isna(), (la == 0) | (lo == 0), la.between(33, 39) & lo.between(124, 132), la.between(124, 132) & lo.between(33, 39)],
        ["숫자 아님", "0", "", "위도·경도 바뀜"], "엉뚱한 값"), index=df.index)


def main():
    df = read_csv(SRC)
    key_cols = [c for c in df.columns if c not in SKIP]
    print("행", f"{len(df):,}", "컬럼", len(df.columns))
    for col in ("과속방지턱형태구분", "도로유형구분", "규격여부", "보차분리여부", "연속형여부", "과속방지턱재료"):
        print(f"  {col}", df[col].value_counts().head(6).to_dict())

    # 좌표
    df["좌표문제"] = coord_problem(df)
    bad = df[df.좌표문제 != ""]
    has_addr = (bad.소재지도로명주소.str.strip() != "") | (bad.소재지지번주소.str.strip() != "")
    print("좌표 오류", f"{len(bad):,}", bad.좌표문제.value_counts(sort=False).to_dict(), "| 오류 행 중 주소 있음", has_addr.sum())
    by_xy = df.groupby([LAT, LON], sort=False)
    xy = by_xy.agg(행수=("도로명", "size"), 도로명수=("도로명", "nunique"), 문제=("좌표문제", "first"),
                   시도명=("시도명", "first"), 시군구명=("시군구명", "first"))
    rep = xy[(xy.도로명수 >= 4) & (xy.문제 == "")]
    big = rep.sort_values("행수", ascending=False, kind="stable").head(3)
    print("대표 좌표(같은 좌표에 도로명 4개 이상)", len(rep), "좌표", f"{rep.행수.sum():,}", "행 | 큰 곳",
          [(k, r.행수, r.시도명 + " " + r.시군구명) for k, r in big.iterrows()])
    print("고유 좌표", f"{len(xy):,}", "같은 좌표 중복 행(전체에서 고유 좌표 수를 제외한 것)", f"{len(df) - len(xy):,}")
    df["대표좌표"] = pd.MultiIndex.from_frame(df[[LAT, LON]]).isin(rep.index)

    # 통째로 같은 행
    dup = df[df.duplicated(key_cols, keep=False)]
    groups = dup.groupby(key_cols).ngroups
    print(f"{len(key_cols)}개 컬럼이 글자 그대로 같은 행", f"{len(dup):,}", "행", f"{groups:,}", "건 (각각 한 행만 남기면 중복 행",
          f"{len(dup) - groups:,}) | 많은 시군구", list(dup.시군구명.value_counts().head(3).items()))
    # 같은 좌표·주소인 행들 사이에서 어떤 컬럼이 달라지는지
    nu = df.groupby([LAT, LON, "소재지도로명주소", "소재지지번주소"], sort=False)[key_cols].nunique() > 1
    diff_col = nu.sum()
    only_date = (nu.sum(axis=1) == 1) & nu.데이터기준일자
    print("같은 좌표·주소인 행들 사이에서 값이 달라지는 컬럼(건수)", list(diff_col[diff_col > 0].sort_values(ascending=False, kind="stable").head(5).items()),
          "| 데이터기준일자만 다른 경우", f"{only_date.sum():,}")

    # 값 이상
    h, w, ln = (df[c].map(num).fillna(0) for c in ("과속방지턱높이", "과속방지턱폭", "과속방지턱연장"))
    print("높이 0", f"{(h == 0).sum():,}", f"({(h == 0).mean():.0%})", "최대 높이", h.max(), "폭 최대", w.max(), "연장 최대", ln.max(),
          "폭 0", f"{(w == 0).sum():,}", "연장 0", f"{(ln == 0).sum():,}")
    mm = df[df.과속방지턱재료 != df.과속방지턱형태구분]
    print("재료 컬럼과 형태 컬럼이 다른 행", f"{len(mm):,}", list(mm.groupby(["과속방지턱재료", "과속방지턱형태구분"]).size().sort_values(ascending=False).head(3).items()))
    date_no = df[df.과속방지턱관리번호.str.strip().str.fullmatch(r"(19|20)\d{6}")]
    print("과속방지턱관리번호가 8자리 날짜 형식", f"{len(date_no):,}", list(date_no.시군구명.value_counts().head(2).items()))
    nl = df[df[key_cols].apply(lambda s: s.str.contains("\n")).any(axis=1)]
    print("셀 안에 줄바꿈 있는 행", len(nl), "예:", repr(nl.관리기관명.iloc[0]) if len(nl) else "")
    road, parcel = df.소재지도로명주소.str.strip(), df.소재지지번주소.str.strip()
    addr = road.where(road != "", parcel)
    raw = df.소재지도로명주소.where(df.소재지도로명주소 != "", df.소재지지번주소)
    odd = [r for a, s, r in zip(addr, df.시도명, raw) if a and not a.startswith(s)]
    print("주소가 시도명으로 시작하지 않는 행", f"{len(odd):,}", "예:", [r[:20] for r in odd[:3]])
    print("빈값: 설치연도", f"{(df.과속방지턱설치연도.str.strip() == '').sum():,}", "도로명주소", f"{(road == '').sum():,}",
          "지번주소", f"{(parcel == '').sum():,}", "과속방지턱관리번호", f"{(df.과속방지턱관리번호.str.strip() == '').sum():,}")
    dates = df.데이터기준일자[df.데이터기준일자.str.strip() != ""]
    print("데이터기준일자 범위", dates.min(), "~", dates.max())

    # 지자체별 건수. 개방자치단체코드 표에서 '_ALL'인 시도 전체와 시도 본청 코드를 제외한 개별 지자체로 비교한다.
    # 표가 CSV보다 오래되어 전남광주통합특별시(이전 광주·전남 코드)와 인천 개편 구(영종구·제물포구)는 코드가 일치하지 않으므로
    # CSV에 있는데 표에 없는 코드를 사용하는 시도는 표의 이전 코드를 비교에서 제외한다.
    codes = pd.read_excel(CODES, sheet_name="1. 개방자치단체코드", dtype=str).iloc[:, 1:3].dropna()
    codes.columns = ["이름", "코드"]
    is_all = codes.코드.str.contains("_ALL")
    sido = set(codes.이름[is_all].str.replace(" 전체", ""))
    gov = codes[~is_all & ~codes.이름.isin(sido) & ~codes.이름.str.endswith("도")]  # '제주도' 같은 도 본청 코드도 제외한다
    gov = gov.drop_duplicates("코드", keep="last").set_index("코드").이름
    df["코드"] = df.개방자치단체코드.str.strip()
    cnt = df.코드.value_counts()
    new_sido = sorted(set(df.시도명[~df.코드.isin(gov.index) & (df.코드 != "")]))
    old_prefix = [p for s in new_sido for p in OLD_PREFIX.get(s, [])]
    old = gov.map(lambda n: any(n.startswith(p) for p in old_prefix))
    print("표에 없는 코드를 사용하는 시도", new_sido, "→ 표의", old_prefix, "코드는 비교에서 제외", old.sum(), "곳")
    gov = gov[~old]
    n = cnt.reindex(gov.index, fill_value=0)
    zero = list(gov[n == 0])
    few = [(gov[c], v) for c, v in n.items() if 0 < v < 30]
    print("비교 대상 지자체", len(gov), "| 0건", len(zero), "| 30건 미만", len(few), few)
    print("  0건:", zero)

    ok = df[(df.좌표문제 == "") & ~df.대표좌표 & (df.과속방지턱형태구분 != "가상형")]
    usable = len(ok.drop_duplicates([LAT, LON]))
    print("좌표 오류·대표 좌표·가상형을 제외하고 좌표를 병합한 위치", f"{usable:,}", f"({usable / len(df):.0%})")

    # 지오코딩 결과
    geo = read_csv(SPEEDBUMP_DIR / "행안부" / "지오코딩_행안부_좌표오류_대표좌표.csv")
    got = geo[geo.변환위도 != ""]
    print("지오코딩 대상", f"{len(geo):,}", geo.사유.value_counts(sort=False).to_dict(), "고유 주소", f"{geo.사용주소.nunique():,}")
    print("  좌표 얻음", f"{len(got):,}", f"({len(got) / len(geo):.0%})", "결과", geo.변환결과.value_counts().to_dict())
    miss = geo[geo.변환위도 == ""]
    print("  못 찾은 행이 많은 곳", list((miss.시도명 + " " + miss.시군구명).value_counts().head(6).items()))
    pts = got.groupby(["변환위도", "변환경도"], sort=False).agg(행수=("사용주소", "size"), 주소=("사용주소", "first"))
    big = pts[pts.행수 >= 20]
    print("  변환 좌표 하나에 20행 이상", len(big), "곳", big.행수.sum(), "행. 주소:", list(big.주소))


if __name__ == "__main__":
    main()

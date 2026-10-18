"""MOCT_LINK 전체를 읽어 필드별 분포를 출력한다.

python -X utf8 scripts/nodelink/link_stats.py
"""
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from nodelink import read_links

FIELDS = ["LINK_ID", "F_NODE", "T_NODE", "LANES", "ROAD_RANK", "ROAD_TYPE", "ROAD_NAME", "ROAD_USE",
          "MULTI_LINK", "CONNECT", "MAX_SPD", "REST_VEH", "REST_W", "REST_H", "C-ITS", "LENGTH", "REMARK", "HIST_TYPE"]
RANK = {"101": "고속국도", "102": "도시고속국도", "103": "일반국도", "104": "특별광역시도",
        "105": "국가지원지방도", "106": "지방도", "107": "시군도", "108": "기타"}


def show(title, s, names=None, top=None):
    vc = s.value_counts()
    if top:
        vc = vc.head(top)
    print(title, " · ".join(f"{k if k != '' else '(빈값)'}{(' ' + names[k]) if names and k in names else ''} {v:,}" for k, v in vc.items()))


def main():
    links = read_links(FIELDS)
    n = len(links)
    print("링크", f"{n:,}")

    for f in ("ROAD_RANK", "ROAD_USE", "LANES", "ROAD_TYPE", "CONNECT", "MULTI_LINK", "C-ITS", "MAX_SPD", "REST_VEH"):
        show(f, links[f], RANK if f == "ROAD_RANK" else None)
    print("REST_H 값 있음", f"{(links.REST_H != 0).sum():,}", "REST_W 값 있음", f"{(links.REST_W != 0).sum():,}")
    print("ROAD_RANK 빈값", (links.ROAD_RANK == "").sum(), "MAX_SPD 0", (links.MAX_SPD == 0).sum())
    print("길이 중앙값 m", links.LENGTH.median())

    # 구축기준은 방향별 대응링크의 일련번호가 1 차이라고 한다. 10자리 ID로는 ±100이다.
    lid = links.LINK_ID.astype("int64")
    links["대응링크"] = (lid + 100).isin(lid) | (lid - 100).isin(lid)
    pair = links.대응링크.sum()
    print("대응링크(ID ±100) 있음", f"{pair:,}", "없음", f"{n - pair:,}", f"({(n - pair) / n:.1%})")
    no_pair = links[~links.대응링크]
    show("  대응링크 없는 링크의 등급", no_pair.ROAD_RANK, RANK)
    print("  대응링크 없는 링크 중 1차로", f"{(no_pair.LANES == 1).sum():,}")
    ft = pd.MultiIndex.from_frame(links[["F_NODE", "T_NODE"]])
    tf = pd.MultiIndex.from_frame(links[["T_NODE", "F_NODE"]])
    print("F/T 노드가 뒤집힌 쌍이 있는 링크", f"{tf.isin(ft).sum():,}")

    unused = links[links.ROAD_USE == "1"]
    print("ROAD_USE=1", f"{len(unused):,}")
    show("  등급", unused.ROAD_RANK, RANK)
    print("  대응링크 있음", unused.대응링크.sum(), "없음", (~unused.대응링크).sum())
    print("  길이 중앙값 m", unused.LENGTH.median())
    show("  도로명 상위", unused.ROAD_NAME, top=3)
    show("  HIST_TYPE 상위", unused.HIST_TYPE, top=3)

    s10 = links[links.MAX_SPD == 10]
    print("MAX_SPD=10", f"{len(s10):,}")
    print("  시군도 비율", f"{(s10.ROAD_RANK == '107').mean():.1%}", "1차로 비율", f"{(s10.LANES == 1).mean():.1%}",
          "길이 중앙값 m", s10.LENGTH.median(), "도로명 없음", f"{(s10.REMARK == '도로명없음').sum():,}")
    show("  도로명 상위", s10.ROAD_NAME, top=5)

    x0, y0, x1, y1 = links.total_bounds
    print("shp 좌표 범위 x", f"{x0:,.0f}~{x1:,.0f}", "y", f"{y0:,.0f}~{y1:,.0f}")


if __name__ == "__main__":
    main()

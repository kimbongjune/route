"""한국도로공사 노면색깔유도선 현황 CSV를 내비 API가 읽는 파일로 만든다.

- 시설물 명칭을 표준노드링크 NODE_NAME 형식으로 바꾼다: 공백 제거, "나들목"→"IC", "분기점"→"JC", "영업소"→"TG"
- 색 표기를 통일한다: 분홍→분홍색, 연녹색·연한 녹색→연한녹색, 끝 공백 제거, 괄호 설명 제거, 쉼표·빗금으로 이어진 색은 "·"로 잇는다
- 출력: maps/유도선/유도선_노드.csv (UTF-8). 컬럼: 노드이름, 노선명, 방향, 분기형태, 좌, 우, 비고
- 표준노드링크 NODE_NAME(괄호 설명 제거, JCT→JC)과 이름이 같은 시설물 수와 행 수를 센다

python -X utf8 scripts/guideline/build_guide_lines.py
"""
import re
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, read_csv, write_csv
from nodelink import read_nodes

SRC = ROOT / "maps" / "유도선" / "한국도로공사_노면색깔유도선 현황_20260811.csv"
OUT = ROOT / "maps" / "유도선" / "유도선_노드.csv"
COLOR = {"분홍": "분홍색", "연녹색": "연한녹색", "연한 녹색": "연한녹색", "미개통": ""}


def node_name(facility):
    name = facility.replace(" ", "")
    name = re.sub(r"나들목$", "IC", name)
    name = re.sub(r"분기점$", "JC", name)
    name = re.sub(r"영업소$", "TG", name)
    return name


def color(text):
    parts = []
    for part in re.split(r"[,/]", text):
        part = re.sub(r"\(.*?\)", "", part).strip()
        part = COLOR.get(part, part)
        if part:
            parts.append(part)
    return "·".join(parts)


def main():
    df = read_csv(SRC)
    out = pd.DataFrame({
        "노드이름": df["시설물 명칭"].str.strip().map(node_name),
        "노선명": df["노선명"].str.strip(),
        "방향": df["방향"].str.strip(),
        "분기형태": df["분기형태"].str.strip(),
        "좌": df["좌"].map(color),
        "우": df["우"].map(color),
        "비고": df["비고"].str.strip(),
    })
    out = out[(out["좌"] != "") | (out["우"] != "")]
    write_csv(OUT, out)
    print(f"{OUT.name}: {len(out):,}행 (원본 {len(df):,}행, 색이 없는 행 {len(df) - len(out)} 제외)")
    print("좌", out["좌"].value_counts().to_dict())
    print("우", out["우"].value_counts().to_dict())
    print("노드이름 종류", out["노드이름"].nunique())
    names = set(re.sub(r"\(.*?\)", "", n).replace(" ", "").replace("JCT", "JC")
                for n in read_nodes(["NODE_ID", "NODE_NAME"], geometry=False)["NODE_NAME"].fillna(""))
    matched = out[out["노드이름"].isin(names)]
    print(f"노드 이름과 같은 시설물 {matched['노드이름'].nunique()}종 {len(matched):,}행, 비고별", matched["비고"].value_counts().head(6).to_dict())


if __name__ == "__main__":
    main()

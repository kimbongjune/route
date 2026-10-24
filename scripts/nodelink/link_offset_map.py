"""표준노드링크 링크 좌표를 브이월드 위성·배경 타일 위에 그려 실제 도로와의 어긋남을 확인한다.

- 인자: 링크 ID들(쉼표 구분), 중심 위도, 중심 경도, 타일 레이어(Satellite 또는 Base), 줌(기본 18)
- 링크마다 색을 바꿔 그리고 링크 ID를 적는다. 결과는 maps/노드링크확인/링크위치_<첫 링크>.png
- 금정교 확인: python -X utf8 scripts/nodelink/link_offset_map.py 3150036901,3150037002,3150036900,3150037001 35.7170 127.6140 Satellite

python -X utf8 scripts/nodelink/link_offset_map.py <링크ID,...> <위도> <경도> [Satellite|Base] [줌]
"""
import io
import math
import sys
import urllib.request
from pathlib import Path

from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tollroad"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, env
from nodelink import read_links, tm_to_wgs84

COLORS = [(40, 90, 220), (220, 60, 60), (30, 160, 60), (230, 140, 0)]
TILES = 5


def tile_xy(lat, lon, z):
    n = 2 ** z
    x = (lon + 180) / 360 * n
    y = (1 - math.log(math.tan(math.radians(lat)) + 1 / math.cos(math.radians(lat))) / math.pi) / 2 * n
    return x, y


def main():
    ids = sys.argv[1].split(",")
    lat, lon = float(sys.argv[2]), float(sys.argv[3])
    layer = sys.argv[4] if len(sys.argv) > 4 else "Satellite"
    z = int(sys.argv[5]) if len(sys.argv) > 5 else 18
    ext = "jpeg" if layer == "Satellite" else "png"
    key = env("VWORLD_API_KEY")
    cx, cy = tile_xy(lat, lon, z)
    tx0, ty0 = int(cx) - TILES // 2, int(cy) - TILES // 2
    img = Image.new("RGB", (TILES * 256, TILES * 256))
    for dx in range(TILES):
        for dy in range(TILES):
            url = f"https://api.vworld.kr/req/wmts/1.0.0/{key}/{layer}/{z}/{ty0 + dy}/{tx0 + dx}.{ext}"
            img.paste(Image.open(io.BytesIO(urllib.request.urlopen(url, timeout=20).read())), (dx * 256, dy * 256))
    draw = ImageDraw.Draw(img)
    links = read_links(["LINK_ID", "ROAD_NAME"]).set_index("LINK_ID")
    for i, lid in enumerate(ids):
        geom = links.loc[lid].geometry
        pts = []
        for x, y in geom.coords:
            plat, plon = tm_to_wgs84(x, y)
            px, py = tile_xy(plat, plon, z)
            pts.append(((px - tx0) * 256, (py - ty0) * 256))
        color = COLORS[i % len(COLORS)]
        draw.line(pts, fill=color, width=4)
        draw.text(pts[len(pts) // 2], f"{lid} {links.loc[lid, 'ROAD_NAME']}", fill=color)
    out = ROOT / "maps" / "노드링크확인" / f"링크위치_{ids[0]}_{layer}.png"
    out.parent.mkdir(parents=True, exist_ok=True)
    img.crop((256, 256, (TILES - 1) * 256, (TILES - 1) * 256)).save(out)
    print(out)


if __name__ == "__main__":
    main()

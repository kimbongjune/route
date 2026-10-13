"""스크립트 공용 함수.

API 키는 프로젝트 루트의 .env에서 읽고 화면에 출력하지 않는다.
"""
import json
import time
import urllib.error
import urllib.request
from pathlib import Path

import pandas as pd

ROOT = Path(__file__).resolve().parents[1]
SPEEDBUMP_DIR = ROOT / "maps" / "과속방지턱정보"
TOLLROAD_DIR = ROOT / "maps" / "유료도로"


def env(name):
    """.env에서 변수 하나를 읽는다. 없으면 SystemExit."""
    for line in (ROOT / ".env").read_text(encoding="utf-8-sig").splitlines():
        if line.startswith(name + "="):
            return line.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit(f".env에 {name} 가 없습니다")


def http_get(url, timeout=60, retries=3, secrets=()):
    """GET 요청. 실패하면 retries번 재시도한다. 오류 메시지에서 secrets 값은 가린다."""
    last = None
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
            return urllib.request.urlopen(req, timeout=timeout).read()
        except urllib.error.HTTPError as e:
            return e.read()  # 게이트웨이 오류 본문인 JSON, XML을 그대로 반환한다
        except Exception as e:
            last = str(e)
            time.sleep(1 + attempt)
    msg = last or "알 수 없는 오류"
    for s in secrets:
        msg = msg.replace(s, "<KEY>")
    raise RuntimeError(msg)


def get_json(url, **kw):
    return json.loads(http_get(url, **kw).decode("utf-8"))


def read_csv(path):
    """CSV를 모든 컬럼 문자열로 읽는다. utf-8-sig로 읽히지 않으면 cp949로 읽는다."""
    for enc in ("utf-8-sig", "cp949"):
        try:
            df = pd.read_csv(path, dtype=str, keep_default_na=False, encoding=enc)
        except UnicodeDecodeError:
            continue
        # 머리글이 빈 컬럼은 pandas가 'Unnamed: n'으로 이름을 붙이므로 원래대로 빈 이름으로 둔다
        df.columns = ["" if c.startswith("Unnamed: ") else c for c in df.columns]
        return df
    raise ValueError(f"인코딩을 알 수 없음: {path}")


def write_csv(path, df, cols=None):
    """utf-8-sig CSV로 저장한다. cols에 없는 컬럼은 버리고 df에 없는 컬럼은 빈 값으로 둔다."""
    if not isinstance(df, pd.DataFrame):
        df = pd.DataFrame(list(df), dtype=object)
    if cols is not None:
        df = df.reindex(columns=cols)
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    df.astype(object).where(df.notna(), "").to_csv(path, index=False, encoding="utf-8-sig")

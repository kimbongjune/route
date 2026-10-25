#!/usr/bin/env bash
# 전국 16개 시도의 골목 입력 CSV를 만든다. 시도마다 extract_alleys.py를 실행하며 동시에 3개씩 돌린다.
# 로그는 maps/골목/<시도 코드>.log에 남는다
# bash scripts/alley/extract_all.sh
cd "$(dirname "$0")/../.."
mkdir -p maps/골목
printf '%s\n' 41 11 48 47 12 44 51 43 52 28 26 27 30 31 50 36 |
  xargs -P 3 -I{} sh -c 'python -X utf8 -u scripts/alley/extract_alleys.py {} > maps/골목/{}.log 2>&1; echo "{} 끝 $?"'

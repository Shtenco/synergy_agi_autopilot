#!/usr/bin/env bash
set -euo pipefail
ROOT="${1:-vendor/shttps}"
COMMIT="e8bb87fae6d98417782d51a583d4b9bee7896680"
if [ ! -d "$ROOT/.git" ]; then git clone https://github.com/Shtenco/shttps.git "$ROOT"; fi
git -C "$ROOT" fetch origin "$COMMIT"
git -C "$ROOT" checkout --detach "$COMMIT"
python -m pip install -e "$ROOT[pq]"

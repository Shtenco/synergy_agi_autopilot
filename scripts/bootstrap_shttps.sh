#!/usr/bin/env bash
set -euo pipefail
ROOT="${1:-vendor/shttps}"
COMMIT="85dd756dab6c923a9c819017d1f20c80647a360e"
if [ ! -d "$ROOT/.git" ]; then git clone https://github.com/Shtenco/shttps.git "$ROOT"; fi
git -C "$ROOT" fetch origin "$COMMIT"
git -C "$ROOT" checkout --detach "$COMMIT"
python -m pip install -e "$ROOT[pq]"

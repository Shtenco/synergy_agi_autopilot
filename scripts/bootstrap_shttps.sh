#!/usr/bin/env bash
set -euo pipefail
ROOT="${1:-vendor/shttps}"
COMMIT="1776c255dd1af37a376784f771456176e6a4a8cc"
if [ ! -d "$ROOT/.git" ]; then git clone https://github.com/Shtenco/shttps.git "$ROOT"; fi
git -C "$ROOT" fetch origin "$COMMIT"
git -C "$ROOT" checkout --detach "$COMMIT"
python -m pip install -e "$ROOT[pq]"

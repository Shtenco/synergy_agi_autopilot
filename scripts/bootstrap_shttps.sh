#!/usr/bin/env bash
set -euo pipefail
ROOT="${1:-vendor/shttps}"
COMMIT="3625fd9fc4dbb319dcf58f12e7a58c09322ef91e"
if [ ! -d "$ROOT/.git" ]; then git clone https://github.com/Shtenco/shttps.git "$ROOT"; fi
git -C "$ROOT" fetch origin "$COMMIT"
git -C "$ROOT" checkout --detach "$COMMIT"
python -m pip install -e "$ROOT"

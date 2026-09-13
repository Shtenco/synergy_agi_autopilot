#!/usr/bin/env bash
set -euo pipefail
ROOT="${1:-vendor/shttps}"
COMMIT="2d6cab8af334c083d7d7fe31f6d298777bde200a"
if [ ! -d "$ROOT/.git" ]; then git clone https://github.com/Shtenco/shttps.git "$ROOT"; fi
git -C "$ROOT" fetch origin "$COMMIT"
git -C "$ROOT" checkout --detach "$COMMIT"
python -m pip install -e "$ROOT[pq]"

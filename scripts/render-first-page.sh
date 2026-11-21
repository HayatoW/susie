#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"

if ! command -v tesseract >/dev/null 2>&1; then
  echo "tesseract がありません。フレームの文字を確認できません。" >&2
  exit 1
fi

export CEF_PATH="${CEF_PATH:-"$HOME/.cache/susie/cef"}"
mkdir -p "$CEF_PATH"
mkdir -p "$ROOT/docs/evidence/S03"

cargo build --manifest-path "$ROOT/crates/engine/Cargo.toml" --bin render-first-page
"$ROOT/target/debug/render-first-page" \
  --fixture "$ROOT/fixtures/first-page.html" \
  --output "$ROOT/docs/evidence/S03/frame.png"

text=$(tesseract "$ROOT/docs/evidence/S03/frame.png" stdout --psm 6 | tr '[:upper:]' '[:lower:]')
printf '%s\n' "$text"
printf '%s\n' "$text" | grep -q 'susie-top'
if printf '%s\n' "$text" | grep -q 'susie-bottom'; then
  echo "susie-bottom がフレームに写っています。" >&2
  exit 1
fi

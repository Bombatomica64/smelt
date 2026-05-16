#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <lcov.info> <output.svg>"
  exit 1
fi

LCOV_FILE="$1"
OUTPUT_BADGE="$2"

if [[ ! -f "$LCOV_FILE" ]]; then
  echo "lcov file not found: $LCOV_FILE"
  exit 1
fi

percent() {
  local hit="$1"
  local found="$2"
  if [[ "$found" -eq 0 ]]; then
    printf '0.00'
  else
    awk -v h="$hit" -v f="$found" 'BEGIN { printf "%.2f", (h / f) * 100 }'
  fi
}

pick_color() {
  local p_int
  p_int="${1%.*}"
  if (( p_int >= 90 )); then echo "#4c1"; return; fi
  if (( p_int >= 80 )); then echo "#97CA00"; return; fi
  if (( p_int >= 70 )); then echo "#a4a61d"; return; fi
  if (( p_int >= 60 )); then echo "#dfb317"; return; fi
  if (( p_int >= 50 )); then echo "#fe7d37"; return; fi
  echo "#e05d44"
}

make_badge() {
  local label="$1"
  local pct="$2"
  local file="$3"
  local color
  color="$(pick_color "$pct")"

  cat > "$file" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="140" height="20" role="img" aria-label="$label: $pct%">
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
  <clipPath id="r">
    <rect width="140" height="20" rx="3" fill="#fff"/>
  </clipPath>
  <g clip-path="url(#r)">
    <rect width="78" height="20" fill="#555"/>
    <rect x="78" width="62" height="20" fill="$color"/>
    <rect width="140" height="20" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" text-rendering="geometricPrecision" font-size="11">
    <text x="40" y="15" fill="#010101" fill-opacity=".3">$label</text>
    <text x="40" y="14">$label</text>
    <text x="108" y="15" fill="#010101" fill-opacity=".3">$pct%</text>
    <text x="108" y="14">$pct%</text>
  </g>
</svg>
SVG
}

sum_value() {
  local key="$1"
  awk -F: -v k="$key" '$1 == k { s += $2 } END { print s + 0 }' "$LCOV_FILE"
}

lf="$(sum_value LF)"
lh="$(sum_value LH)"
fnf="$(sum_value FNF)"
fnh="$(sum_value FNH)"
brf="$(sum_value BRF)"
brh="$(sum_value BRH)"

line_pct="$(percent "$lh" "$lf")"
fn_pct="$(percent "$fnh" "$fnf")"
branch_pct="$(percent "$brh" "$brf")"

region_pct="$line_pct"

make_badge "coverage" "$line_pct" "$OUTPUT_BADGE"
make_badge "functions" "$fn_pct" "coverage-functions.svg"
make_badge "lines" "$line_pct" "coverage-lines.svg"
make_badge "regions" "$region_pct" "coverage-regions.svg"
make_badge "branches" "$branch_pct" "coverage-branches.svg"

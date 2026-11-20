#!/bin/sh
set -eu

TPL="/etc/nginx/default.conf.tpl"
OUT="/etc/nginx/conf.d/default.conf"
TOKEN="${OWNER_API_BEARER_TOKEN:-}"

if [ -n "$TOKEN" ]; then
  HEADER_VAL="Bearer $TOKEN"
else
  HEADER_VAL=""
fi

HEADER_VAL="$HEADER_VAL" awk '{
  val = ENVIRON["HEADER_VAL"]
  gsub(/\\/, "\\\\", val)
  gsub(/"/, "\\\"", val)
  gsub(/&/, "\\\\&", val)
  sub(/__PROXY_DEFAULT_BEARER__/, val)
  print
}' "$TPL" > "$OUT"
chmod 0644 "$OUT"

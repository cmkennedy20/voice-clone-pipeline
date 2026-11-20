#!/bin/sh
set -eu

LAN_HOST_IP="${LAN_HOST_IP:-127.0.0.1}"
CERT_DIR="/etc/nginx/certs"
CERT_FILE="${CERT_DIR}/lan.crt"
KEY_FILE="${CERT_DIR}/lan.key"
IP_TRACK_FILE="${CERT_DIR}/lan_ip.txt"

mkdir -p "$CERT_DIR"

CURRENT_SAVED_IP=""
if [ -f "$IP_TRACK_FILE" ]; then
  CURRENT_SAVED_IP="$(cat "$IP_TRACK_FILE" 2>/dev/null || true)"
fi

if [ ! -s "$CERT_FILE" ] || [ ! -s "$KEY_FILE" ] || [ "$CURRENT_SAVED_IP" != "$LAN_HOST_IP" ]; then
  case "$LAN_HOST_IP" in
    *[0-9].[0-9]*|*:*)
      SAN_PRIMARY="IP:${LAN_HOST_IP}"
      ;;
    *)
      SAN_PRIMARY="DNS:${LAN_HOST_IP}"
      ;;
  esac

  echo "[10-generate-lan-cert] Generating 3650-day self-signed SAN TLS cert for ${LAN_HOST_IP}..."
  openssl req -x509 -nodes -newkey rsa:2048 -days 3650 \
    -keyout "$KEY_FILE" \
    -out "$CERT_FILE" \
    -subj "/CN=${LAN_HOST_IP}" \
    -addext "subjectAltName=${SAN_PRIMARY},IP:127.0.0.1,DNS:localhost"
  chmod 600 "$KEY_FILE"
  chmod 644 "$CERT_FILE"
  printf '%s' "$LAN_HOST_IP" > "$IP_TRACK_FILE"
else
  echo "[10-generate-lan-cert] Reusing persisted SAN TLS cert for ${LAN_HOST_IP}."
fi

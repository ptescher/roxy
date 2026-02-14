#!/usr/bin/env bash
set -euo pipefail

# ── CA certificate trust ──────────────────────────────────────────
CA_SRC="/usr/local/share/ca-certificates/roxy-ca.crt"
CA_DST="/etc/ssl/certs/roxy-ca.pem"

if [ -f "$CA_SRC" ]; then
    cp "$CA_SRC" "$CA_DST"
    update-ca-certificates --fresh >/dev/null 2>&1 || true
fi

# ── Proxy connectivity check ──────────────────────────────────────
if [ -n "${HTTP_PROXY:-}" ]; then
    # Extract host:port from the proxy URL
    PROXY_URL="${HTTP_PROXY#http://}"
    PROXY_URL="${PROXY_URL#https://}"
    PROXY_HOST="${PROXY_URL%%:*}"
    PROXY_PORT="${PROXY_URL##*:}"
    PROXY_PORT="${PROXY_PORT%%/*}"

    if curl -sS --max-time 3 --proxy "$HTTP_PROXY" -o /dev/null http://example.org 2>/dev/null; then
        PROXY_STATUS="connected"
    else
        PROXY_STATUS="unreachable (proxy at ${PROXY_HOST}:${PROXY_PORT})"
    fi
else
    PROXY_STATUS="not configured"
fi

# ── Git proxy + SSL configuration ─────────────────────────────────
if [ -n "${HTTP_PROXY:-}" ]; then
    git config --global http.proxy "$HTTP_PROXY" 2>/dev/null || true
fi
if [ -n "${HTTPS_PROXY:-}" ]; then
    git config --global https.proxy "$HTTPS_PROXY" 2>/dev/null || true
fi
if [ -f "$CA_DST" ]; then
    git config --global http.sslCAInfo "$CA_DST" 2>/dev/null || true
fi

# ── Claude Code support ───────────────────────────────────────────
if [ "${ROXY_CLAUDE_CODE:-}" = "true" ]; then
    if ! command -v claude >/dev/null 2>&1; then
        echo "Installing Claude Code..."
        npm install -g @anthropic-ai/claude-code 2>/dev/null || true
    fi
fi

# ── Banner ────────────────────────────────────────────────────────
echo ""
echo "  ╭─────────────────────────────────────╮"
echo "  │         Roxy Sandbox                 │"
echo "  ╰─────────────────────────────────────╯"
echo ""
echo "  Proxy:     ${HTTP_PROXY:-none}"
echo "  SOCKS:     ${ALL_PROXY:-none}"
echo "  Status:    ${PROXY_STATUS}"
echo "  Workspace: $(pwd)"
if [ "${ROXY_CLAUDE_CODE:-}" = "true" ]; then
echo "  Claude:    enabled"
fi
echo ""

exec "$@"

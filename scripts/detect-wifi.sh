#!/bin/bash
# Detect active WiFi interface

# Try to find WiFi interface (wl* pattern)
WIFI=$(ip link show | grep -oP 'wl[a-z0-9]+' | head -1)

if [ -n "$WIFI" ]; then
    echo "$WIFI"
    exit 0
fi

# Fallback: check wireless extensions
WIFI=$(iw dev | awk '$1=="Interface"{print $2}' | head -1)

if [ -n "$WIFI" ]; then
    echo "$WIFI"
    exit 0
fi

# Nothing found
echo "ERROR: No WiFi interface detected" >&2
exit 1

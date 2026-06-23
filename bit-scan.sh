#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cd "$SCRIPT_DIR"
exec "$SCRIPT_DIR/bin/bit-scan" scan --version v3 --stats "$@"

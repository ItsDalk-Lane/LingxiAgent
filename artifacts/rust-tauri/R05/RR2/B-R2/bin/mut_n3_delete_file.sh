#!/usr/bin/env bash
# N3: a tracked file is DELETED mid-binding
F="$1/scripts/rust-tauri/r02_t01_service_smoke.sh"
rm -f "$F"
echo "deleted $F"

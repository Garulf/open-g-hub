#!/usr/bin/env bash
set -euo pipefail

host=${1:?usage: ps-remote.sh <host> < script.ps1}

script="\$ProgressPreference = 'SilentlyContinue'; \$ErrorActionPreference = 'Stop'
$(cat)"
encoded=$(printf '%s' "$script" | iconv -f UTF-8 -t UTF-16LE | base64 -w0)

ssh -o BatchMode=yes "$host" "powershell -NoProfile -NonInteractive -EncodedCommand $encoded" </dev/null | tr -d '\r'

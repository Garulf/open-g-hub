#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

remote=$(to_sftp_path "${1:?usage: fetch.sh <remote-path> <local-path>}")
local_path=${2:?usage: fetch.sh <remote-path> <local-path>}

echo "get \"$remote\" \"$local_path\"" | sftp_batch
verify_size "$remote" "$local_path"

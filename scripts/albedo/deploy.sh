#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

local_path=${1:?usage: deploy.sh <local-file>}
remote="$scratch_sftp/$(basename "$local_path")"

echo "put \"$local_path\" \"$remote\"" | sftp_batch
verify_size "$remote" "$local_path"
echo "$remote"

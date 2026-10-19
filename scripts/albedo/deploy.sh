#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

local_path=${1:?usage: deploy.sh <local-file>}
scratch=$(remote_scratch)
remote="$(to_sftp_path "$scratch")/$(basename "$local_path")"

echo "put \"$local_path\" \"$remote\"" | sftp_batch
verify_size "$remote" "$local_path"
echo "$remote"

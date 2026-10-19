#!/usr/bin/env bash

albedo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$albedo_dir/../.." && pwd)

albedo_host=${OGH_ALBEDO_HOST:-albedo}
pcap_header_bytes=24
usbpcap_exe='C:\Program Files\USBPcap\USBPcapCMD.exe'

ps_remote() {
    "$albedo_dir/ps-remote.sh" "$albedo_host"
}

sftp_batch() {
    sftp -q -R 1 -b - -o BatchMode=yes "$albedo_host" >/dev/null
}

to_sftp_path() {
    local path=${1//\\//}
    [[ $path =~ ^[A-Za-z]: ]] && path="/$path"
    printf '%s' "$path"
}

# The remote scratch directory as a Windows path: OGH_ALBEDO_SCRATCH when set, otherwise
# open-g-hub under the remote user's temp directory, created if missing.
remote_scratch() {
    local dir=${OGH_ALBEDO_SCRATCH:-}
    if [[ -z $dir ]]; then
        dir=$(ps_remote <<'PS'
$dir = Join-Path ([IO.Path]::GetTempPath()) 'open-g-hub'
$null = New-Item -ItemType Directory -Force -Path $dir
$dir
PS
)
    fi
    if [[ -z $dir || $dir == *"'"* ]]; then
        echo "unusable remote scratch directory: '$dir'" >&2
        return 1
    fi
    printf '%s' "$dir"
}

remote_size() {
    local win_path=${1#/}
    ps_remote <<PS
(Get-Item -LiteralPath '${win_path//\'/\'\'}').Length
PS
}

verify_size() {
    local sftp_path=$1 local_path=$2
    local remote_bytes local_bytes
    remote_bytes=$(remote_size "$sftp_path")
    local_bytes=$(stat -c %s "$local_path")
    if [[ $remote_bytes != "$local_bytes" ]]; then
        echo "size mismatch: remote $remote_bytes, local $local_bytes" >&2
        return 1
    fi
}

ps_remote_with_exe() {
    {
        echo "\$exe = '$usbpcap_exe'"
        cat
    } | ps_remote
}

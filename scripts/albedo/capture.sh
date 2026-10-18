#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

label=${1:?usage: capture.sh <label> <seconds>}
seconds=${2:?usage: capture.sh <label> <seconds>}
hub=${OGH_USBPCAP_HUB:-USBPcap1}
device=${OGH_USBPCAP_DEVICE:-8}

[[ $label =~ ^[A-Za-z0-9._-]+$ ]] || { echo "invalid label: $label" >&2; exit 2; }
[[ $seconds =~ ^[0-9]+$ ]] || { echo "invalid seconds: $seconds" >&2; exit 2; }
[[ $hub =~ ^USBPcap[0-9]+$ ]] || { echo "invalid hub: $hub" >&2; exit 2; }
[[ $device =~ ^[0-9]+$ ]] || { echo "invalid device: $device" >&2; exit 2; }

remote_pcap="$scratch_win\\$label.pcap"
local_pcap="$repo_root/captures/$label.pcap"

ps_remote_with_exe <<PS
\$out = '$remote_pcap'
Remove-Item -LiteralPath \$out -ErrorAction SilentlyContinue
\$p = Start-Process -FilePath \$exe -ArgumentList '-d','\\\\.\\$hub','--devices','$device','-o',\$out -PassThru -WindowStyle Hidden
try { Start-Sleep -Seconds $seconds } finally { Stop-Process -Id \$p.Id -Force; Start-Sleep -Milliseconds 500 }
PS

mkdir -p "$repo_root/captures"
"$albedo_dir/fetch.sh" "$remote_pcap" "$local_pcap"
stat -c %s "$local_pcap"

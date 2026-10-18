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

{
    echo "\$out = '$remote_pcap'"
    echo "\$hub = '$hub'"
    echo "\$device = '$device'"
    echo "\$seconds = $seconds"
    cat <<'PS'
Get-Process USBPcapCMD -ErrorAction SilentlyContinue | ForEach-Object {
    Stop-Process -Id $_.Id -Force
    if (-not $_.WaitForExit(5000)) { throw "stale USBPcapCMD (pid $($_.Id)) did not exit" }
}
if (Test-Path -LiteralPath $out) { Remove-Item -LiteralPath $out -Force }

$p = Start-Process -FilePath $exe -ArgumentList '-d',"\\.\$hub",'--devices',$device,'-o',$out -PassThru -WindowStyle Hidden
$null = $p.Handle
try {
    Start-Sleep -Seconds $seconds
    if ($p.HasExited) { throw "USBPcapCMD exited early with code $($p.ExitCode) (hub $hub, device $device)" }
} finally {
    if (-not $p.HasExited) {
        Stop-Process -Id $p.Id -Force
        if (-not $p.WaitForExit(5000)) { throw "USBPcapCMD (pid $($p.Id)) did not exit after stop" }
    }
}
PS
} | ps_remote_with_exe

mkdir -p "$repo_root/captures"
"$albedo_dir/fetch.sh" "$remote_pcap" "$local_pcap"
size=$(stat -c %s "$local_pcap")
if ((size <= pcap_header_bytes)); then
    echo "warning: pcap holds no packets (idle device, or wrong OGH_USBPCAP_DEVICE=$device?)" >&2
fi
echo "$size"

#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

hub_configs=$(ps_remote_with_exe <<'PS'
$interfaces = & $exe --extcap-interfaces | Out-String
foreach ($m in [regex]::Matches($interfaces, '\{display=(USBPcap\d+)\}')) {
    $hub = $m.Groups[1].Value
    $config = & $exe --extcap-interface "\\.\$hub" --extcap-config | Out-String
    $config -split "`n" | ForEach-Object { "$hub`t$_" }
}
PS
)

match=$(awk -F'\t' '
    $2 ~ /\{value=[0-9]+_[0-9]+\}\{display=[^}]*A50 X/ {
        match($2, /\{value=[0-9]+/)
        print $1, substr($2, RSTART + 7, RLENGTH - 7)
        exit
    }
' <<<"$hub_configs")

if [[ -z $match ]]; then
    echo "no USBPcap device with an A50 X interface found" >&2
    exit 1
fi
echo "$match"

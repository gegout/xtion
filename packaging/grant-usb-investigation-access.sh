#!/bin/bash
# Install reviewed root-owned helpers and exact sudo command grants.
# This installer changes permissions policy only; it does not restart USB.
set -euo pipefail

xtion_allow_controller=0
case "${1:-}" in
    '') ;;
    --allow-controller-reset) xtion_allow_controller=1 ;;
    --help|-h)
        printf '%s\n' 'Usage: sudo bash grant-usb-investigation-access.sh [--allow-controller-reset]' \
          'Default: read-only USB diagnostics and guarded unoccupied-port recovery.' \
          'Opt-in: shared-controller restart, temporarily disconnecting mouse, cameras and Bluetooth.'
        exit 0 ;;
    *) printf '%s\n' 'Unknown option.' >&2; exit 2 ;;
esac
if (( $# > 1 )); then
    printf '%s\n' 'Too many arguments.' >&2
    exit 2
fi
if (( EUID != 0 )); then
    printf '%s\n' 'Run this installer with sudo in your normal terminal.' >&2
    exit 1
fi
xtion_account=${SUDO_USER:-}
if [[ -z "$xtion_account" || "$xtion_account" == root || ! "$xtion_account" =~ ^[a-z_][a-z0-9_-]*\$?$ ]]; then
    printf '%s\n' 'A normal invoking account (SUDO_USER) is required; do not run from a root shell.' >&2
    exit 1
fi
/usr/bin/id -- "$xtion_account" >/dev/null
xtion_source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
xtion_destination=/usr/local/libexec/xtion
for xtion_directory in /usr/local /usr/local/libexec "$xtion_destination"; do
    if [[ -L "$xtion_directory" || ( -e "$xtion_directory" && ! -d "$xtion_directory" ) ]]; then
        printf 'Refusing unexpected installation path: %s\n' "$xtion_directory" >&2
        exit 1
    fi
    if [[ -d "$xtion_directory" ]]; then
        xtion_owner=$(/usr/bin/stat -c '%u' -- "$xtion_directory")
        xtion_mode=$(/usr/bin/stat -c '%a' -- "$xtion_directory")
        if [[ "$xtion_owner" != 0 ]] || (( (8#$xtion_mode & 0022) != 0 )); then
            printf 'Installation parent must be root-owned and not group/world writable: %s\n' "$xtion_directory" >&2
            exit 1
        fi
    fi
done
/usr/bin/install -d -o root -g root -m 0755 "$xtion_destination"
for xtion_helper in usb_diagnostics.py recover_usb.py recover_controller.py; do
    /usr/bin/install -o root -g root -m 0644 \
      "$xtion_source_dir/scripts/$xtion_helper" "$xtion_destination/$xtion_helper"
done
xtion_policy=$(/usr/bin/mktemp)
trap '/usr/bin/rm -f -- "$xtion_policy"' EXIT
{
    printf '%s\n' '# Xtion investigation: exact root-owned helper commands, isolated Python.'
    printf '%s ALL=(root) NOPASSWD: /usr/bin/python3 -I %s/usb_diagnostics.py\n' "$xtion_account" "$xtion_destination"
    printf '%s ALL=(root) NOPASSWD: /usr/bin/python3 -I %s/recover_usb.py\n' "$xtion_account" "$xtion_destination"
    printf '%s ALL=(root) NOPASSWD: /usr/bin/python3 -I %s/recover_usb.py --apply\n' "$xtion_account" "$xtion_destination"
    printf '%s ALL=(root) NOPASSWD: /usr/bin/python3 -I %s/recover_controller.py\n' "$xtion_account" "$xtion_destination"
    if (( xtion_allow_controller )); then
        printf '%s ALL=(root) NOPASSWD: /usr/bin/python3 -I %s/recover_controller.py --apply --allow-unrelated-usb-disconnect\n' "$xtion_account" "$xtion_destination"
    fi
} > "$xtion_policy"
/usr/sbin/visudo -cf "$xtion_policy"
/usr/bin/install -o root -g root -m 0440 "$xtion_policy" /etc/sudoers.d/xtion-investigation
/usr/sbin/visudo -cf /etc/sudoers.d/xtion-investigation
printf 'Installed scoped USB investigation access for %s. Shared-controller restart allowed: %s\n' "$xtion_account" "$xtion_allow_controller"
printf '%s\n' 'No USB restart was performed by this installer.' \
  'Revoke access with: sudo rm /etc/sudoers.d/xtion-investigation'

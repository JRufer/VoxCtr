#!/bin/bash
# VoxCtrl Permissions Setup Script
# This script configures udev rules and user groups for global hotkeys and text injection.
#
# The rule uses systemd's `uaccess` tag, so access is granted to the active
# local session immediately after the rules are reloaded — no logout required.
# Group membership is kept only as a fallback for systems without logind.

set -e

echo "[*] VoxCtrl: Setting up system permissions..."

UDEV_RULE="/etc/udev/rules.d/99-voxctrl.rules"

# Written unconditionally: older VoxCtrl versions installed a group-only rule,
# and leaving that in place is exactly what forces users to log out.
echo "[*] Installing udev rule for input devices..."
sudo tee "$UDEV_RULE" > /dev/null <<'EOF'
# Installed by VoxCtrl — global hotkey and input access.
#
# uaccess grants the user of the ACTIVE local session an ACL on these devices,
# applied immediately by systemd-logind on `udevadm trigger`. This is why
# VoxCtrl does not require logging out after setup, and it is narrower than
# permanent `input` group membership: access follows the seat, not the account.
SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"

# Virtual keyboard device used for synthetic keystroke injection.
KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput", TAG+="uaccess"
EOF
sudo chmod 0644 "$UDEV_RULE"

echo "[*] Reloading udev rules..."
sudo udevadm control --reload-rules || true
sudo udevadm trigger --subsystem-match=input --action=change || true
sudo udevadm trigger --subsystem-match=misc --action=change || true

# Fallback: add the user to 'input' for systems where uaccess does not apply.
# REAL_USER is set by install.sh; SUDO_USER is set by sudo; $USER is fallback
TARGET_USER="${REAL_USER:-${SUDO_USER:-$USER}}"
echo "[*] Adding $TARGET_USER to the 'input' group (fallback)..."
sudo usermod -aG input "$TARGET_USER" || true

hotkeys_live() {
    for dev in /dev/input/event*; do
        [ -r "$dev" ] && return 0
    done
    return 1
}

echo ""
echo "----------------------------------------------------------------"
if hotkeys_live; then
    echo "DONE! Keyboard access is live — no logout required."
else
    echo "DONE! Start VoxCtrl; it picks up the new permissions on launch and"
    echo "tells you in-app if anything is still missing."
fi
echo "----------------------------------------------------------------"

#!/bin/bash
# Whisper Wayland Permissions Setup Script
# This script configures udev rules and user groups for global hotkeys and text injection.

set -e

echo "[*] Whisper Wayland: Setting up system permissions..."

# 1. Check for udev rules
UDEV_RULE="/etc/udev/rules.d/99-whisper-wayland.rules"
RULE_CONTENT='KERNEL=="uinput", GROUP="uinput", MODE="0660"'

if [ ! -f "$UDEV_RULE" ]; then
    echo "[*] Creating udev rule for uinput..."
    echo "$RULE_CONTENT" | sudo tee "$UDEV_RULE" > /dev/null
    echo "[*] Reloading udev rules..."
    sudo udevadm control --reload-rules && sudo udevadm trigger
else
    echo "[OK] udev rule already exists."
fi

# 2. Check for uinput group
if ! getent group uinput > /dev/null; then
    echo "[*] Creating uinput group..."
    sudo groupadd -f uinput
fi

# 3. Add user to groups
echo "[*] Adding $USER to 'input' and 'uinput' groups..."
sudo usermod -aG input,uinput "$USER"

echo ""
echo "----------------------------------------------------------------"
echo "DONE! Please LOG OUT and LOG BACK IN for changes to take effect."
echo "----------------------------------------------------------------"

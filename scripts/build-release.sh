#!/usr/bin/env bash
# Build the release firmware and produce a single merged binary suitable for
# flashing via ESP Web Tools.
#
# The merged image bundles bootloader + partition table + the application at
# the right offsets, so the browser flasher only needs to ship a single file
# at offset 0.
#
# Required: the Espressif Rust toolchain (`espup install`) and `espflash`.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIRMWARE_DIR="$ROOT/firmware"
FLASHER_BIN_DIR="$ROOT/flasher/bin"

mkdir -p "$FLASHER_BIN_DIR"

# Wi-Fi credentials are optional — leaving them empty produces an "offline"
# firmware that simply skips the boot-time NTP fetch.
export SSID="${SSID:-}"
export WIFI_PASSWORD="${WIFI_PASSWORD:-}"

echo ">>> cargo build --release"
(
    cd "$FIRMWARE_DIR"
    cargo build --release
)

ELF="$FIRMWARE_DIR/target/xtensa-esp32-none-elf/release/ulanzi-pomodoro"
if [ ! -f "$ELF" ]; then
    echo "ELF not found at $ELF" >&2
    exit 1
fi

echo ">>> espflash save-image --merge"
espflash save-image \
    --chip esp32 \
    --merge \
    --partition-table "$FIRMWARE_DIR/partitions.csv" \
    "$ELF" \
    "$FLASHER_BIN_DIR/firmware.bin"

echo ">>> done: $FLASHER_BIN_DIR/firmware.bin ($(stat -f%z "$FLASHER_BIN_DIR/firmware.bin" 2>/dev/null || stat -c%s "$FLASHER_BIN_DIR/firmware.bin") bytes)"

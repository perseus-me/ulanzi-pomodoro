# ulanzi-pomodoro

Custom Rust firmware for the [Ulanzi TC001 pixel clock](https://www.ulanzi.com/products/ulanzi-pixel-smart-clock-2882) that turns it into a self-contained Pomodoro timer with three configurable presets and on-device statistics.

> **Status:** initial implementation done. Has not been smoke-tested on hardware yet — the host machine this was written on doesn't have the ESP Rust toolchain installed. Treat 0.1.0 as "compiles, lints clean, ready for first flash".

## Features

- Three preset slots (defaults: `25/5`, `50/10`, `90/20`) bound to the three front-panel buttons.
- Countdown screen with `MM:SS`, phase letter (`W` / `R`) and a progress bar.
- Idle screen showing today's count and a 7-day bar chart.
- Settings menu (long-press SELECT) to adjust brightness and per-preset work/rest minutes; saves on exit.
- Rolling 32-day statistics with two A/B copies and CRC32 for atomic writes.
- One-shot NTP fetch at boot (when Wi-Fi credentials are baked in); the radio is shut down immediately after to free memory.
- Factory reset: hold LEFT for ~1.5 s while booting to wipe NVS and revert to defaults.
- Browser-based flashing via [ESP Web Tools](https://esphome.github.io/esp-web-tools/) on GitHub Pages.

## Controls

Three front-panel buttons: **LEFT** (GPIO 26), **SELECT** (centre, GPIO 27),
**RIGHT** (GPIO 14). Each one generates `Press` (≤ 700 ms tap), `LongPress`
(≥ 700 ms hold) and `Repeat` (auto-fire every 150 ms while held, only used in
the menu editing mode).

### Idle screen

Shows today's completed count and a 7-day bar chart. The rightmost bar is
today and blinks at 1 Hz. A green pixel in the top-right corner means the
clock is anchored to real wall-clock time via NTP.

| Button | Press                      | Long press         |
|--------|----------------------------|--------------------|
| LEFT   | start preset 1 (25 / 5 by default)   | —              |
| SELECT | start preset 2 (50 / 10)             | open menu       |
| RIGHT  | start preset 3 (90 / 20)             | —              |

### Running (W) / Resting (R) screen

Shows `MM:SS` countdown, the `W` / `R` phase letter, and a progress bar.
Orange-red during work, green during rest. Paused = dim + slow blink.

| Button | Press                | Long press                                 |
|--------|----------------------|--------------------------------------------|
| LEFT   | toggle pause         | —                                          |
| SELECT | toggle pause         | during work → stop session; during rest → skip to idle |
| RIGHT  | toggle pause         | —                                          |

Audio cues: click on session start, three short pips when work finishes, one
beep when rest finishes.

### Menu

Reached from Idle via long-press SELECT. The dot row at the top shows the
selection cursor, the label and value are drawn underneath. Items:

| idx | label | what                | range          | step |
|-----|-------|---------------------|----------------|------|
|  0  | BRT   | matrix brightness   | 8…255          | 16   |
|  1  | P1W   | preset 1 work       | 1…240 min      | 5    |
|  2  | P1R   | preset 1 rest       | 1…240 min      | 5    |
|  3  | P2W   | preset 2 work       | 1…240 min      | 5    |
|  4  | P2R   | preset 2 rest       | 1…240 min      | 5    |
|  5  | P3W   | preset 3 work       | 1…240 min      | 5    |
|  6  | P3R   | preset 3 rest       | 1…240 min      | 5    |

The menu has two sub-modes — *navigation* and *editing* (the value blinks
while editing).

| Button | Navigation                  | Editing                       |
|--------|-----------------------------|-------------------------------|
| LEFT   | prev item                   | decrement (Press or Repeat)   |
| RIGHT  | next item                   | increment (Press or Repeat)   |
| SELECT | enter editing mode          | leave editing (stay in menu)  |
| SELECT (long) | exit menu & save     | exit menu & save              |

Changes take effect immediately on the matrix; persistence to NVS happens on
menu exit (long-press SELECT).

### Boot-time

- **Factory reset.** Hold **LEFT** while powering the clock on. A red bar
  fills from the bottom; if you keep holding for ~1.5 s the NVS partition is
  wiped and the device boots back to default presets and empty stats. Release
  early to abort.
- The recessed **RESET** button (GPIO 13) is a hardware reset for the ESP32
  and is not observed by the firmware.

### Offline behaviour

Booting without Wi-Fi credentials (or with the access point out of reach)
skips the single NTP fetch — the device runs normally, but the sync dot stays
off and the 7-day chart pins to the last known date. Today's counter is
preserved across reboots as long as the clock manages to sync once; if it
never syncs the counter is attributed to the day of the last successful sync.

## Hardware (Ulanzi TC001)

| ESP32 GPIO | Role                                          |
|------------|-----------------------------------------------|
| 32         | WS2812B chain for the 32x8 matrix             |
| 26         | LEFT button                                   |
| 27         | SELECT button                                 |
| 14         | RIGHT button                                  |
| 15         | Piezo buzzer                                  |
| 34         | Battery sense (ADC1_CH6) — unused for MVP     |
| 35         | LDR / ambient light (ADC1_CH7) — unused for MVP |
| 21 / 22    | I2C SDA / SCL (SHT3x temp/humidity) — unused for MVP |
| 13         | Hidden reset button                           |

The board uses an **ESP32-WROOM** (Xtensa LX6, classic ESP32) — not an STM32.

## Repository layout

```
firmware/   Rust no_std crate (target xtensa-esp32-none-elf)
flasher/    Static page deployed to GitHub Pages with ESP Web Tools
scripts/    Local build & release helpers
.github/    Release workflow (firmware build + Pages deploy on tag v*)
```

## Display layout

The 32x8 matrix is driven over RMT in row-major zig-zag order (pixel (0,0) is the top-left; even rows go left-to-right, odd rows right-to-left). The [`display`](firmware/src/display/mod.rs) module exposes both a `set_pixel` API and a full `embedded-graphics::DrawTarget<Color = Rgb888>` implementation, so the UI screens can compose with the standard `embedded_graphics` primitives.

When the clock has been anchored to a real time via NTP, a small green dot in the top-right corner is drawn on every screen.

## Building the firmware

1. Install the Espressif Rust toolchain (one-time, see [esp-rs/rust-build](https://github.com/esp-rs/rust-build)):
   ```sh
   curl -L https://github.com/esp-rs/espup/releases/latest/download/espup-aarch64-apple-darwin -o espup
   chmod +x espup
   ./espup install
   source ~/export-esp.sh
   cargo install espflash --locked
   ```
   On Linux/x86_64 use the matching `espup` binary.
2. Build:
   ```sh
   cd firmware
   SSID=myssid WIFI_PASSWORD=mypass cargo build --release
   ```
   The Wi-Fi credentials are only used for the single NTP fetch at boot. Building with both empty (`SSID="" WIFI_PASSWORD=""` or just unset) yields a fully functional "offline" firmware that skips the NTP step.
3. Flash directly to a connected TC001:
   ```sh
   cargo run --release
   ```

## Browser installer

The [`flasher/`](flasher/) directory is deployed to GitHub Pages by the [release workflow](.github/workflows/release.yml). Visit the published page in Chrome or Edge, plug the TC001 in over USB and click *Install*. Web Serial is unsupported on Firefox/Safari and iOS.

The merged binary is produced with [`scripts/build-release.sh`](scripts/build-release.sh), which boils down to:

```sh
cd firmware
cargo build --release
espflash save-image --chip esp32 --merge \
    --partition-table partitions.csv \
    target/xtensa-esp32-none-elf/release/ulanzi-pomodoro \
    ../flasher/bin/firmware.bin
```

## Persistent storage

The 24 KB `nvs` partition is sliced into 4 KB sectors that this firmware manages directly (i.e. not using ESP-IDF NVS):

```text
  0x9000  Settings copy A   (presets, brightness, tz, last_seen_unix)
  0xA000  Settings copy B
  0xB000  Stats copy A      (32-day ring of DayStats)
  0xC000  Stats copy B
  0xD000  reserved
  0xE000  reserved
```

Each slot starts with a 20-byte header (magic + version + length + monotonic sequence + CRC32 of header+payload). On load we pick the slot with the higher sequence that also passes CRC; on save we overwrite the *other* slot. This gives us atomic writes that survive a power-cut mid-flush.

## Acknowledgements

- The WS2812 + framebuffer driver in [`firmware/src/display/mod.rs`](firmware/src/display/mod.rs) is ported from [cpa/ulanzi-tc001](https://github.com/cpa/ulanzi-tc001).
- Pinout and hardware notes are based on [Blueforcer/awtrix3](https://github.com/blueforcer/awtrix3).
- Date math uses Howard Hinnant's [civil_from_days algorithm](https://howardhinnant.github.io/date_algorithms.html#civil_from_days).

## License

Dual-licensed under either of:

- Apache License, Version 2.0
- MIT license

at your option.

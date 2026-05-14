#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use embedded_graphics::{
    Drawable,
    mono_font::{MonoTextStyle, ascii::FONT_4X6},
    pixelcolor::Rgb888,
    prelude::Point,
    text::{Baseline, Text},
};
use embedded_storage::nor_flash::NorFlash;
use esp_hal::{
    clock::CpuClock,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    rmt::{Rmt, TxChannelConfig, TxChannelCreator},
    time::{Duration, Instant, Rate},
    timer::timg::TimerGroup,
};
use esp_println::{logger, println};
use esp_storage::FlashStorage;

use ulanzi_pomodoro::{
    display::{
        FrameBuffer, MATRIX_DATA_GPIO, MatrixDisplay, PixelMapping, PixelSink, RgbColor,
        Ws2812Sink, Ws2812Timing,
    },
    input::{ButtonEvent, Buttons},
    pomodoro::{
        PhaseTransition, Pomodoro,
        presets::{MAX_MINUTES, MIN_MINUTES},
    },
    storage::{
        SETTINGS_OFFSET_A, SLOT_SIZE, STATS_OFFSET_A, Settings, Stats,
        stats::StatsStore,
    },
    time::Clock,
    ui::{RenderContext, Ui, UiAction, notify::Notifier},
    wifi,
};

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("panic: {}", info);
    loop {}
}

esp_bootloader_esp_idf::esp_app_desc!();

const SSID: &str = match option_env!("SSID") {
    Some(s) => s,
    None => "",
};
const WIFI_PASSWORD: &str = match option_env!("WIFI_PASSWORD") {
    Some(p) => p,
    None => "",
};
const NTP_BUDGET_MS: u64 = 10_000;

const FRAME_PERIOD_MS: u64 = 33;
const LAST_SEEN_SAVE_PERIOD_MS: u64 = 5 * 60_000;
const MIN_BRIGHTNESS: u8 = 8;
const SPLASH_INITIAL_BRIGHTNESS: u8 = 64;
const FACTORY_RESET_HOLD_MS: u32 = 1_500;

#[esp_hal::main]
fn main() -> ! {
    logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 36 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    println!("ulanzi-pomodoro v{} booting", env!("CARGO_PKG_VERSION"));

    let buzzer = Output::new(peripherals.GPIO15, Level::Low, OutputConfig::default());
    let mut notifier = Notifier::new(buzzer);

    let btn_cfg = InputConfig::default().with_pull(Pull::Up);
    let left = Input::new(peripherals.GPIO26, btn_cfg);
    let select = Input::new(peripherals.GPIO27, btn_cfg);
    let right = Input::new(peripherals.GPIO14, btn_cfg);

    let rmt_clock = Rate::from_mhz(80);
    let rmt = Rmt::new(peripherals.RMT, rmt_clock).expect("init RMT peripheral");
    let tx_config = TxChannelConfig::default()
        .with_clk_divider(1)
        .with_idle_output(true)
        .with_idle_output_level(Level::Low)
        .with_memsize(2);
    let tx_channel = rmt
        .channel0
        .configure_tx(peripherals.GPIO32, tx_config)
        .expect("configure RMT for WS2812 on GPIO32");
    debug_assert_eq!(MATRIX_DATA_GPIO, 32);

    let sink = Ws2812Sink::new(tx_channel, rmt_clock, Ws2812Timing::ws2812_800khz());
    let mut display =
        MatrixDisplay::new(sink, PixelMapping::RowZigZag, SPLASH_INITIAL_BRIGHTNESS);

    let mut flash = FlashStorage::new(peripherals.FLASH);

    let factory_reset = left.is_low() && confirm_factory_reset(&left, &mut display);
    if factory_reset {
        println!("factory reset: erasing NVS");
        if let Err(err) = flash.erase(SETTINGS_OFFSET_A, STATS_OFFSET_A + 2 * SLOT_SIZE) {
            println!("nvs: erase failed: {:?}", err);
        } else {
            // Quick "OK" flash so the user knows it took effect.
            display.fill(RgbColor::new(0, 255, 60));
            let _ = display.flush();
            wait_ms(400);
            display.clear();
            let _ = display.flush();
            notifier.click();
        }
    }

    let (mut settings, mut settings_store) = Settings::load(&mut flash);
    let (mut stats, mut stats_store): (Stats, StatsStore) = Stats::load(&mut flash);
    println!(
        "settings: brightness={}, tz_min={}, last_seen={}",
        settings.brightness, settings.tz_offset_min, settings.last_seen_unix
    );

    let mut buttons = Buttons::new(left, select, right);

    play_boot_animation(&mut display, settings.brightness.max(MIN_BRIGHTNESS));

    let mut clock = Clock::from_last_seen(settings.last_seen_unix, now_ms());

    if let Some(unix) =
        wifi::try_fetch_time(peripherals.WIFI, SSID, WIFI_PASSWORD, NTP_BUDGET_MS)
    {
        let anchored_at = now_ms();
        clock.anchor(unix, anchored_at);
        if unix != settings.last_seen_unix {
            settings.last_seen_unix = unix;
            if let Err(err) = settings_store.save(&mut flash, &settings) {
                println!("nvs: settings save after NTP failed: {:?}", err);
            }
        }
    }

    let mut pomodoro = Pomodoro::new(settings.presets);
    let mut ui = Ui::new();
    let mut events: Vec<ButtonEvent> = Vec::with_capacity(8);
    let mut settings_dirty = false;

    let mut next_frame_ms = now_ms();
    let mut next_last_seen_save_ms = next_frame_ms.saturating_add(LAST_SEEN_SAVE_PERIOD_MS);
    notifier.click();

    loop {
        let now = now_ms();
        let today = clock.today_yyyymmdd(now, settings.tz_offset_min);

        events.clear();
        buttons.poll(now, &mut events);
        for ev in events.drain(..) {
            let ctx = RenderContext {
                pomodoro: &pomodoro,
                settings: &settings,
                stats: &stats,
                today,
                time_synced: clock.synced,
                now_ms: now,
            };
            let Some(action) = ui.handle(ev, &ctx) else {
                continue;
            };
            match action {
                UiAction::StartPreset(idx) => {
                    if pomodoro.start(idx, now) {
                        println!("pomodoro: start preset {}", idx);
                        notifier.click();
                    }
                }
                UiAction::TogglePause => {
                    pomodoro.toggle_pause(now);
                    notifier.click();
                }
                UiAction::StopSession => {
                    pomodoro.stop();
                    notifier.click();
                }
                UiAction::SkipRest => {
                    pomodoro.skip(now);
                    notifier.click();
                }
                UiAction::EnterMenu => {
                    notifier.click();
                }
                UiAction::ExitMenu => {
                    notifier.click();
                    if settings_dirty {
                        if let Err(err) = settings_store.save(&mut flash, &settings) {
                            println!("nvs: settings save on menu exit failed: {:?}", err);
                        } else {
                            settings_dirty = false;
                        }
                    }
                }
                UiAction::AdjustBrightness(delta) => {
                    let new_b = clamp_i16(
                        settings.brightness as i16 + delta,
                        MIN_BRIGHTNESS as i16,
                        255,
                    ) as u8;
                    if new_b != settings.brightness {
                        settings.brightness = new_b;
                        display.set_brightness(new_b);
                        settings_dirty = true;
                    }
                }
                UiAction::AdjustWork {
                    preset_idx,
                    delta_min,
                } => {
                    if let Some(preset) = settings.presets.get_mut(preset_idx as usize) {
                        let new = clamp_i16(
                            preset.work_min as i16 + delta_min,
                            MIN_MINUTES as i16,
                            MAX_MINUTES as i16,
                        ) as u16;
                        if new != preset.work_min {
                            preset.work_min = new;
                            settings_dirty = true;
                            if let Some(p) =
                                pomodoro.presets_mut().get_mut(preset_idx as usize)
                            {
                                p.work_min = new;
                            }
                        }
                    }
                }
                UiAction::AdjustRest {
                    preset_idx,
                    delta_min,
                } => {
                    if let Some(preset) = settings.presets.get_mut(preset_idx as usize) {
                        let new = clamp_i16(
                            preset.rest_min as i16 + delta_min,
                            MIN_MINUTES as i16,
                            MAX_MINUTES as i16,
                        ) as u16;
                        if new != preset.rest_min {
                            preset.rest_min = new;
                            settings_dirty = true;
                            if let Some(p) =
                                pomodoro.presets_mut().get_mut(preset_idx as usize)
                            {
                                p.rest_min = new;
                            }
                        }
                    }
                }
            }
        }

        if let Some(transition) = pomodoro.tick(now) {
            match transition {
                PhaseTransition::WorkFinished => {
                    println!("pomodoro: work finished");
                    if let Some(completed) = pomodoro.take_completed_work() {
                        if stats.record_completed_work_best_effort(today, completed.minutes) {
                            if today.is_none() {
                                println!("pomodoro: no clock anchor, crediting current stats slot");
                            }
                            if let Err(err) = stats_store.save(&mut flash, &stats) {
                                println!("nvs: stats save failed: {:?}", err);
                            }
                            // Stamp last_seen_unix alongside the stats commit so
                            // a power-cut right after this won't lose the date.
                            if let Some(unix) = clock.now_unix(now) {
                                if unix > settings.last_seen_unix {
                                    settings.last_seen_unix = unix;
                                    if let Err(err) = settings_store.save(&mut flash, &settings) {
                                        println!(
                                            "nvs: settings save after stats failed: {:?}",
                                            err
                                        );
                                    }
                                }
                            }
                        }
                    }
                    notifier.finish();
                }
                PhaseTransition::RestFinished => {
                    println!("pomodoro: rest finished");
                    notifier.beep(200);
                }
            }
        }

        ui.sync_to_state(pomodoro.state());

        let ctx = RenderContext {
            pomodoro: &pomodoro,
            settings: &settings,
            stats: &stats,
            today,
            time_synced: clock.synced,
            now_ms: now,
        };
        ui.render(display.framebuffer_mut(), &ctx);
        if clock.synced {
            draw_sync_dot(display.framebuffer_mut());
        }
        if let Err(err) = display.flush() {
            println!("matrix flush failed: {:?}", err);
        }

        if now >= next_last_seen_save_ms {
            if let Some(unix) = clock.now_unix(now) {
                if unix > settings.last_seen_unix {
                    settings.last_seen_unix = unix;
                    if let Err(err) = settings_store.save(&mut flash, &settings) {
                        println!("nvs: periodic settings save failed: {:?}", err);
                    }
                }
            }
            next_last_seen_save_ms = now.saturating_add(LAST_SEEN_SAVE_PERIOD_MS);
        }

        next_frame_ms = next_frame_ms.saturating_add(FRAME_PERIOD_MS);
        let now_after = now_ms();
        if next_frame_ms > now_after {
            wait_ms((next_frame_ms - now_after) as u32);
        } else {
            next_frame_ms = now_after;
        }
    }
}

/// Wait for the LEFT button to remain held for ~1.5 seconds while showing
/// a red wash. Returns `true` if the user kept holding all the way through.
fn confirm_factory_reset<S: PixelSink>(left: &Input<'_>, display: &mut MatrixDisplay<S>) -> bool {
    let start = Instant::now();
    let target = Duration::from_millis(FACTORY_RESET_HOLD_MS as u64);
    while start.elapsed() < target {
        if !left.is_low() {
            return false;
        }
        // Fill bottom-up red bar to convey progress.
        let progress_ms = start.elapsed().as_millis() as u32;
        let height = (progress_ms * 8 / FACTORY_RESET_HOLD_MS).min(8) as usize;
        display.clear();
        for y in (8 - height)..8 {
            for x in 0..32 {
                display.set_pixel(x, y, RgbColor::new(200, 30, 30));
            }
        }
        let _ = display.flush();
        wait_ms(50);
    }
    left.is_low()
}

/// 250 ms fade-in of the boot splash from black up to the user's brightness.
fn play_boot_animation<S: PixelSink>(display: &mut MatrixDisplay<S>, final_brightness: u8) {
    const STEPS: u8 = 12;
    for step in 1..=STEPS {
        let b = ((step as u32) * (final_brightness as u32) / STEPS as u32) as u8;
        display.set_brightness(b.max(MIN_BRIGHTNESS));
        draw_boot_splash(display);
        let _ = display.flush();
        wait_ms(20);
    }
    display.set_brightness(final_brightness);
    draw_boot_splash(display);
    let _ = display.flush();
}

fn draw_boot_splash<S: PixelSink>(display: &mut MatrixDisplay<S>) {
    display.clear();
    let style = MonoTextStyle::new(&FONT_4X6, Rgb888::new(200, 80, 0));
    let _ = Text::with_baseline("POMO", Point::new(2, 0), style, Baseline::Top)
        .draw(display.framebuffer_mut());
    let dot = RgbColor::new(80, 80, 120);
    for x in 19..=23 {
        display.set_pixel(x, 5, dot);
    }
}

fn draw_sync_dot(fb: &mut FrameBuffer) {
    fb.set_pixel(31, 0, RgbColor::new(0, 120, 80));
}

fn clamp_i16(v: i16, lo: i16, hi: i16) -> i16 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

fn now_ms() -> u64 {
    Instant::now().duration_since_epoch().as_millis()
}

fn wait_ms(ms: u32) {
    if ms == 0 {
        return;
    }
    let start = Instant::now();
    let target = Duration::from_millis(ms as u64);
    while start.elapsed() < target {
        core::hint::spin_loop();
    }
}

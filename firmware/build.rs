fn main() {
    linker_be_nice();
    // Make sure linkall.x is the last linker script (otherwise might cause
    // problems with flip-link).
    println!("cargo:rustc-link-arg=-Tlinkall.x");

    // Re-build whenever the partitions table changes so that the binary
    // bundled with `espflash save-image --merge` is kept in sync.
    println!("cargo:rerun-if-changed=partitions.csv");

    // Wi-Fi credentials are passed at build time. We deliberately accept
    // empty strings so a developer can produce a flashable binary without
    // any network configured.
    println!("cargo:rerun-if-env-changed=SSID");
    println!("cargo:rerun-if-env-changed=WIFI_PASSWORD");
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                "_defmt_timestamp" => {
                    eprintln!();
                    eprintln!(
                        "tip: `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("tip: is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                "esp_rtos_initialized" | "esp_rtos_yield_task" | "esp_rtos_task_create" => {
                    eprintln!();
                    eprintln!(
                        "tip: `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler."
                    );
                    eprintln!();
                }
                _ => (),
            },
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}

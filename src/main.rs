// SPDX-License-Identifier: GPL-3.0-or-later
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = run() {
        eprintln!("MyDigitalBuddy: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "MyDigitalBuddy 0.1.0\n\nDesktop companion powered by a local 470-neuron LIF network.\n\n  --headless [SECONDS]   Simulate without a display\n  --seed N               Reproducible simulation seed\n  --data-dir PATH        Local memory directory\n  --content-dir PATH     Custom pet definitions and local images\n  --pet ID               Pet definition (bryan / jorge / bia / sophia / custom)\n  --test-mode            Accelerated behavior\n  --direto               Skip onboarding, use saved permissions\n  --import-legacy PATH   Import a Pestinha memory directory without modifying it\n  --license              Show the license\n"
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--license") {
        println!(
            "GPL-3.0-or-later, WITHOUT ANY WARRANTY.\n\n{}",
            include_str!("../LICENSE")
        );
        return Ok(());
    }
    let value = |key: &str| {
        args.iter()
            .position(|a| a == key)
            .and_then(|i| args.get(i + 1))
    };
    let data_dir = value("--data-dir")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(mydigitalbuddy::storage::default_data_dir);
    let seed: u64 = value("--seed")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or_else(|| {
            (mydigitalbuddy::storage::wall_time() * 1_000_000.0) as u64 ^ std::process::id() as u64
        });
    let content = value("--content-dir").map(std::path::PathBuf::from);
    let mut engine = mydigitalbuddy::engine::Engine::load_with_content(data_dir, seed, content)?;
    if let Some(pet) = value("--pet") {
        if value("--import-legacy").is_some() {
            engine.select_pet_for_import(pet)?;
        } else {
            engine.switch_pet(pet)?;
        }
    }
    if args.iter().any(|a| a == "--test-mode") {
        engine.settings.test_mode = true;
    }
    if let Some(path) = value("--import-legacy") {
        let report = engine.import_legacy(std::path::Path::new(path))?;
        println!("{report}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--headless") {
        let seconds: f64 = value("--headless")
            .filter(|s| !s.starts_with("--"))
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or(10.0);
        if !seconds.is_finite() || !(0.0..=86400.0).contains(&seconds) {
            return Err("SECONDS must be finite and between 0 and 86400".into());
        }
        let input = mydigitalbuddy::platform::Snapshot::default();
        for _ in 0..(seconds * 60.0) as usize {
            engine.advance(1.0 / 60.0, &input);
        }
        engine.save()?;
        println!("{}", serde_json::to_string_pretty(&engine.summary())?);
        return Ok(());
    }
    #[cfg(feature = "desktop")]
    {
        mydigitalbuddy::desktop::run(engine, args.iter().any(|a| a == "--direto"))?;
    }
    #[cfg(not(feature = "desktop"))]
    {
        return Err(
            "Desktop support disabled; build with default features or use --headless".into(),
        );
    }
    #[allow(unreachable_code)]
    Ok(())
}

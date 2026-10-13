use flux_pro::{
    Health,
    hid::{self, Hid},
    report,
    sensors::{self, Gpu, Hwmon},
};
use signal_hook::{
    consts::{SIGINT, SIGTERM},
    iterator::Signals,
};
use std::{
    env,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Args {
    cpu: String,
    gpu: String,
    device: Option<PathBuf>,
    interval: Duration,
    once: bool,
    dry: bool,
    list: bool,
}
impl Args {
    fn parse(input: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut args = Self {
            cpu: "auto".into(),
            gpu: "nvidia:auto".into(),
            device: None,
            interval: Duration::from_secs(1),
            once: false,
            dry: false,
            list: false,
        };
        let mut iter = input;
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    println!(
                        "flux-pro {}\nCPU/GPU temperature display daemon (direct NVML + hidraw).\n\n  --list            List hwmon sensors, NVIDIA GPUs, and display devices\n  --cpu SELECTOR    auto, DRIVER:LABEL, or absolute temp input path\n  --gpu SELECTOR    nvidia:auto, nvidia:PCI_ADDRESS, nvidia:GPU-UUID, or hwmon:DRIVER:LABEL\n                    Default: nvidia:auto (exactly one NVIDIA GPU)\n  --device PATH     Select one matching HID device\n  --interval SEC    Update period, 0.1–3600 seconds (default 1)\n  --once            Send one reading and exit\n  --dry-run         Print readings without opening the display\n  --version         Print version",
                        env!("CARGO_PKG_VERSION")
                    );
                    return Ok(None);
                }
                "--version" => {
                    println!("flux-pro {}", env!("CARGO_PKG_VERSION"));
                    return Ok(None);
                }
                "--list" => args.list = true,
                "--once" => args.once = true,
                "--dry-run" => args.dry = true,
                "--cpu" | "--gpu" | "--device" | "--interval" => {
                    let value = iter.next().ok_or_else(|| format!("{arg} needs a value"))?;
                    match arg.as_str() {
                        "--cpu" => args.cpu = value,
                        "--gpu" => args.gpu = value,
                        "--device" => args.device = Some(value.into()),
                        _ => {
                            let seconds: f64 = value.parse().map_err(|_| "invalid interval")?;
                            if !seconds.is_finite() || !(0.1..=3600.).contains(&seconds) {
                                return Err("interval must be 0.1–3600 seconds".into());
                            }
                            args.interval = Duration::from_secs_f64(seconds);
                        }
                    }
                }
                _ => return Err(format!("unknown argument {arg}; use --help")),
            }
        }
        Ok(Some(args))
    }
}

fn list() {
    for sensor in sensors::discover(Path::new("/sys/class/hwmon")) {
        println!("hwmon {} | {}", sensor.selector(), sensor.path.display());
    }
    match nvml_wrapper::Nvml::init() {
        Ok(nvml) => {
            for i in 0..nvml.device_count().unwrap_or(0) {
                if let Ok(d) = nvml.device_by_index(i) {
                    println!(
                        "NVIDIA {:?} | {:?} | {:?}",
                        d.name(),
                        d.pci_info().map(|p| p.bus_id),
                        d.uuid()
                    );
                }
            }
        }
        Err(e) => eprintln!("NVIDIA unavailable: {e}"),
    }
    for device in hid::discover(Path::new("/sys/class/hidraw")) {
        println!(
            "Flux Pro {} (node {})",
            device.display(),
            if device.exists() { "present" } else { "absent" }
        );
    }
}

fn temperature(value: Option<i64>) -> String {
    value
        .map(|v| format!("{}.{:01}", v / 1000, (v % 1000) / 100))
        .unwrap_or_else(|| "missing".into())
}

fn run(args: Args) -> Result<bool, String> {
    if args.list {
        list();
        return Ok(true);
    }
    let mut cpu = Hwmon::new(&args.cpu);
    let mut gpu = Gpu::new(&args.gpu)?;
    let mut display = Hid::new(args.device);
    let mut cpu_health = Health::default();
    let mut gpu_health = Health::default();
    let mut display_health = Health::default();
    let stopped = Arc::new(AtomicBool::new(false));
    let mut signals = Signals::new([SIGINT, SIGTERM]).map_err(|e| e.to_string())?;
    let handle = signals.handle();
    let main_thread = thread::current();
    let stop_flag = stopped.clone();
    let signal_thread = thread::spawn(move || {
        if signals.forever().next().is_some() {
            stop_flag.store(true, Ordering::Relaxed);
            main_thread.unpark();
        }
    });
    let mut ok = true;
    while !stopped.load(Ordering::Relaxed) {
        let started = Instant::now();
        let cpu = cpu_health.observe("CPU", cpu.read());
        let gpu = gpu_health.observe("GPU", gpu.read());
        let packet = report(cpu, gpu);
        ok = cpu.is_some() && gpu.is_some();
        if args.dry {
            println!(
                "CPU={} GPU={} payload={}",
                temperature(cpu),
                temperature(gpu),
                packet[1..13]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        } else {
            ok &= display_health
                .observe("display", display.write(&packet))
                .is_some();
        }
        if args.once {
            break;
        }
        let deadline = started + args.interval;
        while !stopped.load(Ordering::Relaxed) && Instant::now() < deadline {
            thread::park_timeout(deadline.saturating_duration_since(Instant::now()));
        }
    }
    if stopped.load(Ordering::Relaxed) && !args.dry {
        display_health.observe("shutdown clear", display.write(&report(None, None)));
    }
    handle.close();
    signal_thread
        .join()
        .map_err(|_| "signal handler thread failed")?;
    Ok(if args.once { ok } else { true })
}

fn main() -> ExitCode {
    match Args::parse(env::args().skip(1)).and_then(|a| a.map(run).unwrap_or(Ok(true))) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("flux-pro: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_cli_does_not_reach_hardware() {
        for input in [
            vec!["--unknown"],
            vec!["--cpu"],
            vec!["--interval", "NaN"],
            vec!["--interval", "0"],
            vec!["--interval", "inf"],
            vec!["--interval", "999999999999"],
        ] {
            assert!(Args::parse(input.into_iter().map(str::to_owned)).is_err());
        }
    }
}

use nvml_wrapper::{Nvml, enum_wrappers::device::TemperatureSensor};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    driver: String,
    label: String,
    device: PathBuf,
    input: String,
}

#[derive(Clone, Debug)]
pub struct Sensor {
    identity: Identity,
    pub path: PathBuf,
}

impl Sensor {
    pub fn selector(&self) -> String {
        format!("{}:{}", self.identity.driver, self.identity.label)
    }

    fn read(&self) -> Result<i64, String> {
        // Check identity even for a cached path; hwmon numbers can be reused.
        let current = identity(&self.path).ok_or("sensor disappeared")?;
        if current != self.identity {
            return Err("cached sensor identity changed".into());
        }
        let fault = self
            .path
            .with_file_name(self.identity.input.replace("_input", "_fault"));
        match fs::read_to_string(fault) {
            Ok(value) if value.trim() != "0" => return Err("hardware fault flag set".into()),
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.to_string());
            }
            _ => {}
        }
        checked(
            fs::read_to_string(&self.path)
                .map_err(|e| e.to_string())?
                .trim()
                .parse()
                .map_err(|_| "invalid hwmon temperature")?,
        )
    }
}

fn identity(path: &Path) -> Option<Identity> {
    let parent = path.parent()?;
    let input = path.file_name()?.to_str()?.to_owned();
    let driver = fs::read_to_string(parent.join("name"))
        .ok()?
        .trim()
        .to_owned();
    let label = fs::read_to_string(parent.join(input.replace("_input", "_label")))
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| input.replace("_input", ""));
    let device = parent.join("device").canonicalize().ok()?;
    Some(Identity {
        driver,
        label,
        device,
        input,
    })
}

pub fn discover(root: &Path) -> Vec<Sensor> {
    let mut result = Vec::new();
    if let Ok(dirs) = fs::read_dir(root) {
        for dir in dirs
            .flatten()
            .filter(|d| d.file_name().to_string_lossy().starts_with("hwmon"))
        {
            if let Ok(files) = fs::read_dir(dir.path()) {
                for file in files.flatten() {
                    let name = file.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with("temp")
                        && name.ends_with("_input")
                        && let Some(identity) = identity(&file.path())
                    {
                        result.push(Sensor {
                            identity,
                            path: file.path(),
                        });
                    }
                }
            }
        }
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    result
}

pub fn checked(value: i64) -> Result<i64, String> {
    if (0..=99_900).contains(&value) {
        Ok(value)
    } else {
        Err(format!(
            "{value} millidegrees is outside display range 0–99900"
        ))
    }
}

pub struct Hwmon {
    selector: String,
    root: PathBuf,
    pinned: Option<Identity>,
    cached: Option<Sensor>,
}

impl Hwmon {
    pub fn new(selector: &str) -> Self {
        Self {
            selector: selector.into(),
            root: "/sys/class/hwmon".into(),
            pinned: None,
            cached: None,
        }
    }

    pub fn read(&mut self) -> Result<i64, String> {
        if let Some(sensor) = &self.cached {
            match sensor.read() {
                Ok(value) => return Ok(value),
                Err(_) => self.cached = None,
            }
        }
        let matches: Vec<_> = discover(&self.root)
            .into_iter()
            .filter(|s| {
                if let Some(pinned) = &self.pinned {
                    return &s.identity == pinned;
                }
                if self.selector == "auto" {
                    (s.identity.driver == "k10temp" && s.identity.label == "Tctl")
                        || (s.identity.driver == "coretemp"
                            && s.identity.label.starts_with("Package id "))
                } else if self.selector.starts_with('/') {
                    s.path
                        .canonicalize()
                        .ok()
                        .zip(Path::new(&self.selector).canonicalize().ok())
                        .is_some_and(|(a, b)| a == b)
                } else {
                    s.selector() == self.selector
                }
            })
            .collect();
        if matches.len() != 1 {
            return Err(format!(
                "sensor {:?}: expected one match, found {}; use --list",
                self.selector,
                matches.len()
            ));
        }
        let sensor = matches.into_iter().next().unwrap();
        self.pinned = Some(sensor.identity.clone());
        let result = sensor.read();
        self.cached = Some(sensor);
        result
    }
}

pub fn normalize_pci(value: &str) -> Result<String, String> {
    let fields: Vec<_> = value.split([':', '.']).collect();
    if fields.len() != 4
        || ![4, 8].contains(&fields[0].len())
        || fields[1].len() != 2
        || fields[2].len() != 2
        || fields[3].len() != 1
        || fields
            .iter()
            .any(|v| !v.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(format!("invalid PCI address: {value}"));
    }
    let numbers: Vec<_> = fields
        .iter()
        .map(|v| u32::from_str_radix(v, 16).unwrap())
        .collect();
    if numbers[2] > 31 || numbers[3] > 7 {
        return Err(format!("invalid PCI address: {value}"));
    }
    Ok(format!(
        "{:04x}:{:02x}:{:02x}.{:x}",
        numbers[0], numbers[1], numbers[2], numbers[3]
    ))
}

pub enum Gpu {
    Nvidia {
        selector: String,
        nvml: Option<Box<Nvml>>,
    },
    Hwmon(Box<Hwmon>),
}

fn require_single_gpu(count: u32) -> Result<(), String> {
    if count == 1 {
        Ok(())
    } else {
        Err(format!(
            "nvidia:auto: expected one NVIDIA GPU, found {count}; use --list and select a UUID or PCI address"
        ))
    }
}

impl Gpu {
    pub fn new(selector: &str) -> Result<Self, String> {
        if let Some(value) = selector.strip_prefix("nvidia:") {
            if value.starts_with("GPU-") {
                if value.len() <= 4 {
                    return Err("empty NVIDIA UUID".into());
                }
            } else if value != "auto" {
                normalize_pci(value)?;
            }
            Ok(Self::Nvidia {
                selector: value.into(),
                nvml: None,
            })
        } else if let Some(value) = selector.strip_prefix("hwmon:") {
            Ok(Self::Hwmon(Box::new(Hwmon::new(value))))
        } else {
            Err(
                "GPU selector must be nvidia:auto, nvidia:PCI_ADDRESS, nvidia:GPU-UUID or hwmon:DRIVER:LABEL"
                    .into(),
            )
        }
    }

    pub fn read(&mut self) -> Result<i64, String> {
        match self {
            Self::Hwmon(reader) => reader.read(),
            Self::Nvidia { selector, nvml } => {
                if nvml.is_none() {
                    *nvml = Some(Box::new(Nvml::init().map_err(|e| format!("NVML: {e}"))?));
                }
                let library = nvml.as_ref().unwrap();
                let result = (|| {
                    if selector == "auto" {
                        require_single_gpu(library.device_count().map_err(|e| e.to_string())?)?;
                        // Pin the initial choice across driver reloads and reconnects.
                        *selector = library
                            .device_by_index(0)
                            .and_then(|device| device.uuid())
                            .map_err(|e| e.to_string())?;
                    }
                    let device = if selector.starts_with("GPU-") {
                        library.device_by_uuid(selector.as_str())
                    } else {
                        library.device_by_pci_bus_id(selector.as_str())
                    }
                    .map_err(|e| e.to_string())?;
                    let matches = if selector.starts_with("GPU-") {
                        device.uuid().map_err(|e| e.to_string())? == *selector
                    } else {
                        normalize_pci(&device.pci_info().map_err(|e| e.to_string())?.bus_id)?
                            == normalize_pci(selector)?
                    };
                    if !matches {
                        return Err("NVML returned a different GPU".into());
                    }
                    checked(
                        i64::from(
                            device
                                .temperature(TemperatureSensor::Gpu)
                                .map_err(|e| e.to_string())?,
                        ) * 1000,
                    )
                })();
                // Reinitialize after errors (driver reload/GPU loss); retain the library on success.
                if result.is_err() {
                    *nvml = None;
                }
                result
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    fn sensor(root: &Path, index: usize, device: &str, label: &str) -> PathBuf {
        let dir = root.join(format!("hwmon{index}"));
        fs::create_dir_all(&dir).unwrap();
        let physical = root.join(device);
        fs::create_dir_all(&physical).unwrap();
        symlink(physical, dir.join("device")).unwrap();
        fs::write(dir.join("name"), "k10temp").unwrap();
        fs::write(dir.join("temp1_label"), label).unwrap();
        fs::write(dir.join("temp1_input"), "42500").unwrap();
        dir
    }
    fn reader(root: &Path) -> Hwmon {
        let mut r = Hwmon::new("auto");
        r.root = root.into();
        r
    }

    #[test]
    fn startup_and_renumber_recovery() {
        let t = TempDir::new().unwrap();
        let mut r = reader(t.path());
        assert!(r.read().is_err());
        let d = sensor(t.path(), 1, "cpu0", "Tctl");
        assert_eq!(r.read().unwrap(), 42500);
        fs::write(d.join("temp1_input"), "46875").unwrap();
        assert_eq!(r.read().unwrap(), 46875);
        fs::rename(&d, t.path().join("old")).unwrap();
        sensor(t.path(), 2, "cpu0", "Tctl");
        assert_eq!(r.read().unwrap(), 42500);
    }
    #[test]
    fn cached_number_cannot_switch_cpu() {
        let t = TempDir::new().unwrap();
        let d = sensor(t.path(), 1, "cpu0", "Tctl");
        let mut r = reader(t.path());
        assert!(r.read().is_ok());
        fs::rename(d, t.path().join("old")).unwrap();
        sensor(t.path(), 1, "cpu1", "Tctl");
        assert!(r.read().is_err());
    }
    #[test]
    fn ambiguous_cpu_and_bad_readings() {
        let t = TempDir::new().unwrap();
        let d = sensor(t.path(), 1, "cpu0", "Tctl");
        sensor(t.path(), 2, "cpu1", "Tctl");
        assert!(reader(t.path()).read().is_err());
        let mut r = reader(t.path());
        r.selector = d.join("temp1_input").display().to_string();
        assert!(r.read().is_ok());
        for value in ["NaN", "bad", "-1", "100000", "99901"] {
            fs::write(d.join("temp1_input"), value).unwrap();
            assert!(r.read().is_err());
        }
        fs::write(d.join("temp1_input"), "55000").unwrap();
        fs::write(d.join("temp1_fault"), "1").unwrap();
        assert!(r.read().is_err());
        fs::write(d.join("temp1_fault"), "0").unwrap();
        assert_eq!(r.read().unwrap(), 55000);
    }
    #[test]
    fn gpu_selection_is_explicit() {
        assert!(Gpu::new("nvidia:auto").is_ok());
        assert!(require_single_gpu(1).is_ok());
        for count in [0, 2, 8] {
            assert!(require_single_gpu(count).is_err());
        }
        assert_eq!(normalize_pci("00000000:01:00.0").unwrap(), "0000:01:00.0");
        for s in [
            "nvidia:0",
            "nvidia:",
            "nvidia:0000:01:00.8",
            "nvidia:0000:01:ff.0",
            "nvidia:GPU-",
            "auto",
        ] {
            assert!(Gpu::new(s).is_err(), "{s}");
        }
        assert!(matches!(
            Gpu::new("nvidia:0000:01:00.0").unwrap(),
            Gpu::Nvidia { .. }
        ));
        assert!(matches!(
            Gpu::new("hwmon:amdgpu:edge").unwrap(),
            Gpu::Hwmon(_)
        ));
    }
}

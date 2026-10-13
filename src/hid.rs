use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};

pub const DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x00, 0xa1, 0x01, 0x15, 0x00, 0x25, 0xff, 0x19, 0x01, 0x29, 0x08, 0x95, 0x40,
    0x75, 0x08, 0x81, 0x02, 0x19, 0x01, 0x29, 0x08, 0x95, 0x40, 0x75, 0x08, 0x91, 0x02, 0xc0,
];
const HID_ID: &str = "HID_ID=0003:00002022:00000522";

pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut devices = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if let Ok(uevent) = fs::read_to_string(entry.path().join("device/uevent"))
                && uevent.lines().any(|line| line.eq_ignore_ascii_case(HID_ID))
            {
                devices.push(Path::new("/dev").join(entry.file_name()));
            }
        }
    }
    devices.sort();
    devices
}

#[repr(C)]
#[derive(Default)]
struct DeviceInfo {
    bus: u32,
    vendor: i16,
    product: i16,
}
#[repr(C)]
struct ReportDescriptor {
    size: u32,
    value: [u8; 4096],
}

fn validate(file: &File) -> io::Result<()> {
    let mut info = DeviceInfo::default();
    let mut descriptor_size: libc::c_int = 0;
    let mut descriptor = ReportDescriptor {
        size: 0,
        value: [0; 4096],
    };
    // SAFETY: Linux hidraw ioctls use these repr(C) layouts from linux/hidraw.h.
    // Both buffers are initialized, writable and alive for the synchronous calls.
    // The FD belongs to the File we subsequently write, avoiding a sysfs/open race.
    unsafe {
        if libc::ioctl(
            file.as_raw_fd(),
            libc::_IOR::<DeviceInfo>(b'H'.into(), 3),
            &mut info,
        ) < 0
        {
            return Err(io::Error::last_os_error());
        }
        if info.bus != 3 || info.vendor != 0x2022 || info.product != 0x0522 {
            return Err(io::Error::other("opened device is not the Flux Pro"));
        }
        if libc::ioctl(
            file.as_raw_fd(),
            libc::_IOR::<libc::c_int>(b'H'.into(), 1),
            &mut descriptor_size,
        ) < 0
        {
            return Err(io::Error::last_os_error());
        }
        if descriptor_size != DESCRIPTOR.len() as libc::c_int {
            return Err(io::Error::other("unrecognized HID descriptor length"));
        }
        descriptor.size = descriptor_size as u32;
        if libc::ioctl(
            file.as_raw_fd(),
            libc::_IOR::<ReportDescriptor>(b'H'.into(), 2),
            &mut descriptor,
        ) < 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    if descriptor.size as usize != DESCRIPTOR.len()
        || &descriptor.value[..DESCRIPTOR.len()] != DESCRIPTOR
    {
        return Err(io::Error::other("unrecognized HID descriptor"));
    }
    Ok(())
}

/// One write is one HID report. Never retry a short write's remaining bytes.
fn send_report<W: Write>(writer: &mut W, report: &[u8; 65]) -> io::Result<()> {
    loop {
        match writer.write(report) {
            Ok(65) => return Ok(()),
            Ok(n) => return Err(io::Error::other(format!("short HID write: {n}/65"))),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

#[derive(Default)]
struct Connection<W> {
    writer: Option<W>,
}
impl<W: Write> Connection<W> {
    fn send(&mut self, report: &[u8; 65], open: impl FnOnce() -> io::Result<W>) -> io::Result<()> {
        if self.writer.is_none() {
            self.writer = Some(open()?);
        }
        let result = send_report(self.writer.as_mut().unwrap(), report);
        if result.is_err() {
            self.writer = None;
        }
        result
    }
}

pub struct Hid {
    selected: Option<PathBuf>,
    connection: Connection<File>,
}
impl Hid {
    pub fn new(selected: Option<PathBuf>) -> Self {
        Self {
            selected,
            connection: Connection { writer: None },
        }
    }
    pub fn write(&mut self, report: &[u8; 65]) -> Result<(), String> {
        let selected = &self.selected;
        self.connection
            .send(report, || {
                let mut devices = discover(Path::new("/sys/class/hidraw"));
                if let Some(selected) = selected {
                    let target = selected.canonicalize()?;
                    devices.retain(|p| p.canonicalize().is_ok_and(|p| p == target));
                }
                if devices.len() != 1 {
                    return Err(io::Error::other(format!(
                        "expected one Flux Pro device, found {}",
                        devices.len()
                    )));
                }
                let file = OpenOptions::new().write(true).open(&devices[0])?;
                validate(&file)?;
                eprintln!("display connected: {}", devices[0].display());
                Ok(file)
            })
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    struct Fake {
        fail: Rc<Cell<bool>>,
        count: Rc<Cell<usize>>,
        short: bool,
    }
    impl Write for Fake {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.count.set(self.count.get() + 1);
            if self.fail.get() {
                Err(io::Error::other("disconnected"))
            } else {
                Ok(if self.short { 10 } else { b.len() })
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn reuses_connection_and_reopens_after_failure() {
        let fail = Rc::new(Cell::new(false));
        let count = Rc::new(Cell::new(0));
        let opens = Cell::new(0);
        let open = || {
            opens.set(opens.get() + 1);
            Ok(Fake {
                fail: fail.clone(),
                count: count.clone(),
                short: false,
            })
        };
        let mut connection = Connection { writer: None };
        let packet = crate::report(Some(40000), Some(50000));
        connection.send(&packet, open).unwrap();
        connection.send(&packet, open).unwrap();
        assert_eq!(opens.get(), 1);
        fail.set(true);
        assert!(connection.send(&packet, open).is_err());
        assert!(connection.writer.is_none());
        fail.set(false);
        connection.send(&packet, open).unwrap();
        assert_eq!(opens.get(), 2);
    }
    #[test]
    fn short_write_is_never_fragmented() {
        let count = Rc::new(Cell::new(0));
        let mut writer = Fake {
            fail: Rc::new(Cell::new(false)),
            count: count.clone(),
            short: true,
        };
        assert!(send_report(&mut writer, &crate::report(None, None)).is_err());
        assert_eq!(count.get(), 1);
    }
    #[test]
    fn regular_file_fails_identity_ioctl() {
        let file = tempfile::tempfile().unwrap();
        assert!(validate(&file).is_err());
    }
    #[test]
    fn discovers_only_matching_usb_ids() {
        let root = tempfile::tempdir().unwrap();
        for (name, id) in [
            ("hidraw1", HID_ID),
            ("hidraw2", "HID_ID=0003:00002022:00000523"),
        ] {
            let d = root.path().join(name).join("device");
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("uevent"), id).unwrap();
        }
        assert_eq!(discover(root.path()), vec![PathBuf::from("/dev/hidraw1")]);
    }
}

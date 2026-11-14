// SPDX-License-Identifier: Apache-2.0
//! Which device node belongs to a YKUR: read from sysfs on Linux and from
//! the pnpinfo of the USB device on FreeBSD. Built on every system, so that
//! its tests run everywhere.

use super::{PRODUCT_ID, VENDOR_ID};
use crate::sanitize::sanitize;

/// The serial number of a YKUR in the `%pnpinfo` of a FreeBSD USB device, or
/// `None` for any other device.
#[cfg_attr(not(target_os = "freebsd"), allow(dead_code))]
pub(super) fn ykur_serial(pnpinfo: &str) -> Option<String> {
    let field = |key: &str| {
        pnpinfo
            .split_whitespace()
            .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
    };
    let hex = |key: &str| u16::from_str_radix(field(key)?.strip_prefix("0x")?, 16).ok();

    (hex("vendor")? == VENDOR_ID && hex("product")? == PRODUCT_ID).then(|| {
        match field("sernum").unwrap_or("").trim_matches('"') {
            // Named as the device list names a board without one.
            "" => "<unknown>".to_owned(),
            sernum => sanitize(sernum),
        }
    })
}

/// Reads `DEVNAME` from the `uevent` file in a hidraw directory of sysfs and
/// turns it into the path under `/dev`. Only a plain `hidrawN` is taken, so a
/// file that says something else cannot point the lock anywhere.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn hidraw_node(sysfs: &std::path::Path) -> Option<std::path::PathBuf> {
    let uevent = std::fs::read_to_string(sysfs.join("uevent")).ok()?;
    let name = uevent
        .lines()
        .find_map(|line| line.strip_prefix("DEVNAME="))?
        .trim();

    is_hidraw_name(name).then(|| std::path::Path::new("/dev").join(name))
}

/// Whether `name` is `hidraw` followed by one or more ASCII digits.
pub(super) fn is_hidraw_name(name: &str) -> bool {
    name.strip_prefix("hidraw")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_is_recognised_in_the_pnp_info_of_freebsd() {
        let ykur = r#"vendor=0x04d8 product=0xf1cb devclass=0x00 sernum="YK22170" release=0x0100"#;
        assert_eq!(ykur_serial(ykur), Some("YK22170".to_owned()));
        let mouse = r#"vendor=0x0e0f product=0x0003 sernum="" release=0x0102"#;
        assert_eq!(ykur_serial(mouse), None);
        let unnamed = r#"vendor=0x04d8 product=0xf1cb sernum="""#;
        assert_eq!(ykur_serial(unnamed), Some("<unknown>".to_owned()));
        assert_eq!(ykur_serial("pci0:0:0:0"), None);
    }

    /// A directory in the temporary directory standing in for sysfs,
    /// removed again when dropped.
    struct FakeSysfs(std::path::PathBuf);

    impl FakeSysfs {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("ykur-sysfs-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            FakeSysfs(dir)
        }

        fn file(&self, path: &str, content: &str) -> &Self {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
            self
        }
    }

    impl Drop for FakeSysfs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_hidraw_node_is_read_from_sysfs() {
        let sysfs = FakeSysfs::new("node");
        sysfs.file(
            "hidraw/hidraw3/uevent",
            "MAJOR=241\nMINOR=3\nDEVNAME=hidraw3",
        );

        assert_eq!(
            hidraw_node(&sysfs.0.join("hidraw/hidraw3")),
            Some(std::path::PathBuf::from("/dev/hidraw3"))
        );
    }

    #[test]
    fn a_node_name_that_is_not_hidraw_is_refused() {
        let sysfs = FakeSysfs::new("badnode");
        sysfs
            .file("a/uevent", "DEVNAME=../sda")
            .file("b/uevent", "DEVNAME=hidraw")
            .file("c/uevent", "DEVNAME=hidraw1/../../sda")
            .file("d/uevent", "MAJOR=241");

        for dir in ["a", "b", "c", "d", "missing"] {
            assert_eq!(hidraw_node(&sysfs.0.join(dir)), None, "{dir}");
        }
    }
}

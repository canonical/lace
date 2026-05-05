use dm::{DeviceId, DeviceManager, Driver, DriverRegistry};

// -- Interfaces --

trait InputDevice: std::fmt::Debug {
    fn read(&self) -> &str;
}

trait OutputDevice: std::fmt::Debug {
    fn write(&self, data: &str);
}

trait BlockDevice: std::fmt::Debug {
    fn read_block(&self, lba: u64, buf: &mut [u8]);
}

// -- Bus metadata types --

/// Metadata for devices discovered on a PCI bus.
#[derive(Debug, Clone)]
struct PciDevice {
    bus: u8,
    dev: u8,
    func: u8,
    vendor_id: u16,
    device_id: u16,
}

impl PciDevice {
    fn addr(&self) -> String {
        format!("{:02x}:{:02x}.{}", self.bus, self.dev, self.func)
    }
}

impl std::fmt::Display for PciDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} [{:04x}:{:04x}]", self.addr(), self.vendor_id, self.device_id)
    }
}

/// Metadata for devices on the platform bus (device-tree style).
#[derive(Debug, Clone)]
struct PlatformDevice {
    compatible: Vec<String>,
    base_addr: u64,
}

/// Metadata for devices on a USB bus.
#[derive(Debug, Clone)]
struct UsbDevice {
    port: u8,
    vendor_id: u16,
    product_id: u16,
    class: u8,
}

impl std::fmt::Display for UsbDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "port {} [{:04x}:{:04x}] class {:02x}",
            self.port, self.vendor_id, self.product_id, self.class)
    }
}

// -- Match helpers --

fn pci_match(dm: &DeviceManager, id: DeviceId, vendor: u16, device: u16) -> bool {
    dm.get::<PciDevice>(id)
        .is_some_and(|p| p.vendor_id == vendor && p.device_id == device)
}

fn platform_match(dm: &DeviceManager, id: DeviceId, compat: &str) -> bool {
    dm.get::<PlatformDevice>(id)
        .is_some_and(|p| p.compatible.iter().any(|c| c == compat))
}

fn usb_match_class(dm: &DeviceManager, id: DeviceId, class: u8) -> bool {
    dm.get::<UsbDevice>(id)
        .is_some_and(|u| u.class == class)
}

// -- Concrete device types --

#[derive(Debug)]
struct SerialPort { name: String }

impl InputDevice for SerialPort {
    fn read(&self) -> &str { &self.name }
}

impl OutputDevice for SerialPort {
    fn write(&self, data: &str) {
        println!("    [{} tx] {}", self.name, data);
    }
}

#[derive(Debug)]
struct NvmeCtrl { pci_addr: String, capacity_mb: u64 }

impl BlockDevice for NvmeCtrl {
    fn read_block(&self, lba: u64, buf: &mut [u8]) {
        buf.fill(lba as u8);
    }
}

#[derive(Debug)]
struct UsbXhci { pci_addr: String, ports: u8 }

#[derive(Debug)]
struct UsbKeyboard { name: String }

impl InputDevice for UsbKeyboard {
    fn read(&self) -> &str { &self.name }
}

#[derive(Debug)]
struct UsbMassStorage { name: String, capacity_mb: u64 }

impl BlockDevice for UsbMassStorage {
    fn read_block(&self, lba: u64, buf: &mut [u8]) {
        buf.fill(lba as u8);
    }
}

#[derive(Debug)]
struct Pl011Uart { name: String, base: u64 }

impl InputDevice for Pl011Uart {
    fn read(&self) -> &str { &self.name }
}

impl OutputDevice for Pl011Uart {
    fn write(&self, data: &str) {
        println!("    [{} @ {:#x} tx] {}", self.name, self.base, data);
    }
}

#[derive(Debug)]
struct GpioKeys { name: String }

impl InputDevice for GpioKeys {
    fn read(&self) -> &str { &self.name }
}

// -- Drivers --

struct PlatformBusDriver;

impl Driver for PlatformBusDriver {
    fn name(&self) -> &str { "platform-bus" }

    fn matches(&self, dm: &DeviceManager, id: DeviceId) -> bool {
        platform_match(dm, id, "simple-bus")
    }

    fn bind(&self, dm: &mut DeviceManager, reg: &DriverRegistry, id: DeviceId) {
        let entries: &[(&[&str], u64)] = &[
            (&["arm,pl011", "arm,primecell"], 0x0900_0000),
            (&["gpio-keys"], 0x0901_0000),
            (&["pci-host-ecam-generic"], 0x4000_0000),
        ];

        for &(compat, base) in entries {
            let child = dm.add(Some(id));
            dm.set_interface::<PlatformDevice>(child, Box::new(PlatformDevice {
                compatible: compat.iter().map(|s| s.to_string()).collect(),
                base_addr: base,
            }));
        }

        reg.bind_children(dm, id);
    }
}

struct PciHostEcamDriver;

impl Driver for PciHostEcamDriver {
    fn name(&self) -> &str { "pci-host-ecam" }

    fn matches(&self, dm: &DeviceManager, id: DeviceId) -> bool {
        platform_match(dm, id, "pci-host-ecam-generic")
    }

    fn bind(&self, dm: &mut DeviceManager, reg: &DriverRegistry, id: DeviceId) {
        let found: &[(u8, u8, u8, u16, u16)] = &[
            (0, 0, 0, 0x8086, 0x1234),
            (0, 1, 0, 0x144d, 0xa808),
            (0, 2, 0, 0x1b21, 0x3241),
        ];

        for &(bus, dev, func, vendor, devid) in found {
            let child = dm.add(Some(id));
            dm.set_interface::<PciDevice>(child, Box::new(PciDevice {
                bus, dev, func, vendor_id: vendor, device_id: devid,
            }));
        }

        reg.bind_children(dm, id);
    }
}

struct PciSerialDriver;

impl Driver for PciSerialDriver {
    fn name(&self) -> &str { "pci-serial" }

    fn matches(&self, dm: &DeviceManager, id: DeviceId) -> bool {
        pci_match(dm, id, 0x8086, 0x1234)
    }

    fn bind(&self, dm: &mut DeviceManager, _reg: &DriverRegistry, id: DeviceId) {
        let addr = dm.get::<PciDevice>(id).unwrap().addr();
        let name = format!("pci-serial@{}", addr);
        dm.set_interface::<dyn InputDevice>(id, Box::new(
            SerialPort { name: name.clone() }
        ));
        dm.set_interface::<dyn OutputDevice>(id, Box::new(
            SerialPort { name }
        ));
    }
}

struct NvmeDriver;

impl Driver for NvmeDriver {
    fn name(&self) -> &str { "nvme" }

    fn matches(&self, dm: &DeviceManager, id: DeviceId) -> bool {
        pci_match(dm, id, 0x144d, 0xa808)
    }

    fn bind(&self, dm: &mut DeviceManager, _reg: &DriverRegistry, id: DeviceId) {
        let addr = dm.get::<PciDevice>(id).unwrap().addr();
        dm.set_interface::<dyn BlockDevice>(id, Box::new(NvmeCtrl {
            pci_addr: addr,
            capacity_mb: 512000,
        }));
    }
}

struct XhciDriver;

impl Driver for XhciDriver {
    fn name(&self) -> &str { "xhci" }

    fn matches(&self, dm: &DeviceManager, id: DeviceId) -> bool {
        pci_match(dm, id, 0x1b21, 0x3241)
    }

    fn bind(&self, dm: &mut DeviceManager, reg: &DriverRegistry, id: DeviceId) {
        let addr = dm.get::<PciDevice>(id).unwrap().addr();
        dm.set_interface::<UsbXhci>(id, Box::new(UsbXhci {
            pci_addr: addr,
            ports: 4,
        }));

        let usb_devices: &[(u8, u16, u16, u8)] = &[
            (1, 0x046d, 0xc534, 0x03),
            (2, 0x0781, 0x5583, 0x08),
        ];

        for &(port, vendor, product, class) in usb_devices {
            let child = dm.add(Some(id));
            dm.set_interface::<UsbDevice>(child, Box::new(UsbDevice {
                port, vendor_id: vendor, product_id: product, class,
            }));
        }

        reg.bind_children(dm, id);
    }
}

struct UsbHidKeyboardDriver;

impl Driver for UsbHidKeyboardDriver {
    fn name(&self) -> &str { "usb-hid-keyboard" }

    fn matches(&self, dm: &DeviceManager, id: DeviceId) -> bool {
        usb_match_class(dm, id, 0x03)
    }

    fn bind(&self, dm: &mut DeviceManager, _reg: &DriverRegistry, id: DeviceId) {
        let usb = dm.get::<UsbDevice>(id).unwrap().clone();
        dm.set_interface::<dyn InputDevice>(id, Box::new(UsbKeyboard {
            name: format!("usb-kbd@port{} [{:04x}:{:04x}]",
                usb.port, usb.vendor_id, usb.product_id),
        }));
    }
}

struct UsbMassStorageDriver;

impl Driver for UsbMassStorageDriver {
    fn name(&self) -> &str { "usb-mass-storage" }

    fn matches(&self, dm: &DeviceManager, id: DeviceId) -> bool {
        usb_match_class(dm, id, 0x08)
    }

    fn bind(&self, dm: &mut DeviceManager, _reg: &DriverRegistry, id: DeviceId) {
        let usb = dm.get::<UsbDevice>(id).unwrap().clone();
        dm.set_interface::<dyn BlockDevice>(id, Box::new(UsbMassStorage {
            name: format!("usb-storage@port{} [{:04x}:{:04x}]",
                usb.port, usb.vendor_id, usb.product_id),
            capacity_mb: 32000,
        }));
    }
}

struct Pl011Driver;

impl Driver for Pl011Driver {
    fn name(&self) -> &str { "pl011" }

    fn matches(&self, dm: &DeviceManager, id: DeviceId) -> bool {
        platform_match(dm, id, "arm,pl011")
    }

    fn bind(&self, dm: &mut DeviceManager, _reg: &DriverRegistry, id: DeviceId) {
        let plat = dm.get::<PlatformDevice>(id).unwrap().clone();
        let name = format!("pl011@{:#x}", plat.base_addr);
        dm.set_interface::<dyn InputDevice>(id, Box::new(
            Pl011Uart { name: name.clone(), base: plat.base_addr }
        ));
        dm.set_interface::<dyn OutputDevice>(id, Box::new(
            Pl011Uart { name, base: plat.base_addr }
        ));
    }
}

struct GpioKeysDriver;

impl Driver for GpioKeysDriver {
    fn name(&self) -> &str { "gpio-keys" }

    fn matches(&self, dm: &DeviceManager, id: DeviceId) -> bool {
        platform_match(dm, id, "gpio-keys")
    }

    fn bind(&self, dm: &mut DeviceManager, _reg: &DriverRegistry, id: DeviceId) {
        let plat = dm.get::<PlatformDevice>(id).unwrap().clone();
        dm.set_interface::<dyn InputDevice>(id, Box::new(
            GpioKeys { name: format!("gpio-keys@{:#x}", plat.base_addr) }
        ));
    }
}

fn register_drivers(reg: &mut DriverRegistry) {
    reg.register(PlatformBusDriver);
    reg.register(PciHostEcamDriver);
    reg.register(PciSerialDriver);
    reg.register(NvmeDriver);
    reg.register(XhciDriver);
    reg.register(UsbHidKeyboardDriver);
    reg.register(UsbMassStorageDriver);
    reg.register(Pl011Driver);
    reg.register(GpioKeysDriver);
}

// -- Tree printing --

fn print_tree(dm: &DeviceManager, id: DeviceId, depth: usize) {
    let indent = "  ".repeat(depth);

    let mut desc = Vec::new();
    if let Some(pci) = dm.get::<PciDevice>(id) {
        desc.push(format!("pci {}", pci));
    }
    if let Some(plat) = dm.get::<PlatformDevice>(id) {
        desc.push(format!("platform {:?} @ {:#x}", plat.compatible, plat.base_addr));
    }
    if let Some(usb) = dm.get::<UsbDevice>(id) {
        desc.push(format!("usb {}", usb));
    }

    let mut ifaces = Vec::new();
    if dm.get::<dyn InputDevice>(id).is_some() { ifaces.push("InputDevice"); }
    if dm.get::<dyn OutputDevice>(id).is_some() { ifaces.push("OutputDevice"); }
    if dm.get::<dyn BlockDevice>(id).is_some() { ifaces.push("BlockDevice"); }
    if dm.get::<UsbXhci>(id).is_some() { ifaces.push("UsbXhci"); }

    let desc_str = if desc.is_empty() { String::new() } else { format!(" {}", desc.join(", ")) };
    let iface_str = if ifaces.is_empty() { "bus".to_string() } else { ifaces.join(", ") };

    println!("{}[{}]{} ({})", indent, id, desc_str, iface_str);

    for &child in dm.children_of(id) {
        print_tree(dm, child, depth + 1);
    }
}

fn main() {
    // Register all drivers (bus + leaf)
    let mut reg = DriverRegistry::new();
    register_drivers(&mut reg);

    let mut dm = DeviceManager::new();

    // The root is a platform bus node. Binding it cascades:
    //   platform-bus driver enumerates DT entries
    //     -> pl011, gpio-keys get bound as leaf drivers
    //     -> pci-host-ecam-generic gets bound as a bus driver
    //       -> PCI scan creates child nodes
    //         -> pci-serial, nvme, xhci get bound as leaf drivers
    let root = dm.add(None);
    dm.set_interface::<PlatformDevice>(root, Box::new(PlatformDevice {
        compatible: vec!["simple-bus".to_string()],
        base_addr: 0,
    }));

    println!("Binding root...");
    reg.bind_device(&mut dm, root);

    println!("\nDevice tree:");
    print_tree(&dm, root, 0);

    println!("\nAll InputDevices:");
    dm.for_each::<dyn InputDevice>(|id, dev| {
        println!("  [{}] {:?}", id, dev);
    });

    println!("\nAll BlockDevices:");
    dm.for_each::<dyn BlockDevice>(|id, dev| {
        println!("  [{}] {:?}", id, dev);
    });

    println!("\nWriting to all OutputDevices:");
    dm.for_each::<dyn OutputDevice>(|id, dev| {
        print!("  [{}]", id);
        dev.write("hello");
    });

    // Hot-unplug: find the PCI host bridge and remove it
    let pci_bridge = dm.children_of(root).iter()
        .find(|&&c| platform_match(&dm, c, "pci-host-ecam-generic"))
        .copied()
        .unwrap();

    println!("\nRemoving PCI host bridge (hot-unplug entire bus)...");
    dm.remove(pci_bridge);

    println!("\nDevice tree after unplug:");
    print_tree(&dm, root, 0);

    println!("\nAll InputDevices after unplug:");
    dm.for_each::<dyn InputDevice>(|id, dev| {
        println!("  [{}] {:?}", id, dev);
    });
}

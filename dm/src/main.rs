use dm::{Device, Query, DeviceManager, interface};

#[interface]
trait InputDevice: std::fmt::Debug {}

#[interface]
trait OutputDevice: std::fmt::Debug {}

#[derive(Debug, Device)]
#[interfaces(InputDevice, OutputDevice)]
struct SerialPort {}

impl InputDevice for SerialPort {}
impl OutputDevice for SerialPort {}

#[derive(Debug, Device)]
#[interfaces(InputDevice)]
struct Keyboard {}

impl InputDevice for Keyboard {}

#[derive(Debug, Device)]
#[interfaces(OutputDevice)]
struct Screen {}

impl OutputDevice for Screen {}

fn main() {
    let mut dm = DeviceManager::new();
    dm.register_device(SerialPort {});
    dm.register_device(Keyboard {});
    dm.register_device(Screen {});

    println!("InputDevices:");
    dm.for_each_device(|dev| {
        if let Some(input) = dev.query::<dyn InputDevice>() {
            println!("  {:?}", input);
        }
    });

    println!("OutputDevices:");
    dm.for_each_device(|dev| {
        if let Some(output) = dev.query::<dyn OutputDevice>() {
            println!("  {:?}", output);
        }
    });
}

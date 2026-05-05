//! Device manager framework with runtime interface querying.
//!
//! Types register which interface traits they implement via `#[derive(Device)]`
//! and `#[interfaces(...)]`. Callers can then query any device for a specific
//! interface at runtime using `Query::query` or iterate matching devices with
//! `DeviceManager::for_each_iface`.

use std::any::TypeId;

pub use dm_derive::Device;
pub use dm_derive::interface;

/// Placeholder trait whose only purpose is to give us a `dyn`-compatible fat pointer type.
///
/// `*const dyn Erased` can carry any trait object's data pointer through the
/// type-erased interface. The vtable is meaningless and never called, it
/// exists solely so that `*const dyn Erased` has the same size and layout as
/// any other `*const dyn Trait`. This trait is never implemented by anything.
pub trait Erased {}

/// Enables runtime casting between `*const dyn Erased` and `&dyn SomeTrait`.
///
/// Implemented on `dyn SomeTrait` (not on concrete types) via the `#[interface]`
/// attribute macro. The `transmute` is sound here because all `dyn Trait` fat
/// pointers have the same size and layout as `*const dyn Erased`.
///
/// # Safety
/// Must only be implemented for `dyn Trait` types. The generated `to_erased`
/// and `from_erased` rely on all `dyn` pointer types being the same size.
pub unsafe trait Interface: 'static {
    /// Reinterpret `&self` (a `&dyn Trait` fat pointer) as `*const dyn Erased`.
    unsafe fn to_erased(&self) -> *const dyn Erased;

    /// Reconstruct a `&dyn Trait` from a `*const dyn Erased` that was
    /// originally created by `to_erased` on the same trait.
    unsafe fn from_erased<'a>(raw: *const dyn Erased) -> &'a Self;
}

/// Core trait for all managed device types.
///
/// Automatically implemented by `#[derive(Device)]`. The generated code
/// checks the `TypeId` against each trait listed in `#[interfaces(...)]` and
/// returns the corresponding type-erased fat pointer.
pub trait Device {
    /// If this device implements the interface identified by `id`, return a
    /// type-erased fat pointer that can be cast back via `Interface::from_erased`.
    fn query_raw(&self, id: TypeId) -> Option<*const dyn Erased>;
}

/// Extension trait providing a type-safe `query` method on any `Device`.
///
/// Blanket-implemented for all `Device` types so callers can write
/// `device.query::<dyn InputDevice>()` without importing anything extra.
pub trait Query: Device {
    /// Query this device for a specific interface, returning a reference if supported.
    fn query<T>(&self) -> Option<&T>
    where
        T: Interface + ?Sized,
    {
        let id = TypeId::of::<T>();
        // SAFETY: `query_raw` guarantees that a returned pointer was created by
        // `Interface::to_erased` for the same `TypeId`, so `from_erased` recovers
        // the original `&dyn T`.
        self.query_raw(id).map(|raw| unsafe { T::from_erased(raw) })
    }
}

impl<T: Device + ?Sized> Query for T {}

pub struct DeviceManager {
    devices: Vec<Box<dyn Device>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    /// Add a device to the manager. The device must implement `Device` (via `#[derive(Device)]`).
    pub fn register_device<T: Device + 'static>(&mut self, device: T) {
        self.devices.push(Box::new(device));
    }

    /// Iterate over all registered devices as `&dyn Device`.
    pub fn for_each_device(&self, mut cb: impl FnMut(&dyn Device)) {
        self.devices.iter().for_each(|dev| cb(dev.as_ref()));
    }

    /// Iterate over all devices that implement a given interface trait.
    ///
    /// Devices that don't implement the interface are silently skipped.
    pub fn for_each_iface<T: Interface + ?Sized>(&self, mut cb: impl FnMut(&T)) {
        for dev in &self.devices {
            if let Some(iface) = dev.query::<T>() {
                cb(iface);
            }
        }
    }
}

//! Device manager with centralized ownership, ID-based references, and
//! driver binding.
//!
//! The `DeviceManager` owns all devices. Devices reference each other by
//! `DeviceId`, not by pointer. This allows clean removal (hot-unplug) without
//! dangling references. Interface traits are stored as `Box<dyn Trait>` inside
//! `Box<dyn Any>`, using `Any::downcast_ref` for fully safe, stable casts.
//!
//! Each device node sits in a parent-child tree. Removing a node recursively
//! removes all of its children.
//!
//! Drivers register themselves in a `DriverRegistry` with a match predicate
//! and a bind function. Buses enumerate hardware, create device nodes with
//! metadata, then call `bind_device` to let the registry find and attach the
//! right driver.

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Opaque handle to a device in the manager. Stable across removals of other
/// devices (indices are never reused or shifted).
pub type DeviceId = usize;

struct DeviceNode {
    parent: Option<DeviceId>,
    children: Vec<DeviceId>,
    /// Maps `TypeId::of::<Box<dyn SomeTrait>>()` to a `Box<dyn Any>` that
    /// holds a `Box<dyn SomeTrait>`. Using `Box<dyn Trait>` as the inner type
    /// means `downcast_ref` can recover it safely -- no transmute needed.
    interfaces: HashMap<TypeId, Box<dyn Any>>,
}

pub struct DeviceManager {
    nodes: Vec<Option<DeviceNode>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Create a new device node, optionally under a parent. Returns its ID.
    pub fn add(&mut self, parent: Option<DeviceId>) -> DeviceId {
        let id = self.nodes.len();
        if let Some(pid) = parent {
            self.nodes[pid].as_mut().expect("parent does not exist").children.push(id);
        }
        self.nodes.push(Some(DeviceNode {
            parent,
            children: Vec::new(),
            interfaces: HashMap::new(),
        }));
        id
    }

    /// Register an interface implementation on a device node.
    ///
    /// `T` is a trait object type (e.g. `dyn BlockDevice`). The concrete type
    /// behind the `Box` can differ per device -- that is the whole point.
    pub fn set_interface<T: ?Sized + 'static>(&mut self, id: DeviceId, val: Box<T>) {
        self.nodes[id]
            .as_mut()
            .expect("device does not exist")
            .interfaces
            .insert(TypeId::of::<Box<T>>(), Box::new(val));
    }

    /// Get an interface reference from a specific device, if it provides one.
    pub fn get<T: ?Sized + 'static>(&self, id: DeviceId) -> Option<&T> {
        self.nodes[id]
            .as_ref()?
            .interfaces
            .get(&TypeId::of::<Box<T>>())?
            .downcast_ref::<Box<T>>()
            .map(|b| &**b)
    }

    /// Check whether a device node exists (has not been removed).
    pub fn exists(&self, id: DeviceId) -> bool {
        self.nodes.get(id).is_some_and(|slot| slot.is_some())
    }

    /// Return the parent of a device, if any.
    pub fn parent_of(&self, id: DeviceId) -> Option<DeviceId> {
        self.nodes[id].as_ref()?.parent
    }

    /// Return the children of a device.
    pub fn children_of(&self, id: DeviceId) -> &[DeviceId] {
        match &self.nodes[id] {
            Some(node) => &node.children,
            None => &[],
        }
    }

    /// Remove a device and all of its children recursively.
    /// Detaches the device from its parent's child list.
    pub fn remove(&mut self, id: DeviceId) {
        if let Some(node) = self.nodes[id].take() {
            // Detach from parent
            if let Some(pid) = node.parent {
                if let Some(parent) = &mut self.nodes[pid] {
                    parent.children.retain(|&c| c != id);
                }
            }
            // Recursively remove children
            for child in node.children {
                self.remove(child);
            }
        }
    }

    /// Iterate over all live devices that provide the given interface.
    pub fn for_each<T: ?Sized + 'static>(&self, mut cb: impl FnMut(DeviceId, &T)) {
        let key = TypeId::of::<Box<T>>();
        for (id, slot) in self.nodes.iter().enumerate() {
            if let Some(node) = slot {
                if let Some(val) = node.interfaces.get(&key) {
                    if let Some(b) = val.downcast_ref::<Box<T>>() {
                        cb(id, b);
                    }
                }
            }
        }
    }
}

pub trait Driver {
    fn name(&self) -> &str;
    fn matches(&self, dm: &DeviceManager, id: DeviceId) -> bool;
    fn bind(&self, dm: &mut DeviceManager, reg: &DriverRegistry, id: DeviceId);
}

pub struct DriverRegistry {
    drivers: Vec<Box<dyn Driver>>,
}

impl DriverRegistry {
    pub fn new() -> Self {
        Self { drivers: Vec::new() }
    }

    pub fn register(&mut self, driver: impl Driver + 'static) {
        self.drivers.push(Box::new(driver));
    }

    /// Try to find a matching driver for the device and bind it.
    /// Returns the driver name if a match was found.
    pub fn bind_device(&self, dm: &mut DeviceManager, id: DeviceId) -> Option<&str> {
        let idx = self.drivers.iter().position(|d| d.matches(dm, id))?;
        let name = self.drivers[idx].name();
        self.drivers[idx].bind(dm, self, id);
        Some(name)
    }

    /// Try to bind drivers to all unbound children of a device.
    /// Returns the number of devices successfully bound.
    pub fn bind_children(&self, dm: &mut DeviceManager, parent: DeviceId) -> usize {
        let children: Vec<DeviceId> = dm.children_of(parent).to_vec();
        let mut count = 0;
        for child in children {
            if self.bind_device(dm, child).is_some() {
                count += 1;
            }
        }
        count
    }
}

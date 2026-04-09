#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
mod bootblock;

#[cfg(not(target_os = "none"))]
fn main() {}

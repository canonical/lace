# lace

Lace is a framework for writing boot applications.

The current list of applications is:

* `lace-stubble` - a "stub" EFI binary that can be specialized into a bootable image by embedding resources (kernel, initrd, etc.) into PE sections.
* `tools/pewrap` - a tool to wrap assets into a stubble image
* `tools/collect-hwids` - a helper tool to collect hwids from a running laptop

They use the following library crates:

* `lace-platform` - An abstraction of the underlying platform (such as UEFI).
* `lace-util` - Platform-independent utilities
* `lace-util-derive` - Platform-independent macros

# Contributing
Lace is licensed under the GPL-2.0 or GPL-3.0. Contributions are subject
to the Canonical CLA. See the file `CONTRIBUTING.md` for details.

You may want to use development tools:
* `scripts/cargo_ci.py` - A wrapper around cargo avoiding feature unification
* `scripts/vm_manage.py` - a helper tool to setup and run test VMs
* `tools/fakeedid` - a helper tool to fake some EDID data in a test VM

//! USB device layer: raw descriptor parsing for device and configuration
//! blobs. The device enumeration registry extends this module in a later
//! task; for now it only exposes the parsers.

pub mod desc;

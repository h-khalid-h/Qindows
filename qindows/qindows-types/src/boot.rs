//! # Boot Types
//!
//! Shared boot information passed from the UEFI bootloader to the Qernel.

/// Boot information passed from the bootloader to the Qernel.
/// This struct lives at a well-known physical address after UEFI exits.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BootInfo {
    /// Framebuffer base address for direct pixel access
    pub framebuffer_addr: u64,
    /// Framebuffer size in bytes
    pub framebuffer_size: u64,
    /// Horizontal resolution in pixels
    pub horizontal_resolution: u32,
    /// Vertical resolution in pixels
    pub vertical_resolution: u32,
    /// Pixels per scanline (may differ from horizontal_resolution due to padding)
    pub pixels_per_scanline: u32,
    /// Physical address of the UEFI memory map
    pub memory_map_addr: u64,
    /// Number of memory map entries
    pub memory_map_entries: u64,
    /// Size of each memory descriptor
    pub memory_map_desc_size: u64,
}

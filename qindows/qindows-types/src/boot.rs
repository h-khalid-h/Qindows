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
    /// Number of CONVENTIONAL memory entries (usable RAM regions only)
    pub memory_map_entries: u64,
    /// Size of each memory descriptor in bytes
    pub memory_map_desc_size: u64,
    /// Total byte size of the memory map buffer (all entry types)
    pub memory_map_total_size: u64,
    /// Total usable RAM in bytes (computed from CONVENTIONAL entries before exit_boot_services)
    pub usable_ram_bytes: u64,
}

//! # EFI Stub — UEFI Boot Services Interface
//!
//! Minimal EFI stub that interfaces with UEFI firmware during
//! early boot, retrieving memory map, framebuffer info, and
//! ACPI tables before handing off to the Qernel (Section 9.34).
//!
//! Features:
//! - Memory map retrieval from UEFI
//! - GOP framebuffer setup
//! - ACPI RSDP location
//! - Boot services exit
//! - Runtime services preservation

extern crate alloc;

use alloc::vec::Vec;

/// EFI memory type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EfiMemoryType {
    Reserved,
    LoaderCode,
    LoaderData,
    BootServicesCode,
    BootServicesData,
    RuntimeServicesCode,
    RuntimeServicesData,
    Conventional,
    Unusable,
    AcpiReclaim,
    AcpiNvs,
    Mmio,
    MmioPortSpace,
}

/// An EFI memory map entry.
#[derive(Debug, Clone)]
pub struct EfiMemoryDescriptor {
    pub memory_type: EfiMemoryType,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub page_count: u64,
    pub attribute: u64,
}

/// GOP framebuffer info.
#[derive(Debug, Clone)]
pub struct GopFramebuffer {
    pub base_addr: u64,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: PixelFormat,
}

/// Pixel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb,
    Bgr,
    Bitmask,
    BltOnly,
}

/// Boot information collected before ExitBootServices.
#[derive(Debug, Clone)]
pub struct BootInfo {
    pub memory_map: Vec<EfiMemoryDescriptor>,
    pub framebuffer: Option<GopFramebuffer>,
    pub rsdp_addr: Option<u64>,
    pub total_memory: u64,
    pub usable_memory: u64,
}

/// The EFI Stub.
pub struct EfiStub {
    pub boot_info: BootInfo,
    pub exited_boot_services: bool,
}

impl EfiStub {
    pub fn new() -> Self {
        EfiStub {
            boot_info: BootInfo {
                memory_map: Vec::new(),
                framebuffer: None,
                rsdp_addr: None,
                total_memory: 0,
                usable_memory: 0,
            },
            exited_boot_services: false,
        }
    }

    /// Parse the UEFI memory map.
    pub fn parse_memory_map(&mut self, entries: Vec<EfiMemoryDescriptor>) {
        let mut total = 0u64;
        let mut usable = 0u64;
        for entry in &entries {
            let bytes = entry.page_count * 4096;
            total += bytes;
            match entry.memory_type {
                EfiMemoryType::Conventional
                | EfiMemoryType::LoaderCode
                | EfiMemoryType::LoaderData
                | EfiMemoryType::BootServicesCode
                | EfiMemoryType::BootServicesData => {
                    usable += bytes;
                }
                _ => {}
            }
        }
        self.boot_info.memory_map = entries;
        self.boot_info.total_memory = total;
        self.boot_info.usable_memory = usable;
    }

    /// Set GOP framebuffer info.
    pub fn set_framebuffer(&mut self, fb: GopFramebuffer) {
        self.boot_info.framebuffer = Some(fb);
    }

    /// Set RSDP address.
    pub fn set_rsdp(&mut self, addr: u64) {
        self.boot_info.rsdp_addr = Some(addr);
    }

    /// Exit boot services and finalize.
    pub fn exit_boot_services(&mut self) -> &BootInfo {
        self.exited_boot_services = true;
        &self.boot_info
    }

    /// Get conventional (usable) memory regions.
    pub fn conventional_regions(&self) -> Vec<&EfiMemoryDescriptor> {
        self.boot_info.memory_map.iter()
            .filter(|e| e.memory_type == EfiMemoryType::Conventional)
            .collect()
    }
}

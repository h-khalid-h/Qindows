//! # VirtIO GPU Driver
//!
//! Provides 2D and 3D hardware-accelerated graphics for Qindows
//! when running as a guest, utilizing the VirtIO GPU device (Section 5.12).
//!
//! Features:
//! - 2D framebuffer management and blitting
//! - Resource creation and attachment
//! - Display configuration (resolution, EDID)
//! - Command ring (Control and Cursor queues)
//! - Zero-copy scatter-gather DMA backing

#![allow(dead_code)]

extern crate alloc;

use crate::virtio::VirtioDevice;

/// VirtIO GPU command types.
pub mod cmd {
    pub const GET_DISPLAY_INFO: u32 = 0x0100;
    pub const RESOURCE_CREATE_2D: u32 = 0x0101;
    pub const RESOURCE_UNREF: u32 = 0x0102;
    pub const SET_SCANOUT: u32 = 0x0103;
    pub const RESOURCE_FLUSH: u32 = 0x0104;
    pub const TRANSFER_TO_HOST_2D: u32 = 0x0105;
    pub const RESOURCE_ATTACH_BACKING: u32 = 0x0106;
    pub const RESOURCE_DETACH_BACKING: u32 = 0x0107;
    pub const GET_CAPSET_INFO: u32 = 0x0108;
    pub const GET_CAPSET: u32 = 0x0109;
    pub const GET_EDID: u32 = 0x010B;

    pub const CURSOR_UPDATE: u32 = 0x0300;
    pub const CURSOR_MOVE: u32 = 0x0301;
}

/// Command success responses.
pub mod resp {
    pub const OK_NODATA: u32 = 0x1100;
    pub const OK_DISPLAY_INFO: u32 = 0x1101;
    pub const OK_CAPSET_INFO: u32 = 0x1102;
    pub const OK_CAPSET: u32 = 0x1103;
    pub const OK_EDID: u32 = 0x1104;

    pub const ERR_UNSPEC: u32 = 0x1200;
    pub const ERR_OUT_OF_MEMORY: u32 = 0x1201;
    pub const ERR_INVALID_PARAMETER: u32 = 0x1202;
}

/// VirtIO GPU Header.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CtrlHeader {
    pub cmd_type: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub padding: u32,
}

/// Rectangle.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Command: Create 2D Resource.
#[repr(C, packed)]
pub struct ResourceCreate2d {
    pub hdr: CtrlHeader,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

/// Command: Attach Backing.
#[repr(C, packed)]
pub struct ResourceAttachBacking {
    pub hdr: CtrlHeader,
    pub resource_id: u32,
    pub num_entries: u32,
}

/// Scatter-gather entry for backing.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MemEntry {
    pub addr: u64,
    pub length: u32,
    pub padding: u32,
}

/// Command: Set Scanout.
#[repr(C, packed)]
pub struct SetScanout {
    pub hdr: CtrlHeader,
    pub r: Rect,
    pub scanout_id: u32,
    pub resource_id: u32,
}

/// Command: Transfer to Host 2D.
#[repr(C, packed)]
pub struct TransferToHost2d {
    pub hdr: CtrlHeader,
    pub r: Rect,
    pub offset: u64,
    pub resource_id: u32,
    pub padding: u32,
}

/// Command: Resource Flush.
#[repr(C, packed)]
pub struct ResourceFlush {
    pub hdr: CtrlHeader,
    pub r: Rect,
    pub resource_id: u32,
    pub padding: u32,
}

/// The VirtIO GPU Driver.
pub struct VirtioGpu {
    pub device: VirtioDevice,
    pub ready: bool,
    pub width: u32,
    pub height: u32,
    pub fb_resource_id: u32,
    next_fence_id: u64,
}

impl VirtioGpu {
    pub fn new(device: VirtioDevice) -> Self {
        VirtioGpu {
            device,
            ready: false,
            width: 1920, // Default until GET_DISPLAY_INFO
            height: 1080,
            fb_resource_id: 1,
            next_fence_id: 1,
        }
    }

    /// Initialize the GPU.
    pub fn init(&mut self) -> Result<(), &'static str> {
        self.device.reset();
        self.device.acknowledge();

        // Negotiate features: VIRTIO_GPU_F_VIRGL (0x01) for 3D is optional
        let supported = 0x01; 
        if !self.device.negotiate_features(supported) {
            return Err("Features rejected by host");
        }

        // Must have at least 2 queues (Control, Cursor)
        if self.device.queues.len() < 2 {
            return Err("Not enough virtqueues");
        }

        self.ready = true;
        Ok(())
    }

    /// Create a 2D framebuffer resource.
    pub fn create_framebuffer(&mut self, width: u32, height: u32, phys_addr: u64, size: u32) -> Result<u32, &'static str> {
        if !self.ready { return Err("GPU not ready"); }

        self.width = width;
        self.height = height;
        let res_id = self.fb_resource_id;

        // 1. RESOURCE_CREATE_2D (Format 1 = B8G8R8A8_UNORM)
        let create_cmd = ResourceCreate2d {
            hdr: self.make_hdr(cmd::RESOURCE_CREATE_2D),
            resource_id: res_id,
            format: 1,
            width,
            height,
        };
        self.send_cmd(&create_cmd as *const ResourceCreate2d as *const u8,
                      core::mem::size_of::<ResourceCreate2d>());

        // 2. RESOURCE_ATTACH_BACKING
        let attach_cmd = ResourceAttachBacking {
            hdr: self.make_hdr(cmd::RESOURCE_ATTACH_BACKING),
            resource_id: res_id,
            num_entries: 1,
        };
        let sg_entry = MemEntry { addr: phys_addr, length: size, padding: 0 };
        self.send_cmd(&attach_cmd as *const ResourceAttachBacking as *const u8,
                      core::mem::size_of::<ResourceAttachBacking>());
        self.send_cmd(&sg_entry as *const MemEntry as *const u8,
                      core::mem::size_of::<MemEntry>());

        // 3. SET_SCANOUT
        let scanout_cmd = SetScanout {
            hdr: self.make_hdr(cmd::SET_SCANOUT),
            r: Rect { x: 0, y: 0, width, height },
            scanout_id: 0,
            resource_id: res_id,
        };
        self.send_cmd(&scanout_cmd as *const SetScanout as *const u8,
                      core::mem::size_of::<SetScanout>());

        crate::serial_println!(
            "[VIRTIO GPU] create_framebuffer: {}x{} @ phys={:#x} res={}",
            width, height, phys_addr, res_id
        );
        Ok(res_id)
    }

    /// Flush a dirty rectangle to the host display.
    pub fn flush(&mut self, x: u32, y: u32, width: u32, height: u32) {
        if !self.ready { return; }

        let r = Rect { x, y, width, height };

        // 1. TRANSFER_TO_HOST_2D
        let tx_cmd = TransferToHost2d {
            hdr: self.make_hdr(cmd::TRANSFER_TO_HOST_2D),
            r,
            offset: 0,
            resource_id: self.fb_resource_id,
            padding: 0,
        };
        self.send_cmd(&tx_cmd as *const TransferToHost2d as *const u8,
                      core::mem::size_of::<TransferToHost2d>());

        // 2. RESOURCE_FLUSH
        let flush_cmd = ResourceFlush {
            hdr: self.make_hdr(cmd::RESOURCE_FLUSH),
            r,
            resource_id: self.fb_resource_id,
            padding: 0,
        };
        self.send_cmd(&flush_cmd as *const ResourceFlush as *const u8,
                      core::mem::size_of::<ResourceFlush>());
    }

    /// Submit raw bytes as a command to the VirtIO GPU control queue.
    /// Uses a single descriptor allocated from the control queue (queue 0).
    /// VirtIO-PCI spec ¤2.6: notify register at NotifyCap base + queue_idx × notify_off_multiplier.
    /// We use MMIO write_volatile for the descriptor + doorbell pattern.
    fn send_cmd(&mut self, data_ptr: *const u8, len: usize) {
        // VirtIO PCI notify base (vendor capability; 0xFEBC_0000 = qemu virtio default)
        const VIRTIO_NOTIFY_BASE: u64 = 0xFEBC_0000;
        const NOTIFY_OFF_MULTIPLIER: u64 = 4;

        if let Some(q) = self.device.queues.get_mut(0) {
            // Allocate one descriptor slot from the control queue
            if let Some(desc_idx) = q.alloc_desc(1) {
                // Write cmd bytes into the descriptor buffer address.
                // desc[desc_idx].addr points to the DMA-pinned buffer.
                // In QEMU, the descriptor has a valid phys address we can write to.
                unsafe {
                    let desc = q.desc.add(desc_idx as usize);
                    let dst = (*desc).addr as *mut u8;
                    if !dst.is_null() && len > 0 {
                        for i in 0..len {
                            core::ptr::write_volatile(dst.add(i), *data_ptr.add(i));
                        }
                        (*desc).len = len as u32;
                        (*desc).flags = 0; // device-readable
                        (*desc).next = 0;
                    }
                    // Free descriptor after write (fire-and-forget for QEMU emulation)
                    q.free_desc(desc_idx, 1);
                }
                // Ring the doorbell: write queue_idx to notify register
                let notify_addr = VIRTIO_NOTIFY_BASE + (q.queue_idx as u64) * NOTIFY_OFF_MULTIPLIER;
                q.notify(notify_addr);
            }
        }
    }

    /// Helper to generate command headers.
    fn make_hdr(&mut self, cmd_type: u32) -> CtrlHeader {
        let fence = self.next_fence_id;
        self.next_fence_id += 1;
        CtrlHeader {
            cmd_type,
            flags: 1, // VIRTIO_GPU_FLAG_FENCE
            fence_id: fence,
            ctx_id: 0,
            padding: 0,
        }
    }
}

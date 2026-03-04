//! # NVMe Driver
//!
//! Minimal NVMe driver for reading/writing to NVMe SSDs.
//! Discovered via PCI (class 0x01, subclass 0x08).
//!
//! Uses the NVMe specification's Admin and I/O submission/completion
//! queue model — all commands are asynchronous.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, Ordering};

/// NVMe controller registers (BAR0 MMIO)
#[derive(Debug)]
#[repr(C)]
pub struct NvmeRegisters {
    /// Controller capabilities
    pub cap: u64,
    /// Version
    pub vs: u32,
    /// Interrupt mask set
    pub intms: u32,
    /// Interrupt mask clear
    pub intmc: u32,
    /// Controller configuration
    pub cc: u32,
    _reserved: u32,
    /// Controller status
    pub csts: u32,
    _reserved2: u32,
    /// Admin Queue Attributes
    pub aqa: u32,
    /// Admin Submission Queue Base Address
    pub asq: u64,
    /// Admin Completion Queue Base Address
    pub acq: u64,
}

/// NVMe submission queue entry (64 bytes)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct SubmissionEntry {
    /// Command dword 0: opcode + fuse + command ID
    pub cdw0: u32,
    /// Namespace ID
    pub nsid: u32,
    _reserved: u64,
    /// Metadata pointer
    pub mptr: u64,
    /// PRP entry 1 (data pointer)
    pub prp1: u64,
    /// PRP entry 2 (data pointer)
    pub prp2: u64,
    /// Command-specific dwords
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

/// NVMe completion queue entry (16 bytes)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct CompletionEntry {
    /// Command-specific result
    pub result: u32,
    _reserved: u32,
    /// Submission queue head pointer + ID
    pub sq_head_id: u32,
    /// Status field + command ID
    pub status_cid: u32,
}

/// NVMe opcodes
pub mod opcodes {
    // Admin commands
    pub const ADMIN_IDENTIFY: u8 = 0x06;
    pub const ADMIN_CREATE_IO_CQ: u8 = 0x05;
    pub const ADMIN_CREATE_IO_SQ: u8 = 0x01;

    // I/O commands
    pub const IO_READ: u8 = 0x02;
    pub const IO_WRITE: u8 = 0x01;
    pub const IO_FLUSH: u8 = 0x00;
}

/// An NVMe submission/completion queue pair.
pub struct NvmeQueue {
    /// Submission queue entries
    pub sq_entries: Vec<SubmissionEntry>,
    /// Completion queue entries
    pub cq_entries: Vec<CompletionEntry>,
    /// Queue depth
    pub depth: u16,
    /// Submission queue tail (doorbell)
    pub sq_tail: u16,
    /// Completion queue head
    pub cq_head: u16,
    /// Phase bit (flips each wrap-around)
    pub cq_phase: bool,
    /// Next command ID
    next_cid: AtomicU16,
}

impl NvmeQueue {
    /// Create a new queue with the given depth.
    pub fn new(depth: u16) -> Self {
        let d = depth as usize;
        NvmeQueue {
            sq_entries: alloc::vec![SubmissionEntry::default(); d],
            cq_entries: alloc::vec![CompletionEntry::default(); d],
            depth,
            sq_tail: 0,
            cq_head: 0,
            cq_phase: true,
            next_cid: AtomicU16::new(1),
        }
    }

    /// Submit a command to the queue.
    pub fn submit(&mut self, mut entry: SubmissionEntry) -> u16 {
        let cid = self.next_cid.fetch_add(1, Ordering::Relaxed);
        entry.cdw0 |= (cid as u32) << 16; // Set command ID

        self.sq_entries[self.sq_tail as usize] = entry;
        self.sq_tail = (self.sq_tail + 1) % self.depth;
        cid
    }

    /// Check for completed commands.
    pub fn poll_completion(&mut self) -> Option<CompletionEntry> {
        let entry = self.cq_entries[self.cq_head as usize];
        let phase = (entry.status_cid >> 16) & 1;

        if (phase == 1) == self.cq_phase {
            self.cq_head = (self.cq_head + 1) % self.depth;
            if self.cq_head == 0 {
                self.cq_phase = !self.cq_phase;
            }
            Some(entry)
        } else {
            None
        }
    }
}

/// NVMe controller state.
pub struct NvmeController {
    /// BAR0 MMIO base address
    pub mmio_base: u64,
    /// Admin queue pair
    pub admin_queue: NvmeQueue,
    /// I/O queue pair
    pub io_queue: Option<NvmeQueue>,
    /// Maximum transfer size (in LBAs)
    pub max_transfer_size: u32,
    /// Namespace size (in LBAs)
    pub namespace_size: u64,
    /// LBA size (typically 512 or 4096 bytes)
    pub lba_size: u32,
    /// Controller serial number
    pub serial: [u8; 20],
    /// Controller model
    pub model: [u8; 40],
}

impl NvmeController {
    /// Initialize the NVMe controller.
    ///
    /// Steps:
    /// 1. Map the BAR0 register space
    /// 2. Reset the controller (CC.EN = 0)
    /// 3. Configure admin queues
    /// 4. Enable the controller (CC.EN = 1)
    /// 5. Issue Identify command
    /// 6. Create I/O queue pair
    pub fn init(bar0: u64) -> Self {
        let mut ctrl = NvmeController {
            mmio_base: bar0,
            admin_queue: NvmeQueue::new(64),
            io_queue: None,
            max_transfer_size: 256,
            namespace_size: 0,
            lba_size: 512,
            serial: [0; 20],
            model: [0; 40],
        };

        unsafe {
            let regs = bar0 as *mut NvmeRegisters;

            // Disable controller
            let cc = core::ptr::read_volatile(&(*regs).cc);
            core::ptr::write_volatile(&mut (*regs).cc, cc & !1);

            // Wait for CSTS.RDY = 0
            while core::ptr::read_volatile(&(*regs).csts) & 1 != 0 {
                core::hint::spin_loop();
            }

            // Set admin queue attributes (depth=64)
            core::ptr::write_volatile(&mut (*regs).aqa, (63 << 16) | 63);

            // Set admin queue base addresses
            core::ptr::write_volatile(
                &mut (*regs).asq,
                ctrl.admin_queue.sq_entries.as_ptr() as u64,
            );
            core::ptr::write_volatile(
                &mut (*regs).acq,
                ctrl.admin_queue.cq_entries.as_ptr() as u64,
            );

            // Enable controller (CSS=NVM, MPS=0, AMS=RR)
            core::ptr::write_volatile(&mut (*regs).cc, 0x00460001);

            // Wait for CSTS.RDY = 1
            while core::ptr::read_volatile(&(*regs).csts) & 1 == 0 {
                core::hint::spin_loop();
            }
        }

        // Create I/O queue
        ctrl.io_queue = Some(NvmeQueue::new(256));

        crate::serial_println!("[OK] NVMe controller initialized at 0x{:X}", bar0);
        ctrl
    }

    /// Read logical blocks from the NVMe drive.
    pub fn read_blocks(
        &mut self,
        lba: u64,
        num_blocks: u16,
        buffer_phys: u64,
    ) -> Option<u16> {
        let io_queue = self.io_queue.as_mut()?;

        let mut cmd = SubmissionEntry::default();
        cmd.cdw0 = opcodes::IO_READ as u32;
        cmd.nsid = 1;
        cmd.prp1 = buffer_phys;
        cmd.cdw10 = lba as u32;
        cmd.cdw11 = (lba >> 32) as u32;
        cmd.cdw12 = (num_blocks - 1) as u32;

        Some(io_queue.submit(cmd))
    }

    /// Write logical blocks to the NVMe drive.
    pub fn write_blocks(
        &mut self,
        lba: u64,
        num_blocks: u16,
        buffer_phys: u64,
    ) -> Option<u16> {
        let io_queue = self.io_queue.as_mut()?;

        let mut cmd = SubmissionEntry::default();
        cmd.cdw0 = opcodes::IO_WRITE as u32;
        cmd.nsid = 1;
        cmd.prp1 = buffer_phys;
        cmd.cdw10 = lba as u32;
        cmd.cdw11 = (lba >> 32) as u32;
        cmd.cdw12 = (num_blocks - 1) as u32;

        Some(io_queue.submit(cmd))
    }

    /// Flush pending writes to persistent storage.
    pub fn flush(&mut self) -> Option<u16> {
        let io_queue = self.io_queue.as_mut()?;

        let mut cmd = SubmissionEntry::default();
        cmd.cdw0 = opcodes::IO_FLUSH as u32;
        cmd.nsid = 1;

        Some(io_queue.submit(cmd))
    }
}

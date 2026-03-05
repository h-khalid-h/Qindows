//! # USB xHCI Host Controller Driver
//!
//! Manages USB devices via the xHCI (eXtensible Host Controller Interface).
//! Handles device enumeration, descriptor parsing, transfer rings,
//! and hub port management. All device communication flows through
//! Transfer Request Blocks (TRBs) on per-endpoint ring buffers.
//!
//! Supports USB 2.0 (High-Speed) and USB 3.x (SuperSpeed) devices.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ─── xHCI Register Offsets ──────────────────────────────────────────────────

/// xHCI Capability Register offsets.
pub mod cap_regs {
    pub const CAPLENGTH: usize = 0x00;
    pub const HCIVERSION: usize = 0x02;
    pub const HCSPARAMS1: usize = 0x04;
    pub const HCSPARAMS2: usize = 0x08;
    pub const HCSPARAMS3: usize = 0x0C;
    pub const HCCPARAMS1: usize = 0x10;
    pub const DBOFF: usize = 0x14;
    pub const RTSOFF: usize = 0x18;
}

/// xHCI Operational Register offsets (relative to op_base).
pub mod op_regs {
    pub const USBCMD: usize = 0x00;
    pub const USBSTS: usize = 0x04;
    pub const PAGESIZE: usize = 0x08;
    pub const DNCTRL: usize = 0x14;
    pub const CRCR: usize = 0x18;     // Command Ring Control Register
    pub const DCBAAP: usize = 0x30;   // Device Context Base Address Array Pointer
    pub const CONFIG: usize = 0x38;
    pub const PORT_BASE: usize = 0x400;
    pub const PORT_STRIDE: usize = 0x10;
}

// ─── USB Speeds ─────────────────────────────────────────────────────────────

/// USB device speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    /// 1.5 Mbps
    Low,
    /// 12 Mbps
    Full,
    /// 480 Mbps
    High,
    /// 5 Gbps
    Super,
    /// 10 Gbps
    SuperPlus,
}

impl UsbSpeed {
    pub fn from_port_speed(psiv: u8) -> Self {
        match psiv {
            1 => UsbSpeed::Full,
            2 => UsbSpeed::Low,
            3 => UsbSpeed::High,
            4 => UsbSpeed::Super,
            5 => UsbSpeed::SuperPlus,
            _ => UsbSpeed::Full,
        }
    }

    pub fn max_packet_size_ep0(&self) -> u16 {
        match self {
            UsbSpeed::Low => 8,
            UsbSpeed::Full => 64,
            UsbSpeed::High => 64,
            UsbSpeed::Super | UsbSpeed::SuperPlus => 512,
        }
    }
}

// ─── USB Descriptors ────────────────────────────────────────────────────────

/// USB device class codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbClass {
    /// Defined at interface level
    PerInterface,
    /// Human Interface Device (keyboard, mouse, gamepad)
    Hid,
    /// Mass Storage (USB drives, SD readers)
    MassStorage,
    /// Audio (speakers, microphones)
    Audio,
    /// Video (webcams)
    Video,
    /// Communications (serial, modem)
    Cdc,
    /// Printer
    Printer,
    /// Hub
    Hub,
    /// Vendor-specific
    VendorSpecific,
    /// Other/unknown
    Other(u8),
}

impl UsbClass {
    pub fn from_code(code: u8) -> Self {
        match code {
            0x00 => UsbClass::PerInterface,
            0x01 => UsbClass::Audio,
            0x02 => UsbClass::Cdc,
            0x03 => UsbClass::Hid,
            0x06 => UsbClass::Video,
            0x07 => UsbClass::Printer,
            0x08 => UsbClass::MassStorage,
            0x09 => UsbClass::Hub,
            0xFF => UsbClass::VendorSpecific,
            c    => UsbClass::Other(c),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            UsbClass::PerInterface  => "Composite",
            UsbClass::Hid           => "HID",
            UsbClass::MassStorage   => "Mass Storage",
            UsbClass::Audio         => "Audio",
            UsbClass::Video         => "Video",
            UsbClass::Cdc           => "Communications",
            UsbClass::Printer       => "Printer",
            UsbClass::Hub           => "Hub",
            UsbClass::VendorSpecific => "Vendor Specific",
            UsbClass::Other(_)      => "Other",
        }
    }
}

/// Parsed USB Device Descriptor (18 bytes).
#[derive(Debug, Clone)]
pub struct DeviceDescriptor {
    pub usb_version: u16,
    pub device_class: UsbClass,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub max_packet_size_ep0: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_version: u16,
    pub manufacturer_idx: u8,
    pub product_idx: u8,
    pub serial_idx: u8,
    pub num_configurations: u8,
}

impl DeviceDescriptor {
    /// Parse from raw 18-byte descriptor.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 18 || data[1] != 0x01 { return None; }
        Some(DeviceDescriptor {
            usb_version: u16::from_le_bytes([data[2], data[3]]),
            device_class: UsbClass::from_code(data[4]),
            device_subclass: data[5],
            device_protocol: data[6],
            max_packet_size_ep0: data[7],
            vendor_id: u16::from_le_bytes([data[8], data[9]]),
            product_id: u16::from_le_bytes([data[10], data[11]]),
            device_version: u16::from_le_bytes([data[12], data[13]]),
            manufacturer_idx: data[14],
            product_idx: data[15],
            serial_idx: data[16],
            num_configurations: data[17],
        })
    }
}

/// USB endpoint direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointDirection {
    Out,
    In,
}

/// USB endpoint transfer type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferType {
    Control,
    Isochronous,
    Bulk,
    Interrupt,
}

/// A USB endpoint descriptor.
#[derive(Debug, Clone)]
pub struct EndpointDescriptor {
    pub address: u8,
    pub direction: EndpointDirection,
    pub transfer_type: TransferType,
    pub max_packet_size: u16,
    pub interval: u8,
}

impl EndpointDescriptor {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 7 || data[1] != 0x05 { return None; }
        let address = data[2];
        Some(EndpointDescriptor {
            address: address & 0x0F,
            direction: if address & 0x80 != 0 { EndpointDirection::In } else { EndpointDirection::Out },
            transfer_type: match data[3] & 0x03 {
                0 => TransferType::Control,
                1 => TransferType::Isochronous,
                2 => TransferType::Bulk,
                _ => TransferType::Interrupt,
            },
            max_packet_size: u16::from_le_bytes([data[4], data[5]]) & 0x07FF,
            interval: data[6],
        })
    }

    /// xHCI endpoint index (DCI - Device Context Index).
    pub fn dci(&self) -> u8 {
        let ep_num = self.address & 0x0F;
        if ep_num == 0 { return 1; } // EP0 is always DCI 1
        ep_num * 2 + if self.direction == EndpointDirection::In { 1 } else { 0 }
    }
}

// ─── Transfer Request Blocks (TRBs) ────────────────────────────────────────

/// TRB types (subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrbType {
    Normal          = 1,
    SetupStage      = 2,
    DataStage       = 3,
    StatusStage     = 4,
    Link            = 6,
    NoOp            = 8,
    EnableSlot      = 9,
    DisableSlot     = 10,
    AddressDevice   = 11,
    ConfigureEp     = 12,
    EvaluateContext = 13,
    ResetEndpoint   = 14,
    StopEndpoint    = 15,
    SetTrDequeue    = 16,
    ResetDevice     = 17,
    // Event TRBs
    TransferEvent   = 32,
    CommandComplete = 33,
    PortStatusChange = 34,
}

/// A Transfer Request Block (16 bytes in hardware).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Trb {
    /// Parameter (address or data — meaning depends on type)
    pub parameter: u64,
    /// Status field
    pub status: u32,
    /// Control field (type, cycle bit, flags)
    pub control: u32,
}

impl Trb {
    pub fn new(trb_type: TrbType) -> Self {
        Trb {
            parameter: 0,
            status: 0,
            control: (trb_type as u32) << 10,
        }
    }

    /// Set the cycle bit.
    pub fn with_cycle(mut self, cycle: bool) -> Self {
        if cycle { self.control |= 1; } else { self.control &= !1; }
        self
    }

    /// Get the TRB type from the control field.
    pub fn trb_type(&self) -> u32 {
        (self.control >> 10) & 0x3F
    }

    /// Get the completion code from a command/event TRB.
    pub fn completion_code(&self) -> u8 {
        ((self.status >> 24) & 0xFF) as u8
    }

    /// Get the cycle bit.
    pub fn cycle_bit(&self) -> bool {
        self.control & 1 != 0
    }

    /// Get the slot ID from control field.
    pub fn slot_id(&self) -> u8 {
        ((self.control >> 24) & 0xFF) as u8
    }
}

// ─── Transfer Ring ──────────────────────────────────────────────────────────

/// A ring buffer of TRBs (used for command ring and endpoint transfer rings).
pub struct TransferRing {
    /// TRBs in the ring
    pub trbs: Vec<Trb>,
    /// Current enqueue index
    pub enqueue: usize,
    /// Current dequeue index
    pub dequeue: usize,
    /// Producer cycle state
    pub cycle: bool,
    /// Ring capacity
    pub capacity: usize,
}

impl TransferRing {
    pub fn new(capacity: usize) -> Self {
        let mut trbs = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            trbs.push(Trb { parameter: 0, status: 0, control: 0 });
        }
        TransferRing {
            trbs,
            enqueue: 0,
            dequeue: 0,
            cycle: true,
            capacity,
        }
    }

    /// Enqueue a TRB onto the ring.
    pub fn enqueue_trb(&mut self, mut trb: Trb) -> bool {
        let next = (self.enqueue + 1) % self.capacity;

        // Check if ring is full (leave one slot for Link TRB)
        if next == self.dequeue {
            return false; // Ring full
        }

        // Set cycle bit
        trb = trb.with_cycle(self.cycle);
        self.trbs[self.enqueue] = trb;
        self.enqueue = next;

        // Handle wrap-around: toggle cycle
        if self.enqueue == 0 {
            self.cycle = !self.cycle;
        }

        true
    }

    /// Dequeue a completed TRB (consumer side).
    pub fn dequeue_trb(&mut self) -> Option<Trb> {
        if self.dequeue == self.enqueue {
            return None; // Empty
        }

        let trb = self.trbs[self.dequeue];
        self.dequeue = (self.dequeue + 1) % self.capacity;
        Some(trb)
    }

    /// Number of pending TRBs in the ring.
    pub fn pending(&self) -> usize {
        if self.enqueue >= self.dequeue {
            self.enqueue - self.dequeue
        } else {
            self.capacity - self.dequeue + self.enqueue
        }
    }
}

// ─── USB Device ─────────────────────────────────────────────────────────────

/// USB device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Attached but not yet addressed
    Attached,
    /// Default state (address 0)
    Default,
    /// Addressed (has a device address assigned)
    Addressed,
    /// Configured (ready for data transfer)
    Configured,
    /// Suspended
    Suspended,
    /// Error state
    Error,
}

/// A USB device.
#[derive(Debug, Clone)]
pub struct UsbDevice {
    /// xHCI slot ID (1-based)
    pub slot_id: u8,
    /// USB device address
    pub device_address: u8,
    /// Device speed
    pub speed: UsbSpeed,
    /// Root hub port number
    pub port: u8,
    /// Current state
    pub state: DeviceState,
    /// Parsed device descriptor
    pub descriptor: Option<DeviceDescriptor>,
    /// Device name (from string descriptor)
    pub product_name: String,
    /// Manufacturer name
    pub manufacturer: String,
    /// Serial number
    pub serial: String,
    /// Endpoints
    pub endpoints: Vec<EndpointDescriptor>,
    /// Active configuration
    pub configuration: u8,
}

impl UsbDevice {
    pub fn new(slot_id: u8, port: u8, speed: UsbSpeed) -> Self {
        UsbDevice {
            slot_id,
            device_address: 0,
            speed,
            port,
            state: DeviceState::Attached,
            descriptor: None,
            product_name: String::new(),
            manufacturer: String::new(),
            serial: String::new(),
            endpoints: Vec::new(),
            configuration: 0,
        }
    }

    /// Device class (from descriptor or PerInterface).
    pub fn class(&self) -> UsbClass {
        self.descriptor.as_ref()
            .map(|d| d.device_class)
            .unwrap_or(UsbClass::Other(0xFF))
    }

    /// Vendor:Product ID string (e.g., "046D:C534").
    pub fn id_string(&self) -> String {
        if let Some(ref desc) = self.descriptor {
            alloc::format!("{:04X}:{:04X}", desc.vendor_id, desc.product_id)
        } else {
            String::from("????:????")
        }
    }
}

// ─── Port Management ────────────────────────────────────────────────────────

/// Port status flags (from PORTSC register).
#[derive(Debug, Clone, Copy)]
pub struct PortStatus {
    /// Is a device connected?
    pub connected: bool,
    /// Is the port enabled?
    pub enabled: bool,
    /// Is the port in reset?
    pub reset: bool,
    /// Port speed (PSIV)
    pub speed: UsbSpeed,
    /// Port power on?
    pub powered: bool,
    /// Connection status changed?
    pub csc: bool,
    /// Port enabled/disabled changed?
    pub pec: bool,
    /// Port reset changed?
    pub prc: bool,
}

impl PortStatus {
    /// Parse from the PORTSC register value.
    pub fn from_portsc(portsc: u32) -> Self {
        PortStatus {
            connected: portsc & (1 << 0) != 0,
            enabled:   portsc & (1 << 1) != 0,
            reset:     portsc & (1 << 4) != 0,
            speed:     UsbSpeed::from_port_speed(((portsc >> 10) & 0xF) as u8),
            powered:   portsc & (1 << 9) != 0,
            csc:       portsc & (1 << 17) != 0,
            pec:       portsc & (1 << 18) != 0,
            prc:       portsc & (1 << 21) != 0,
        }
    }
}

// ─── xHCI Host Controller ──────────────────────────────────────────────────

/// USB transfer completion codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionCode {
    Success,
    DataBuffer,
    BabbleDetected,
    TransactionError,
    TrbError,
    Stall,
    ShortPacket,
    RingUnderrun,
    RingOverrun,
    NoSlots,
    SlotNotEnabled,
    Other(u8),
}

impl CompletionCode {
    pub fn from_code(code: u8) -> Self {
        match code {
            1  => CompletionCode::Success,
            2  => CompletionCode::DataBuffer,
            3  => CompletionCode::BabbleDetected,
            4  => CompletionCode::TransactionError,
            5  => CompletionCode::TrbError,
            6  => CompletionCode::Stall,
            13 => CompletionCode::ShortPacket,
            14 => CompletionCode::RingUnderrun,
            15 => CompletionCode::RingOverrun,
            9  => CompletionCode::NoSlots,
            11 => CompletionCode::SlotNotEnabled,
            c  => CompletionCode::Other(c),
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, CompletionCode::Success | CompletionCode::ShortPacket)
    }
}

/// USB controller statistics.
#[derive(Debug, Clone, Default)]
pub struct UsbStats {
    pub devices_enumerated: u64,
    pub devices_removed: u64,
    pub control_transfers: u64,
    pub bulk_transfers: u64,
    pub interrupt_transfers: u64,
    pub bytes_transferred: u64,
    pub errors: u64,
    pub stalls: u64,
    pub port_status_changes: u64,
}

/// The xHCI Host Controller.
pub struct XhciController {
    /// MMIO base address
    pub mmio_base: u64,
    /// Operational registers base
    pub op_base: u64,
    /// Runtime registers base
    pub rt_base: u64,
    /// Doorbell array base
    pub db_base: u64,
    /// Number of ports
    pub num_ports: u8,
    /// Maximum device slots
    pub max_slots: u8,
    /// Connected devices
    pub devices: Vec<UsbDevice>,
    /// Command ring
    pub command_ring: TransferRing,
    /// Event ring (consumer)
    pub event_ring: TransferRing,
    /// Per-slot transfer rings (slot_id → ring)
    pub transfer_rings: Vec<Option<TransferRing>>,
    /// Is the controller running?
    pub running: bool,
    /// Stats
    pub stats: UsbStats,
}

impl XhciController {
    /// Initialize from PCI BAR address.
    pub fn new(mmio_base: u64) -> Self {
        // Read capability length to find operational registers
        let cap_length = unsafe {
            core::ptr::read_volatile(mmio_base as *const u8)
        } as u64;

        // Read HCSPARAMS1 for port count and max slots
        let hcsparams1 = unsafe {
            core::ptr::read_volatile((mmio_base + cap_regs::HCSPARAMS1 as u64) as *const u32)
        };
        let max_slots = (hcsparams1 & 0xFF) as u8;
        let num_ports = ((hcsparams1 >> 24) & 0xFF) as u8;

        // Read doorbell and runtime offsets
        let dboff = unsafe {
            core::ptr::read_volatile((mmio_base + cap_regs::DBOFF as u64) as *const u32)
        } as u64;
        let rtsoff = unsafe {
            core::ptr::read_volatile((mmio_base + cap_regs::RTSOFF as u64) as *const u32)
        } as u64;

        let op_base = mmio_base + cap_length;

        // Create per-slot transfer ring slots (256 max)
        let mut transfer_rings = Vec::with_capacity(max_slots as usize + 1);
        for _ in 0..=max_slots {
            transfer_rings.push(None);
        }

        crate::serial_println!(
            "[USB] xHCI controller at {:#X}: {} ports, {} slots",
            mmio_base, num_ports, max_slots
        );

        XhciController {
            mmio_base,
            op_base,
            rt_base: mmio_base + rtsoff,
            db_base: mmio_base + dboff,
            num_ports,
            max_slots,
            devices: Vec::new(),
            command_ring: TransferRing::new(256),
            event_ring: TransferRing::new(256),
            transfer_rings,
            running: false,
            stats: UsbStats::default(),
        }
    }

    /// Start the controller (set Run/Stop bit).
    pub unsafe fn start(&mut self) {
        // Configure max slots
        let config_addr = (self.op_base + op_regs::CONFIG as u64) as *mut u32;
        core::ptr::write_volatile(config_addr, self.max_slots as u32);

        // Set Run/Stop bit in USBCMD
        let cmd_addr = (self.op_base + op_regs::USBCMD as u64) as *mut u32;
        let cmd = core::ptr::read_volatile(cmd_addr);
        core::ptr::write_volatile(cmd_addr, cmd | 1); // R/S bit

        // Wait for HCHalted to clear
        let sts_addr = (self.op_base + op_regs::USBSTS as u64) as *const u32;
        let mut timeout = 100_000u32;
        while core::ptr::read_volatile(sts_addr) & (1 << 0) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        self.running = timeout > 0;
        if self.running {
            crate::serial_println!("[USB] xHCI controller started");
        } else {
            crate::serial_println!("[USB] ERROR: xHCI controller failed to start");
        }
    }

    /// Stop the controller.
    pub unsafe fn stop(&mut self) {
        let cmd_addr = (self.op_base + op_regs::USBCMD as u64) as *mut u32;
        let cmd = core::ptr::read_volatile(cmd_addr);
        core::ptr::write_volatile(cmd_addr, cmd & !1); // Clear R/S bit
        self.running = false;
        crate::serial_println!("[USB] xHCI controller stopped");
    }

    /// Read a port's status register.
    pub unsafe fn read_port_status(&self, port: u8) -> PortStatus {
        let portsc_addr = (self.op_base
            + op_regs::PORT_BASE as u64
            + (port as u64) * op_regs::PORT_STRIDE as u64) as *const u32;
        PortStatus::from_portsc(core::ptr::read_volatile(portsc_addr))
    }

    /// Reset a port (to enable a newly attached device).
    pub unsafe fn reset_port(&mut self, port: u8) {
        let portsc_addr = (self.op_base
            + op_regs::PORT_BASE as u64
            + (port as u64) * op_regs::PORT_STRIDE as u64) as *mut u32;

        // Set Port Reset bit (bit 4), preserve RW bits, clear RW1C bits by writing 0
        let portsc = core::ptr::read_volatile(portsc_addr);
        let preserve = portsc & 0x0E01C3E0; // RW bits mask
        core::ptr::write_volatile(portsc_addr, preserve | (1 << 4));

        // Wait for PRC (port reset change) - port reset complete
        let mut timeout = 500_000u32;
        loop {
            let status = core::ptr::read_volatile(portsc_addr);
            if status & (1 << 21) != 0 { break; } // PRC set
            if timeout == 0 { break; }
            timeout -= 1;
            core::hint::spin_loop();
        }

        // Clear PRC by writing 1 to it
        let portsc = core::ptr::read_volatile(portsc_addr);
        core::ptr::write_volatile(portsc_addr, (portsc & 0x0E01C3E0) | (1 << 21));
    }

    /// Scan all ports for connected devices.
    pub unsafe fn scan_ports(&mut self) {
        for port in 0..self.num_ports {
            let status = self.read_port_status(port);

            if status.connected && !status.enabled {
                // New device — reset port to enable it
                crate::serial_println!(
                    "[USB] Device detected on port {} ({:?})",
                    port, status.speed
                );
                self.reset_port(port);
                let status = self.read_port_status(port);

                if status.enabled {
                    self.enumerate_device(port, status.speed);
                }
            }
        }
    }

    /// Enumerate a newly attached device on a port.
    fn enumerate_device(&mut self, port: u8, speed: UsbSpeed) {
        // Allocate a device slot
        let slot_id = self.allocate_slot();
        if slot_id == 0 {
            crate::serial_println!("[USB] ERROR: No slots available for device on port {}", port);
            return;
        }

        let mut device = UsbDevice::new(slot_id, port, speed);
        device.state = DeviceState::Default;
        device.device_address = slot_id; // Simplified: address = slot_id

        // Create transfer ring for this device's EP0
        if (slot_id as usize) < self.transfer_rings.len() {
            self.transfer_rings[slot_id as usize] = Some(TransferRing::new(64));
        }

        // Address the device
        self.send_address_device(slot_id);
        device.state = DeviceState::Addressed;

        self.devices.push(device);
        self.stats.devices_enumerated += 1;

        crate::serial_println!(
            "[USB] Device enumerated: slot={}, port={}, speed={:?}",
            slot_id, port, speed
        );
    }

    /// Allocate a device slot via Enable Slot command.
    fn allocate_slot(&mut self) -> u8 {
        let existing: Vec<u8> = self.devices.iter().map(|d| d.slot_id).collect();
        for slot in 1..=self.max_slots {
            if !existing.contains(&slot) {
                return slot;
            }
        }
        0 // No slots available
    }

    /// Send Address Device command TRB.
    fn send_address_device(&mut self, slot_id: u8) {
        let mut trb = Trb::new(TrbType::AddressDevice);
        trb.control |= (slot_id as u32) << 24;
        self.command_ring.enqueue_trb(trb);
        self.ring_doorbell(0, 0); // Host Controller doorbell
        self.stats.control_transfers += 1;
    }

    /// Ring a doorbell to notify the controller.
    fn ring_doorbell(&self, slot_id: u8, target: u8) {
        let db_addr = (self.db_base + (slot_id as u64) * 4) as *mut u32;
        unsafe {
            core::ptr::write_volatile(db_addr, target as u32);
        }
    }

    /// Handle a port status change event.
    pub fn handle_port_change(&mut self, port: u8) {
        self.stats.port_status_changes += 1;
        let status = unsafe { self.read_port_status(port) };

        if status.connected && status.csc {
            crate::serial_println!("[USB] Device attached on port {}", port);
            if status.enabled {
                self.enumerate_device(port, status.speed);
            }
        } else if !status.connected && status.csc {
            // Device removed
            self.devices.retain(|d| d.port != port);
            self.stats.devices_removed += 1;
            crate::serial_println!("[USB] Device removed from port {}", port);
        }
    }

    /// Process event ring entries.
    pub fn process_events(&mut self) {
        while let Some(trb) = self.event_ring.dequeue_trb() {
            let trb_type = trb.trb_type();
            let code = CompletionCode::from_code(trb.completion_code());

            match trb_type {
                33 => { // Command Completion
                    if !code.is_success() {
                        self.stats.errors += 1;
                        crate::serial_println!(
                            "[USB] Command failed: {:?}", code
                        );
                    }
                }
                34 => { // Port Status Change
                    let port = ((trb.parameter >> 24) & 0xFF) as u8;
                    self.handle_port_change(port.saturating_sub(1)); // 1-based → 0-based
                }
                32 => { // Transfer Event
                    if code.is_success() {
                        let bytes = (trb.status & 0xFFFFFF) as u64;
                        self.stats.bytes_transferred += bytes;
                    } else if matches!(code, CompletionCode::Stall) {
                        self.stats.stalls += 1;
                    } else {
                        self.stats.errors += 1;
                    }
                }
                _ => {}
            }
        }
    }

    /// Get a device by slot ID.
    pub fn device(&self, slot_id: u8) -> Option<&UsbDevice> {
        self.devices.iter().find(|d| d.slot_id == slot_id)
    }

    /// Get all connected devices.
    pub fn connected_devices(&self) -> &[UsbDevice] {
        &self.devices
    }

    /// Get devices of a specific class.
    pub fn devices_by_class(&self, class: UsbClass) -> Vec<&UsbDevice> {
        self.devices.iter().filter(|d| d.class() == class).collect()
    }
}

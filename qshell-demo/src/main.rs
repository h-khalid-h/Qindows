use std::io::{self, Write, BufRead};
use std::thread;
use std::time::Duration;

const R: &str = "\x1B[0m";
const B: &str = "\x1B[1m";
const D: &str = "\x1B[2m";
const G: &str = "\x1B[38;2;6;214;160m";
const C: &str = "\x1B[38;2;0;200;255m";
const Y: &str = "\x1B[38;2;255;215;0m";
const RD: &str = "\x1B[38;2;255;82;82m";
const M: &str = "\x1B[38;2;200;100;255m";
const W: &str = "\x1B[97m";
const BL: &str = "\x1B[38;2;80;140;255m";

fn main() {
    print!("\x1B[2J\x1B[H");
    io::stdout().flush().unwrap();
    boot();
    repl();
}

fn boot() {
    println!("\n{C}{B}╔══════════════════════════════════════╗{R}");
    println!("{C}{B}║   QINDOWS BOOTLOADER v1.0.0-genesis  ║{R}");
    println!("{C}{B}║   The Final Operating System          ║{R}");
    println!("{C}{B}╚══════════════════════════════════════╝{R}\n");
    sl(300);
    ok("Aether Display: 1920x1080 @ stride 1920 | FB: 0x80000000 (8100 KB)"); sl(150);
    ok("Scanning physical memory layout..."); sl(100);
    ok("Memory: 64 entries, 32768 MB usable RAM"); sl(100);
    ok("Boot info allocated at 0x7E00000"); sl(100);
    ok("Genesis Protocol: BOOTLOADER COMPLETE."); println!(); sl(400);
    println!("{D}Qernel boot sequence initiated...{R}"); sl(200);
    for (p, m) in [
        (" 1","Memory: FrameAllocator initialized — 32768 MiB usable"),
        (" 2","GDT loaded — 64-bit long mode segments active"),
        (" 3","IDT loaded — 256 vectors, 16 exception handlers"),
        (" 4","Local APIC enabled — timer @ 100 Hz"),
        (" 5","Aether Display: framebuffer mapped @ 0x80000000"),
        (" 6","SYSCALL/SYSRET fast-path configured"),
        (" 7","Sentinel AI monitor initialized — 10 Laws enforced"),
        (" 8","Scheduler: Fiber-based CFS active — 1024 Fibers max"),
        (" 9","Timekeeping: HPET + TSC calibrated — 3.2 GHz"),
        ("10","PCI Express: 7 devices discovered"),
        ("11","Prism: Crypto RNG seeded from RDRAND"),
        ("12","ELF Loader: ready for binary loading"),
        ("13","Genesis: System Silo PID 1 spawned"),
        ("14","Service Silos: Prism + Aether + Nexus + Synapse + Q-Shell"),
        ("15","Kernel State: Global singleton initialized"),
    ] { println!(" {G}{B}Phase {p}{R} {G}{m}{R}"); io::stdout().flush().unwrap(); sl(180); }
    println!();sl(300);
    println!("{G}{B}╔══════════════════════════════════════╗{R}");
    println!("{G}{B}║    QINDOWS QERNEL v1.0.0 ONLINE     ║{R}");
    println!("{G}{B}║    15/15 Phases Complete             ║{R}");
    println!("{G}{B}║    Memory · GDT · IDT · APIC        ║{R}");
    println!("{G}{B}║    Aether · Syscall · Sentinel       ║{R}");
    println!("{G}{B}║    Scheduler · Timekeeping           ║{R}");
    println!("{G}{B}║    PCI · Security · Genesis          ║{R}");
    println!("{G}{B}║    6 Silos · 5 IPC Channels          ║{R}");
    println!("{G}{B}╚══════════════════════════════════════╝{R}");
    println!();sl(500);
    println!("{D}Genesis complete. Launching Q-Shell...{R}");sl(400);
}

fn repl() {
    println!("\n  {C}╔═══════════════════════════════════╗{R}");
    println!("  {C}║   Q-Shell v1.0.0-genesis          ║{R}");
    println!("  {C}║   Semantic Command Palette        ║{R}");
    println!("  {C}║   Type 'help' to begin.           ║{R}");
    println!("  {C}╚═══════════════════════════════════╝{R}\n");
    let stdin = io::stdin();
    loop {
        print!("{C}Q{R} {D}⟩{R} {G}System{R} {Y}❯{R} ");
        io::stdout().flush().unwrap();
        let mut input = String::new();
        match stdin.lock().read_line(&mut input) { Ok(0)|Err(_) => break, _ => {} }
        let t = input.trim();
        if t.is_empty() { continue; }
        match t {
            "exit"|"quit"|"logout" => { println!("{D}Q-Shell session ended. Farewell.{R}"); break; }
            "clear"|"cls" => { print!("\x1B[2J\x1B[H"); io::stdout().flush().unwrap(); }
            "help" => help(),
            "version" => version(),
            s if s.starts_with("silo") => silo(s),
            s if s.starts_with("prism") => prism(s),
            s if s.starts_with("mesh") => mesh(s),
            s if s.starts_with("sentinel") => sentinel(s),
            s if s.starts_with("pci") => pci(s),
            s if s.starts_with("memory") => memory(s),
            s if s.starts_with("power") => power(s),
            s if s.starts_with("neofetch") || s.starts_with("sysinfo") => neofetch(),
            u => println!("{RD}Error: Unknown command '{u}'. Type 'help' for available commands.{R}"),
        }
    }
}

fn help() { println!("
{B}{C}Q-Shell v1.0.0{R} — Semantic Command Palette

{Y}SYSTEM:{R}
  │ {G}help{R}           Show this help
  │ {G}version{R}        Display Qindows version
  │ {G}clear{R}          Clear console output
  │ {G}sysinfo{R}        System information
  │ {G}exit{R}           Exit Q-Shell

{Y}PRISM (Object Storage):{R}
  │ {G}prism find{R}     Semantic search objects
  │ {G}prism stats{R}    Storage statistics

{Y}SILO (Process Isolation):{R}
  │ {G}silo list{R}      List active Q-Silos
  │ {G}silo inspect{R}   Detailed Silo info

{Y}MESH (Global Network):{R}
  │ {G}mesh status{R}    Network mesh status
  │ {G}mesh peers{R}     List connected peers
  │ {G}mesh credits{R}   Q-Credits balance

{Y}SENTINEL (Security):{R}
  │ {G}sentinel status{R}  AI auditor status
  │ {G}sentinel laws{R}    Q-Manifest laws

{Y}HARDWARE:{R}
  │ {G}pci list{R}       PCI devices
  │ {G}memory stats{R}   Memory usage
  │ {G}power status{R}   Power status

{Y}FLOW OPERATOR:{R}
  │ Use {C}~>{R} to chain: {D}prism find photos ~> sort date ~> take 5{R}
"); }

fn version() { println!("
  {C}Qindows{R} v1.0.0 Genesis Alpha
  {D}Qernel:{R}    Microkernel RS-1.0 (15-phase boot)
  {D}Prism:{R}     Semantic Object Storage
  {D}Aether:{R}    Vector Compositor (SDF-native)
  {D}Synapse:{R}   Neural AI Engine
  {D}Nexus:{R}     P2P Mesh Networking
  {D}Sentinel:{R}  AI Security Auditor
  {D}Q-Shell:{R}   Semantic Command Palette
  {D}Built:{R}     March 2026
"); }

fn silo(s: &str) { let p: Vec<&str> = s.split_whitespace().collect(); match p.get(1).copied() {
    Some("list") => println!("
  {D}SILO ID   NAME             STATE    RING   HEALTH{R}
  ────────  ───────────────  ───────  ─────  ───────
  {C}[0001]{R}  {W}qernel{R}           {G}ACTIVE{R}   Ring-0  {G}████████████ 100%{R}
  {C}[0002]{R}  {W}sentinel{R}         {G}ACTIVE{R}   Ring-0  {Y}█████████░░░  78%{R}
  {C}[0003]{R}  {W}prism-daemon{R}     {G}ACTIVE{R}   Ring-3  {G}██████████░░  85%{R}
  {C}[0004]{R}  {W}aether{R}           {G}ACTIVE{R}   Ring-3  {Y}████████░░░░  67%{R}
  {C}[0005]{R}  {W}nexus{R}            {G}ACTIVE{R}   Ring-3  {Y}███████░░░░░  58%{R}
  {C}[0006]{R}  {W}synapse{R}          {G}ACTIVE{R}   Ring-3  {Y}██████░░░░░░  50%{R}
  {C}[0007]{R}  {W}q-shell{R}          {G}ACTIVE{R}   Ring-3  {G}████████████ 100%{R}
"),
    Some("inspect") => { let id = p.get(2).unwrap_or(&"0007"); println!("
  {C}Silo {id} — Inspection Report{R}
  ─────────────────────────
  {D}State:{R}    {G}ACTIVE{R}
  {D}Ring:{R}     3 (User Mode)
  {D}Fibers:{R}   12 running, 3 blocked
  {D}Memory:{R}   4.2 MiB mapped (256 pages)
  {D}Caps:{R}     READ, WRITE, EXECUTE, SPAWN
  {D}Health:{R}   {G}92/100{R}
  {D}IPC:{R}      3 channels, 847 msgs processed
"); },
    _ => println!("{RD}Usage: silo [list|inspect <id>]{R}"),
}}

fn prism(s: &str) { let p: Vec<&str> = s.split_whitespace().collect(); match p.get(1).copied() {
    Some("find") => { let q = p.get(2..).map(|a| a.join(" ")).unwrap_or_default();
        let q = if q.is_empty() { "*".to_string() } else { q }; println!("
  {C}🔍 Searching Prism Object Graph for: \"{q}\"{R}
  ────────────────────────────────────────
  {Y}[OID:3a7f2b..]{R}  presentation.qvec     {G}(95% match){R}
  {Y}[OID:8b2c91..]{R}  report-q3-2025.qvec   {G}(82% match){R}
  {Y}[OID:1e9d44..]{R}  meeting-notes.qvec    {Y}(71% match){R}
  {Y}[OID:f7a8e1..]{R}  project-timeline.qvec {Y}(64% match){R}
  {D}4 objects found in 0.3ms{R}
"); },
    Some("stats") => println!("
  {C}Prism Object Storage{R}
  ─────────────────────
  {D}Objects:{R}       12,847
  {D}Total Size:{R}    8.4 GiB
  {D}Deduplicated:{R}  2.1 GiB saved (25%)
  {D}Versions:{R}      34,291 shadow objects
  {D}B-Tree Depth:{R}  4 levels
  {D}Journal:{R}       128 pending transactions
  {D}Integrity:{R}     {G}✓ All checksums valid{R}
"),
    _ => println!("{RD}Usage: prism [find <query>|stats]{R}"),
}}

fn mesh(s: &str) { let p: Vec<&str> = s.split_whitespace().collect(); match p.get(1).copied() {
    Some("status") => println!("
  {C}⬡ Global Mesh Status{R}
  ────────────────────
  {D}State:{R}       {G}CONNECTED{R}
  {D}Peers:{R}       4,291 nodes
  {D}Latency:{R}     12ms (nearest), 183ms (farthest)
  {D}Bandwidth:{R}   2.4 Gbps available
  {D}Antibodies:{R}  847 active threat signatures
  {D}Credits:{R}     {G}1,247 Q₵{R} earned
"),
    Some("peers") => println!("
  {D}MESH PEERS{R}
  ──────────
  {G}🟢{R} node-alpha-7a3f   {G}12ms{R}    Connected     Rep: {G}98{R}
  {G}🟢{R} node-beta-2c8d    {G}28ms{R}    Connected     Rep: {G}95{R}
  {Y}🟡{R} node-gamma-9e1f   {Y}183ms{R}   Degraded      Rep: {Y}72{R}
  {RD}🔴{R} node-delta-4b6a   {RD}---{R}     Disconnected
"),
    Some("credits") => println!("
  {C}Q-Credits Balance{R}
  ─────────────────
  {D}Balance:{R}  {G}1,247 Q₵{R}
  {D}Earned:{R}   89 Q₵ (last 24h)
  {D}Spent:{R}    12 Q₵ (GPU offload)
  {D}Rate:{R}     3.7 Q₵/hr
"),
    _ => println!("{RD}Usage: mesh [status|peers|credits]{R}"),
}}

fn sentinel(s: &str) { let p: Vec<&str> = s.split_whitespace().collect(); match p.get(1).copied() {
    Some("status") => println!("
  {C}🛡 Sentinel AI Auditor — ACTIVE{R}
  ──────────────────────────────
  {D}Laws Enforced:{R}     10/10
  {D}Silos Monitored:{R}   7
  {D}Violations Today:{R}  {G}0{R}
  {D}Antibodies:{R}        847 active
  {D}Health Score:{R}       {G}98/100 (Excellent){R}
  {D}Last Scan:{R}          2s ago
"),
    Some("laws") => println!("
  {Y}THE 10 LAWS OF QINDOWS{R}
  ══════════════════════
  {C}I.{R}    Zero Ambient Authority — Apps launch with nothing
  {C}II.{R}   Immutable Binaries — No self-modifying code
  {C}III.{R}  Asynchronous Everything — All I/O through Q-Ring
  {C}IV.{R}   Vector Native UI — No bitmaps, only SDF math
  {C}V.{R}    Global Deduplication — One copy, many views
  {C}VI.{R}   Silo Sandbox — Every app in hardware isolation
  {C}VII.{R}  Telemetry Transparency — No silent network calls
  {C}VIII.{R} Energy Proportionality — Background deep-slept
  {C}IX.{R}   Universal Namespace — Location-transparent data
  {C}X.{R}    Graceful Degradation — Offline-first design
"),
    _ => println!("{RD}Usage: sentinel [status|laws]{R}"),
}}

fn pci(s: &str) { let p: Vec<&str> = s.split_whitespace().collect(); match p.get(1).copied() {
    Some("list") => println!("
  {D}PCI Express Devices{R}
  ────────────────────
  {C}00:00.0{R}  Host Bridge              {D}Intel Corp{R}
  {C}00:02.0{R}  VGA Compatible Controller {D}Intel UHD 770{R}
  {C}00:1f.0{R}  ISA Bridge               {D}Intel Q670{R}
  {C}00:1f.2{R}  SATA Controller           {D}Intel AHCI{R}
  {C}01:00.0{R}  NVMe Controller           {D}Samsung 990 PRO{R}
  {C}02:00.0{R}  Ethernet Controller       {D}Intel I225-V{R}
  {C}03:00.0{R}  USB Controller            {D}Intel xHCI{R}
"),
    _ => println!("{RD}Usage: pci [list]{R}"),
}}

fn memory(s: &str) { let p: Vec<&str> = s.split_whitespace().collect(); match p.get(1).copied() {
    Some("stats") => println!("
  {C}Physical Memory{R}
  ───────────────
  {D}Total:{R}        32,768 MiB
  {D}Used:{R}         4,291 MiB (13%)  {G}█░░░░░░░░░{R}
  {D}Free:{R}         28,477 MiB
  {D}Kernel Heap:{R}  12 MiB
  {D}Silo Pages:{R}   3,840 MiB
  {D}Page Tables:{R}  128 MiB
  {D}Frame Alloc:{R}  {G}95% free frames{R}
"),
    _ => println!("{RD}Usage: memory [stats]{R}"),
}}

fn power(s: &str) { let p: Vec<&str> = s.split_whitespace().collect(); match p.get(1).copied() {
    Some("status") => println!("
  {C}Power Management{R}
  ────────────────
  {D}State:{R}        S0 (Active)
  {D}Policy:{R}       Adaptive
  {D}CPU Freq:{R}     3.2 GHz (scaled)
  {D}Temperature:{R}  52°C
  {D}Power Draw:{R}   28W
  {D}Battery:{R}      N/A (AC Power)
"),
    _ => println!("{RD}Usage: power [status]{R}"),
}}

fn neofetch() { println!("
  {C}{B}        ██████████          {R}{G}root{R}@{G}qindows{R}
  {C}      ██{R}{BL}░░░░░░░░{R}{C}██        {R}{D}OS:{R}       Qindows v1.0.0 Genesis
  {C}    ██{R}{BL}░░░░░░░░░░░░{R}{C}██      {R}{D}Kernel:{R}   Qernel RS-1.0 (Microkernel)
  {C}   ██{R}{BL}░░░░░░░░░░░░░░{R}{C}██     {R}{D}Uptime:{R}   Since boot
  {C}  ██{R}{BL}░░░░██████░░░░░░{R}{C}██    {R}{D}Silos:{R}    7 (1 kernel + 6 user)
  {C}  ██{R}{BL}░░██      ██░░░░{R}{C}██    {R}{D}Memory:{R}   4,291 / 32,768 MiB (13%)
  {C}  ██{R}{BL}░░██  {Y}Q{R}{BL}  ██░░░░{R}{C}██    {R}{D}CPU:{R}      x86_64 @ 3.2 GHz
  {C}  ██{R}{BL}░░██      ██░░░░{R}{C}██    {R}{D}Display:{R}  1920x1080 (Aether SDF)
  {C}  ██{R}{BL}░░░░██████░░░░░░{R}{C}██    {R}{D}Shell:{R}    Q-Shell v1.0.0
  {C}   ██{R}{BL}░░░░░░░░░░░░░░{R}{C}██     {R}{D}Mesh:{R}     4,291 peers connected
  {C}    ██{R}{BL}░░░░░░░░░░░░{R}{C}██      {R}{D}Sentinel:{R} 10/10 Laws enforced
  {C}      ██{R}{BL}░░░░░░░░{R}{C}██        {R}{D}Credits:{R}  1,247 Q₵
  {C}        ██████████          {R}
"); }

fn ok(m: &str) { println!("{D}[{G}OK{D}]{R} {m}"); io::stdout().flush().unwrap(); }
fn sl(ms: u64) { thread::sleep(Duration::from_millis(ms)); }

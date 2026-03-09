// ═══════════════════════════════════════════════════════════════
//  QINDOWS DESKTOP — Enhanced Window Manager & Shell
// ═══════════════════════════════════════════════════════════════

// ═══════════ BOOT SEQUENCE ═══════════
const bootPhases = [
    'Memory: FrameAllocator initialized',
    'GDT loaded — 64-bit long mode',
    'IDT loaded — 256 vectors',
    'Local APIC enabled',
    'Aether Display: framebuffer mapped',
    'SYSCALL/SYSRET configured',
    'Sentinel AI initialized',
    'Scheduler: Fiber-based CFS',
    'Timekeeping: HPET + TSC',
    'PCI Express: 7 devices',
    'Prism: RNG seeded',
    'ELF Loader ready',
    'Genesis: System Silo spawned',
    'Service Silos online',
    'Kernel State initialized',
];

let bootIdx = 0;
function bootTick() {
    if (bootIdx >= bootPhases.length) {
        document.getElementById('bootFill').style.width = '100%';
        document.getElementById('bootText').textContent = 'Qindows ready.';
        setTimeout(() => {
            document.getElementById('bootScreen').classList.add('hide');
            document.getElementById('desktop').classList.add('show');
            updateClock();
            setInterval(updateClock, 1000);
            setTimeout(() => showNotification('🛡️', 'Sentinel', 'All 10 Laws enforced. System healthy.'), 1000);
            setTimeout(() => showNotification('⬡', 'Mesh', 'Connected to 4,291 peers. 1,247 Q₵ earned.'), 3000);
        }, 600);
        return;
    }
    document.getElementById('bootFill').style.width = ((bootIdx + 1) / bootPhases.length * 100) + '%';
    document.getElementById('bootText').textContent = `Phase ${bootIdx + 1}/15: ${bootPhases[bootIdx]}`;
    bootIdx++;
    setTimeout(bootTick, 150 + Math.random() * 100);
}
setTimeout(bootTick, 500);

// ═══════════ CLOCK ═══════════
function updateClock() {
    const now = new Date();
    document.getElementById('clock').textContent = now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    document.getElementById('date').textContent = now.toLocaleDateString([], { month: 'short', day: 'numeric' });
    const lt = document.getElementById('lockTime');
    if (lt) lt.textContent = now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    const ld = document.getElementById('lockDate');
    if (ld) ld.textContent = now.toLocaleDateString([], { weekday: 'long', month: 'long', day: 'numeric' });
}

// ═══════════ WINDOW MANAGER ═══════════
let windows = {};
let zCounter = 100;
let activeWindow = null;

const windowDefs = {
    terminal: { title: 'Q-Shell', icon: '🖥️', w: 700, h: 450, x: 200, y: 80, content: createTerminal },
    files: { title: 'Prism Files', icon: '📁', w: 650, h: 420, x: 280, y: 100, content: createFileManager },
    monitor: { title: 'Silo Monitor', icon: '📊', w: 580, h: 400, x: 350, y: 60, content: createMonitor },
    settings: { title: 'Settings', icon: '⚙️', w: 540, h: 420, x: 400, y: 120, content: createSettings },
    about: { title: 'About Qindows', icon: 'ℹ️', w: 420, h: 340, x: 450, y: 150, content: createAbout },
    synapse: { title: 'Synapse AI', icon: '🧠', w: 500, h: 460, x: 320, y: 70, content: createSynapse },
};

function openWindow(id) {
    if (windows[id]) { focusWindow(id); if (windows[id].el.classList.contains('minimized')) windows[id].el.classList.remove('minimized'); return; }
    const def = windowDefs[id];
    if (!def) return;
    const el = document.createElement('div');
    el.className = 'q-window focused';
    el.dataset.winId = id;
    el.style.cssText = `left:${def.x}px;top:${def.y}px;width:${def.w}px;height:${def.h}px;z-index:${++zCounter}`;
    el.innerHTML = `<div class="win-titlebar" data-win="${id}"><div class="win-dots"><div class="win-dot red" onclick="closeWindow('${id}')"></div><div class="win-dot yellow" onclick="minimizeWindow('${id}')"></div><div class="win-dot green" onclick="maximizeWindow('${id}')"></div></div><div class="win-title">${def.icon} ${def.title}</div></div><div class="win-body" id="winBody_${id}"></div><div class="resize-handle resize-n"></div><div class="resize-handle resize-s"></div><div class="resize-handle resize-e"></div><div class="resize-handle resize-w"></div><div class="resize-handle resize-ne"></div><div class="resize-handle resize-nw"></div><div class="resize-handle resize-se"></div><div class="resize-handle resize-sw"></div>`;
    document.getElementById('winContainer').appendChild(el);
    windows[id] = { el, def, maximized: false, prevBounds: null };
    def.content(document.getElementById(`winBody_${id}`));
    makeDraggable(el, el.querySelector('.win-titlebar'));
    makeResizable(el);
    el.addEventListener('mousedown', () => focusWindow(id));
    focusWindow(id);
    updateTaskbar();
}

function closeWindow(id) {
    if (!windows[id]) return;
    const el = windows[id].el;
    el.classList.add('closing');
    setTimeout(() => { el.remove(); delete windows[id]; updateTaskbar(); }, 150);
}
function minimizeWindow(id) { if (!windows[id]) return; windows[id].el.classList.add('minimized'); updateTaskbar(); }
function maximizeWindow(id) {
    if (!windows[id]) return;
    const w = windows[id];
    if (!w.maximized) {
        w.prevBounds = { left: w.el.style.left, top: w.el.style.top, width: w.el.style.width, height: w.el.style.height };
    }
    w.maximized = !w.maximized;
    w.el.classList.toggle('maximized');
    if (!w.maximized && w.prevBounds) {
        Object.assign(w.el.style, w.prevBounds);
    }
}
function focusWindow(id) {
    if (activeWindow && windows[activeWindow]) windows[activeWindow].el.classList.remove('focused');
    activeWindow = id;
    if (windows[id]) { windows[id].el.classList.add('focused'); windows[id].el.style.zIndex = ++zCounter; }
    updateTaskbar();
}

// ═══════════ WINDOW DRAGGING WITH SNAP ═══════════
function makeDraggable(el, handle) {
    let ox, oy, sx, sy;
    const snap = document.getElementById('snapPreview');
    handle.addEventListener('mousedown', (e) => {
        if (e.target.classList.contains('win-dot')) return;
        if (el.classList.contains('maximized')) {
            // Un-maximize on drag
            const w = windows[el.dataset.winId];
            if (w) { w.maximized = false; el.classList.remove('maximized'); if (w.prevBounds) Object.assign(el.style, w.prevBounds); }
        }
        ox = e.clientX; oy = e.clientY;
        sx = parseInt(el.style.left); sy = parseInt(el.style.top);
        function move(e) {
            el.style.left = (sx + e.clientX - ox) + 'px';
            el.style.top = (sy + e.clientY - oy) + 'px';
            // Snap detection
            const snapZone = getSnapZone(e.clientX, e.clientY);
            if (snapZone) { snap.style.cssText = snapZone; snap.classList.add('show'); }
            else snap.classList.remove('show');
        }
        function up(e) {
            document.removeEventListener('mousemove', move);
            document.removeEventListener('mouseup', up);
            const zone = getSnapZone(e.clientX, e.clientY);
            snap.classList.remove('show');
            if (zone) applySnap(el, e.clientX, e.clientY);
        }
        document.addEventListener('mousemove', move);
        document.addEventListener('mouseup', up);
    });
}

function getSnapZone(x, y) {
    const W = innerWidth, H = innerHeight - 52, T = 6;
    if (x <= T) return y <= H / 2 ? `left:0;top:0;width:${W / 2}px;height:${H / 2}px;display:block` : y >= H / 2 ? `left:0;top:${H / 2}px;width:${W / 2}px;height:${H / 2}px;display:block` : `left:0;top:0;width:${W / 2}px;height:${H}px;display:block`;
    if (x >= W - T) return y <= H / 2 ? `left:${W / 2}px;top:0;width:${W / 2}px;height:${H / 2}px;display:block` : y >= H / 2 ? `left:${W / 2}px;top:${H / 2}px;width:${W / 2}px;height:${H / 2}px;display:block` : `left:${W / 2}px;top:0;width:${W / 2}px;height:${H}px;display:block`;
    if (y <= T) return `left:0;top:0;width:${W}px;height:${H}px;display:block`;
    return null;
}

function applySnap(el, x, y) {
    const W = innerWidth, H = innerHeight - 52, T = 6;
    if (x <= T) { el.style.left = '0'; el.style.top = '0'; el.style.width = W / 2 + 'px'; el.style.height = (y <= H / 2 ? H / 2 : y >= H / 2 ? H / 2 : H) + 'px'; if (y > H / 2) el.style.top = H / 2 + 'px'; }
    else if (x >= W - T) { el.style.left = W / 2 + 'px'; el.style.top = '0'; el.style.width = W / 2 + 'px'; el.style.height = (y <= H / 2 ? H / 2 : y >= H / 2 ? H / 2 : H) + 'px'; if (y > H / 2) el.style.top = H / 2 + 'px'; }
    else if (y <= T) { el.style.left = '0'; el.style.top = '0'; el.style.width = W + 'px'; el.style.height = H + 'px'; }
}

// ═══════════ WINDOW RESIZE ═══════════
function makeResizable(el) {
    el.querySelectorAll('.resize-handle').forEach(handle => {
        handle.addEventListener('mousedown', (e) => {
            e.stopPropagation();
            const dir = handle.className.replace('resize-handle resize-', '');
            let sx = e.clientX, sy = e.clientY;
            let ow = parseInt(el.style.width), oh = parseInt(el.style.height);
            let ol = parseInt(el.style.left), ot = parseInt(el.style.top);
            function move(e) {
                let dx = e.clientX - sx, dy = e.clientY - sy;
                if (dir.includes('e')) el.style.width = Math.max(300, ow + dx) + 'px';
                if (dir.includes('s')) el.style.height = Math.max(200, oh + dy) + 'px';
                if (dir.includes('w')) { let nw = Math.max(300, ow - dx); el.style.width = nw + 'px'; el.style.left = ol + (ow - nw) + 'px'; }
                if (dir.includes('n')) { let nh = Math.max(200, oh - dy); el.style.height = nh + 'px'; el.style.top = ot + (oh - nh) + 'px'; }
            }
            function up() { document.removeEventListener('mousemove', move); document.removeEventListener('mouseup', up); }
            document.addEventListener('mousemove', move);
            document.addEventListener('mouseup', up);
        });
    });
}

function updateTaskbar() {
    const bar = document.getElementById('taskbarApps');
    bar.innerHTML = '';
    for (const [id, win] of Object.entries(windows)) {
        const btn = document.createElement('button');
        btn.className = 'tb-btn' + (id === activeWindow && !win.el.classList.contains('minimized') ? ' active' : '');
        btn.title = win.def.title;
        btn.textContent = win.def.icon;
        btn.onclick = () => { if (win.el.classList.contains('minimized')) { win.el.classList.remove('minimized'); focusWindow(id); } else if (id === activeWindow) { minimizeWindow(id); } else { focusWindow(id); } };
        bar.appendChild(btn);
    }
}

// ═══════════ TERMINAL ═══════════
function createTerminal(container) {
    const output = document.createElement('div');
    output.className = 'terminal-content';
    output.id = 'termOutput';
    container.appendChild(output);
    const banner = `<span class="dim">╔═══════════════════════════════════╗</span>
<span class="prompt">║   Q-Shell v1.0.0-genesis          ║</span>
<span class="prompt">║   Semantic Command Palette        ║</span>
<span class="prompt">║   Type 'help' to begin.           ║</span>
<span class="dim">╚═══════════════════════════════════╝</span>
`;
    addTermLine(output, banner);
    addPrompt(output);
    container.addEventListener('click', () => { const inp = container.querySelector('.terminal-input'); if (inp) inp.focus(); });
}

function addTermLine(output, html) {
    const div = document.createElement('div');
    div.className = 'line';
    div.innerHTML = html;
    output.appendChild(div);
}

function addPrompt(output) {
    const line = document.createElement('div');
    line.className = 'terminal-input-line';
    line.innerHTML = `<span class="terminal-prompt"><span class="prompt">Q</span> <span class="dim">⟩</span> <span class="green">System</span> <span class="yellow">❯</span> </span>`;
    const input = document.createElement('input');
    input.type = 'text';
    input.className = 'terminal-input';
    input.autocomplete = 'off';
    input.spellcheck = false;
    let histIdx = -1;
    const hist = window._termHistory || (window._termHistory = []);
    input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') {
            const cmd = input.value.trim();
            input.disabled = true;
            if (cmd) { hist.unshift(cmd); histIdx = -1; executeCommand(output, cmd); }
            else addPrompt(output);
            output.scrollTop = output.scrollHeight;
        }
        if (e.key === 'ArrowUp') { e.preventDefault(); if (histIdx < hist.length - 1) input.value = hist[++histIdx]; }
        if (e.key === 'ArrowDown') { e.preventDefault(); if (histIdx > 0) input.value = hist[--histIdx]; else { histIdx = -1; input.value = ''; } }
        if (e.key === 'Tab') { e.preventDefault(); autoComplete(input); }
    });
    line.appendChild(input);
    output.appendChild(line);
    output.scrollTop = output.scrollHeight;
    setTimeout(() => input.focus(), 50);
}

function autoComplete(input) {
    const v = input.value;
    const all = ['help', 'version', 'clear', 'sysinfo', 'neofetch', 'exit', 'silo list', 'silo inspect', 'prism find', 'prism stats', 'mesh status', 'mesh peers', 'mesh credits', 'sentinel status', 'sentinel laws', 'pci list', 'memory stats', 'power status', 'whoami', 'uptime', 'date', 'hostname'];
    const match = all.filter(c => c.startsWith(v));
    if (match.length === 1) input.value = match[0];
    else if (match.length > 1) {
        const output = input.closest('.terminal-content') || document.getElementById('termOutput');
        addTermLine(output, `<span class="dim">${match.join('  ')}</span>`);
    }
}

function executeCommand(output, cmd) {
    const cmds = {
        help: `<span class="prompt">Q-Shell v1.0.0</span> — Semantic Command Palette

<span class="yellow">SYSTEM:</span>  help · version · clear · sysinfo · neofetch · whoami · uptime · date · hostname · exit
<span class="yellow">PRISM:</span>   prism find · prism stats
<span class="yellow">SILO:</span>    silo list · silo inspect
<span class="yellow">MESH:</span>    mesh status · mesh peers · mesh credits
<span class="yellow">SENTINEL:</span> sentinel status · sentinel laws
<span class="yellow">HARDWARE:</span> pci list · memory stats · power status
<span class="dim">Tip: Use ↑↓ for history, Tab for completion</span>`,

        version: `<span class="prompt">Qindows</span> v1.0.0 Genesis Alpha
<span class="dim">Qernel:</span>   Microkernel RS-1.0 (15-phase boot)
<span class="dim">Aether:</span>   Vector Compositor (SDF-native)
<span class="dim">Prism:</span>    Semantic Object Storage
<span class="dim">Synapse:</span>  Neural AI Engine
<span class="dim">Nexus:</span>    P2P Mesh Networking
<span class="dim">Sentinel:</span> AI Security Auditor
<span class="dim">Q-Shell:</span>  Semantic Command Palette
<span class="dim">Built:</span>    March 2026 · Rust · x86_64`,

        'silo list': `<span class="dim">ID      NAME            STATE    RING   HEALTH</span>
<span class="prompt">[0001]</span>  qernel          <span class="green">ACTIVE</span>   Ring-0  <span class="green">████████████</span> 100%
<span class="prompt">[0002]</span>  sentinel        <span class="green">ACTIVE</span>   Ring-0  <span class="yellow">█████████░░░</span>  78%
<span class="prompt">[0003]</span>  prism-daemon    <span class="green">ACTIVE</span>   Ring-3  <span class="green">██████████░░</span>  85%
<span class="prompt">[0004]</span>  aether          <span class="green">ACTIVE</span>   Ring-3  <span class="yellow">████████░░░░</span>  67%
<span class="prompt">[0005]</span>  nexus           <span class="green">ACTIVE</span>   Ring-3  <span class="yellow">███████░░░░░</span>  58%
<span class="prompt">[0006]</span>  synapse         <span class="green">ACTIVE</span>   Ring-3  <span class="yellow">██████░░░░░░</span>  50%
<span class="prompt">[0007]</span>  q-shell         <span class="green">ACTIVE</span>   Ring-3  <span class="green">████████████</span> 100%`,

        'silo inspect': `<span class="prompt">Silo 0007 — q-shell</span>
<span class="dim">State:</span>    <span class="green">ACTIVE</span>
<span class="dim">Ring:</span>     3 (User Mode)
<span class="dim">Fibers:</span>   12 running, 3 blocked
<span class="dim">Memory:</span>   4.2 MiB mapped (256 pages)
<span class="dim">Caps:</span>     READ, WRITE, EXECUTE, SPAWN
<span class="dim">Health:</span>   <span class="green">92/100</span>
<span class="dim">IPC:</span>      3 channels, 847 msgs processed`,

        'mesh status': `<span class="prompt">⬡ Global Mesh Status</span>
<span class="dim">State:</span>       <span class="green">CONNECTED</span>
<span class="dim">Peers:</span>       4,291 nodes
<span class="dim">Latency:</span>     12ms (nearest), 183ms (farthest)
<span class="dim">Bandwidth:</span>   2.4 Gbps available
<span class="dim">Antibodies:</span>  847 active threat signatures
<span class="dim">Credits:</span>     <span class="green">1,247 Q₵</span> earned`,

        'mesh peers': `<span class="dim">MESH PEERS</span>
<span class="green">🟢</span> node-alpha-7a3f   <span class="green">12ms</span>    Connected     Rep: <span class="green">98</span>
<span class="green">🟢</span> node-beta-2c8d    <span class="green">28ms</span>    Connected     Rep: <span class="green">95</span>
<span class="yellow">🟡</span> node-gamma-9e1f   <span class="yellow">183ms</span>   Degraded      Rep: <span class="yellow">72</span>
<span class="red">🔴</span> node-delta-4b6a   <span class="red">---</span>     Disconnected`,

        'mesh credits': `<span class="prompt">Q-Credits Balance</span>
<span class="dim">Balance:</span>  <span class="green">1,247 Q₵</span>
<span class="dim">Earned:</span>   89 Q₵ (last 24h)
<span class="dim">Spent:</span>    12 Q₵ (GPU offload)
<span class="dim">Rate:</span>     3.7 Q₵/hr`,

        'sentinel laws': `<span class="yellow">THE 10 LAWS OF QINDOWS</span>
<span class="prompt">I.</span>   Zero Ambient Authority — Apps launch with nothing
<span class="prompt">II.</span>  Immutable Binaries — No self-modifying code
<span class="prompt">III.</span> Asynchronous Everything — All I/O through Q-Ring
<span class="prompt">IV.</span>  Vector Native UI — No bitmaps, only SDF math
<span class="prompt">V.</span>   Global Deduplication — One copy, many views
<span class="prompt">VI.</span>  Silo Sandbox — Every app in hardware isolation
<span class="prompt">VII.</span> Telemetry Transparency — No silent network calls
<span class="prompt">VIII.</span>Energy Proportionality — Background deep-slept
<span class="prompt">IX.</span>  Universal Namespace — Location-transparent data
<span class="prompt">X.</span>   Graceful Degradation — Offline-first design`,

        'sentinel status': `<span class="prompt">🛡 Sentinel AI Auditor — ACTIVE</span>
<span class="dim">Laws:</span>       10/10 enforced
<span class="dim">Monitored:</span>  7 silos
<span class="dim">Violations:</span> <span class="green">0</span> today
<span class="dim">Antibodies:</span> 847 active
<span class="dim">Health:</span>     <span class="green">98/100 (Excellent)</span>
<span class="dim">Last Scan:</span>  2s ago`,

        'memory stats': `<span class="prompt">Physical Memory</span>
<span class="dim">Total:</span>   32,768 MiB
<span class="dim">Used:</span>    4,291 MiB (13%)  <span class="green">█░░░░░░░░░</span>
<span class="dim">Free:</span>    28,477 MiB
<span class="dim">Heap:</span>    12 MiB
<span class="dim">Silo:</span>    3,840 MiB (7 silos)
<span class="dim">Cache:</span>   439 MiB`,

        'prism stats': `<span class="prompt">Prism Object Storage</span>
<span class="dim">Objects:</span>      12,847
<span class="dim">Size:</span>         8.4 GiB
<span class="dim">Deduplicated:</span> 2.1 GiB saved (25%)
<span class="dim">Versions:</span>     34,291 shadow objects
<span class="dim">B-Tree:</span>       4 levels
<span class="dim">Journal:</span>      128 pending transactions
<span class="dim">Integrity:</span>    <span class="green">✓ All checksums valid</span>`,

        'pci list': `<span class="dim">PCI Express Devices</span>
<span class="prompt">00:00.0</span>  Host Bridge              <span class="dim">Intel Corp</span>
<span class="prompt">00:02.0</span>  VGA Compatible Controller <span class="dim">Intel UHD 770</span>
<span class="prompt">00:1f.0</span>  ISA Bridge               <span class="dim">Intel Q670</span>
<span class="prompt">00:1f.2</span>  SATA Controller          <span class="dim">Intel AHCI</span>
<span class="prompt">01:00.0</span>  NVMe Controller          <span class="dim">Samsung 990 PRO</span>
<span class="prompt">02:00.0</span>  Ethernet Controller      <span class="dim">Intel I225-V</span>
<span class="prompt">03:00.0</span>  USB Controller           <span class="dim">Intel xHCI</span>`,

        'power status': `<span class="prompt">Power Management</span>
<span class="dim">State:</span>        S0 (Active)
<span class="dim">Policy:</span>       Adaptive
<span class="dim">CPU Freq:</span>     3.2 GHz (scaled from 4.5 GHz)
<span class="dim">Temperature:</span>  52°C
<span class="dim">Power Draw:</span>   28W
<span class="dim">Battery:</span>      N/A (AC Power)
<span class="dim">Fans:</span>         1,200 RPM`,

        'whoami': `<span class="green">root</span>@qindows [Ring-0] — Q-Admin privileges`,
        'hostname': `<span class="prompt">qindows-genesis</span>.local`,
        'uptime': `<span class="dim">up</span> ${Math.floor(Math.random() * 48 + 1)} hours, ${Math.floor(Math.random() * 59 + 1)} minutes`,
        'date': `<span class="prompt">${new Date().toString()}</span>`,

        'neofetch': `<span class="prompt">        ██████████          </span><span class="green">root</span>@<span class="green">qindows</span>
<span class="prompt">      ██</span><span class="blue">░░░░░░░░</span><span class="prompt">██        </span><span class="dim">OS:</span>       Qindows v1.0.0 Genesis
<span class="prompt">    ██</span><span class="blue">░░░░░░░░░░░░</span><span class="prompt">██      </span><span class="dim">Kernel:</span>   Qernel RS-1.0 (Microkernel)
<span class="prompt">   ██</span><span class="blue">░░░░░░░░░░░░░░</span><span class="prompt">██     </span><span class="dim">Uptime:</span>   Since boot
<span class="prompt">  ██</span><span class="blue">░░░░██████░░░░░░</span><span class="prompt">██    </span><span class="dim">Silos:</span>    7 (1 kernel + 6 user)
<span class="prompt">  ██</span><span class="blue">░░██      ██░░░░</span><span class="prompt">██    </span><span class="dim">Memory:</span>   4,291 / 32,768 MiB (13%)
<span class="prompt">  ██</span><span class="blue">░░██  </span><span class="yellow">Q</span><span class="blue">  ██░░░░</span><span class="prompt">██    </span><span class="dim">CPU:</span>      x86_64 @ 3.2 GHz
<span class="prompt">  ██</span><span class="blue">░░██      ██░░░░</span><span class="prompt">██    </span><span class="dim">Display:</span>  1920×1080 (Aether SDF)
<span class="prompt">  ██</span><span class="blue">░░░░██████░░░░░░</span><span class="prompt">██    </span><span class="dim">Shell:</span>    Q-Shell v1.0.0
<span class="prompt">   ██</span><span class="blue">░░░░░░░░░░░░░░</span><span class="prompt">██     </span><span class="dim">Mesh:</span>     4,291 peers connected
<span class="prompt">    ██</span><span class="blue">░░░░░░░░░░░░</span><span class="prompt">██      </span><span class="dim">Sentinel:</span> 10/10 Laws enforced
<span class="prompt">      ██</span><span class="blue">░░░░░░░░</span><span class="prompt">██        </span><span class="dim">Credits:</span>  1,247 Q₵
<span class="prompt">        ██████████          </span>`,

        'sysinfo': null, // alias for neofetch
    };
    cmds['sysinfo'] = cmds['neofetch'];

    if (cmd === 'exit') { closeWindow('terminal'); return; }
    if (cmd.startsWith('prism find')) {
        const q = cmd.slice(10).trim() || '*';
        addTermLine(output, `<span class="prompt">🔍 Searching Prism for: "${q}"</span>
<span class="yellow">[OID:3a7f..]</span> presentation.qvec   <span class="green">(95% match)</span>
<span class="yellow">[OID:8b2c..]</span> report-q3.qvec      <span class="green">(82% match)</span>
<span class="yellow">[OID:1e9d..]</span> meeting-notes.qvec  <span class="yellow">(71% match)</span>
<span class="yellow">[OID:f7a8..]</span> project-timeline    <span class="yellow">(64% match)</span>
<span class="dim">4 results in 0.3ms</span>`);
    } else if (cmd === 'clear') {
        output.innerHTML = '';
    } else if (cmds[cmd]) {
        addTermLine(output, cmds[cmd]);
    } else {
        addTermLine(output, `<span class="red">Unknown: '${cmd}'. Type 'help' for available commands.</span>`);
    }
    addPrompt(output);
}

// ═══════════ FILE MANAGER (with navigation) ═══════════
const fileSystem = {
    'Q:/home/root': [
        { name: 'Documents', type: 'dir', icon: '📁' },
        { name: 'Projects', type: 'dir', icon: '📁' },
        { name: 'Pictures', type: 'dir', icon: '📁' },
        { name: 'Music', type: 'dir', icon: '📁' },
        { name: 'Downloads', type: 'dir', icon: '📁' },
        { name: 'notes.qvec', type: 'file', icon: '📄', size: '14 KB' },
        { name: 'report.qvec', type: 'file', icon: '📊', size: '2.3 MB' },
        { name: 'photo.qvec', type: 'file', icon: '🖼️', size: '4.1 MB' },
        { name: 'track.qvec', type: 'file', icon: '🎵', size: '8.7 MB' },
        { name: 'archive.qpkg', type: 'file', icon: '📦', size: '156 MB' },
        { name: '.config', type: 'dir', icon: '🔧' },
        { name: '.qshell_history', type: 'file', icon: '📜', size: '2 KB' },
    ],
    'Q:/home/root/Documents': [
        { name: '..', type: 'up', icon: '⬆️' },
        { name: 'thesis.qvec', type: 'file', icon: '📄', size: '45 KB' },
        { name: 'budget-2026.qvec', type: 'file', icon: '📊', size: '12 KB' },
        { name: 'presentation.qvec', type: 'file', icon: '📄', size: '3.2 MB' },
        { name: 'contracts', type: 'dir', icon: '📁' },
    ],
    'Q:/home/root/Projects': [
        { name: '..', type: 'up', icon: '⬆️' },
        { name: 'qindows', type: 'dir', icon: '📁' },
        { name: 'synapse-model', type: 'dir', icon: '📁' },
        { name: 'mesh-node', type: 'dir', icon: '📁' },
    ],
    'Q:/home/root/Pictures': [
        { name: '..', type: 'up', icon: '⬆️' },
        { name: 'wallpaper.qvec', type: 'file', icon: '🖼️', size: '8.2 MB' },
        { name: 'screenshot-01.qvec', type: 'file', icon: '🖼️', size: '1.4 MB' },
        { name: 'avatar.qvec', type: 'file', icon: '🖼️', size: '240 KB' },
    ],
};

let currentPath = 'Q:/home/root';

function createFileManager(container) {
    currentPath = 'Q:/home/root';
    container.style.display = 'flex';
    container.innerHTML = `
<div class="fm-sidebar" id="fmSidebar">
    <div class="fm-sidebar-section">Favorites</div>
    <div class="fm-sidebar-item active" data-path="Q:/home/root"><span class="icon">🏠</span>Home</div>
    <div class="fm-sidebar-item" data-path="Q:/home/root/Documents"><span class="icon">📄</span>Documents</div>
    <div class="fm-sidebar-item" data-path="Q:/home/root/Pictures"><span class="icon">🖼️</span>Pictures</div>
    <div class="fm-sidebar-item" data-path="Q:/home/root/Music"><span class="icon">🎵</span>Music</div>
    <div class="fm-sidebar-item" data-path="Q:/home/root/Downloads"><span class="icon">📥</span>Downloads</div>
    <div class="fm-sidebar-section">System</div>
    <div class="fm-sidebar-item"><span class="icon">💾</span>Prism Store</div>
    <div class="fm-sidebar-item"><span class="icon">🔒</span>Vault</div>
    <div class="fm-sidebar-item"><span class="icon">⬡</span>Mesh Shared</div>
</div>
<div style="flex:1;display:flex;flex-direction:column">
    <div class="fm-path" id="fmPath"></div>
    <div class="fm-content"><div class="fm-grid" id="fmGrid"></div></div>
</div>`;
    container.querySelectorAll('.fm-sidebar-item[data-path]').forEach(item => {
        item.addEventListener('click', () => { navigateTo(item.dataset.path, container); });
    });
    navigateTo(currentPath, container);
}

function navigateTo(path, container) {
    currentPath = path;
    const pathEl = container.querySelector('#fmPath') || document.getElementById('fmPath');
    const gridEl = container.querySelector('#fmGrid') || document.getElementById('fmGrid');
    const parts = path.split('/');
    pathEl.innerHTML = parts.map((p, i) => `<span onclick="navigateTo('${parts.slice(0, i + 1).join('/')}', this.closest('.win-body'))">${p}</span>`).join(' / ');
    const files = fileSystem[path] || [{ name: '..', type: 'up', icon: '⬆️' }, { name: '(empty)', type: 'none', icon: '📭' }];
    gridEl.innerHTML = files.map(f => {
        if (f.type === 'none') return `<div class="fm-item"><div class="icon">${f.icon}</div><div class="name">${f.name}</div></div>`;
        const clickAction = f.type === 'dir' ? `navigateTo('${path}/${f.name}', this.closest('.win-body'))` :
            f.type === 'up' ? `navigateTo('${parts.slice(0, -1).join('/')}', this.closest('.win-body'))` :
                `showNotification('📄','${f.name}','${f.size || 'Opening...'} — Prism Object Viewer')`;
        return `<div class="fm-item" ondblclick="${clickAction}"><div class="icon">${f.icon}</div><div class="name">${f.name}</div></div>`;
    }).join('');
    container.querySelectorAll('.fm-sidebar-item').forEach(i => i.classList.toggle('active', i.dataset.path === path));
}

// ═══════════ SILO MONITOR ═══════════
function createMonitor(container) {
    const silos = [
        { id: '0001', name: 'qernel', ring: 'Ring-0', pct: 100, color: 'var(--green)' },
        { id: '0002', name: 'sentinel', ring: 'Ring-0', pct: 78, color: 'var(--yellow)' },
        { id: '0003', name: 'prism-daemon', ring: 'Ring-3', pct: 85, color: 'var(--green)' },
        { id: '0004', name: 'aether', ring: 'Ring-3', pct: 67, color: 'var(--yellow)' },
        { id: '0005', name: 'nexus', ring: 'Ring-3', pct: 58, color: 'var(--yellow)' },
        { id: '0006', name: 'synapse', ring: 'Ring-3', pct: 50, color: 'var(--yellow)' },
        { id: '0007', name: 'q-shell', ring: 'Ring-3', pct: 100, color: 'var(--green)' },
    ];
    container.innerHTML = `<div class="monitor-grid">${silos.map(s => `
<div class="silo-card">
  <span class="silo-id">${s.id}</span>
  <span class="silo-name">${s.name}</span>
  <span class="silo-status active">ACTIVE</span>
  <span class="silo-ring">${s.ring}</span>
  <div class="silo-bar"><div class="silo-fill" style="width:${s.pct}%;background:${s.color}"></div></div>
  <span class="silo-pct">${s.pct}%</span>
</div>`).join('')}</div>`;
}

// ═══════════ SETTINGS ═══════════
function createSettings(container) {
    container.innerHTML = `<div style="padding:20px;display:flex;flex-direction:column;gap:16px;overflow-y:auto">
<div style="font-size:16px;font-weight:600;color:var(--cyan)">⚙️ System Settings</div>
<div style="background:rgba(255,255,255,.03);border:1px solid var(--border);border-radius:8px;padding:14px">
  <div style="font-size:13px;font-weight:500;margin-bottom:8px">Appearance</div>
  <div style="font-size:12px;color:var(--dim);margin-bottom:6px">Theme: <span style="color:var(--cyan)">Midnight (Default)</span></div>
  <div style="font-size:12px;color:var(--dim);margin-bottom:6px">Accent: <span style="color:var(--cyan)">●</span> Cyan &nbsp;|&nbsp; <span style="color:var(--green)">●</span> <span style="cursor:pointer;opacity:.5">Green</span> &nbsp;|&nbsp; <span style="color:var(--magenta)">●</span> <span style="cursor:pointer;opacity:.5">Magenta</span></div>
  <div style="font-size:12px;color:var(--dim)">Corner Radius: 10px (Rounded)</div>
</div>
<div style="background:rgba(255,255,255,.03);border:1px solid var(--border);border-radius:8px;padding:14px">
  <div style="font-size:13px;font-weight:500;margin-bottom:8px">Display</div>
  <div style="font-size:12px;color:var(--dim)">Resolution: 1920×1080</div>
  <div style="font-size:12px;color:var(--dim)">Compositor: Aether SDF Vector Engine</div>
  <div style="font-size:12px;color:var(--dim)">Refresh: 60 Hz</div>
  <div style="font-size:12px;color:var(--dim)">Scaling: 100%</div>
</div>
<div style="background:rgba(255,255,255,.03);border:1px solid var(--border);border-radius:8px;padding:14px">
  <div style="font-size:13px;font-weight:500;margin-bottom:8px">Security</div>
  <div style="font-size:12px;color:var(--dim)">Sentinel: <span style="color:var(--green)">Active — All 10 Laws enforced</span></div>
  <div style="font-size:12px;color:var(--dim)">Capability Model: Zero-Trust</div>
  <div style="font-size:12px;color:var(--dim)">SecureBoot: <span style="color:var(--green)">Verified</span></div>
</div>
<div style="background:rgba(255,255,255,.03);border:1px solid var(--border);border-radius:8px;padding:14px">
  <div style="font-size:13px;font-weight:500;margin-bottom:8px">Network</div>
  <div style="font-size:12px;color:var(--dim)">Mesh: <span style="color:var(--green)">Connected</span> · 4,291 peers</div>
  <div style="font-size:12px;color:var(--dim)">Credits: 1,247 Q₵ · 3.7 Q₵/hr</div>
  <div style="font-size:12px;color:var(--dim)">Bandwidth: 2.4 Gbps</div>
</div>
<div style="background:rgba(255,255,255,.03);border:1px solid var(--border);border-radius:8px;padding:14px">
  <div style="font-size:13px;font-weight:500;margin-bottom:8px">Storage</div>
  <div style="font-size:12px;color:var(--dim)">Prism Objects: 12,847 (8.4 GiB)</div>
  <div style="font-size:12px;color:var(--dim)">Deduplication: 2.1 GiB saved (25%)</div>
  <div style="font-size:12px;color:var(--dim)">Encryption: AES-256-GCM</div>
</div>
</div>`;
}

// ═══════════ ABOUT ═══════════
function createAbout(container) {
    container.innerHTML = `<div style="padding:30px;text-align:center;display:flex;flex-direction:column;align-items:center;gap:16px">
<div style="font-size:48px;font-weight:700;color:var(--cyan);letter-spacing:-2px"><span style="color:var(--green)">Q</span>indows</div>
<div style="font-size:14px;color:var(--dim)">The Final Operating System</div>
<div style="font-size:12px;color:var(--dim);line-height:1.8">
  Version 1.0.0 Genesis Alpha<br>
  Qernel: Microkernel RS-1.0 (15-phase boot)<br>
  Aether: Vector Compositor (SDF-native)<br>
  Prism: Semantic Object Storage<br>
  Synapse: Neural AI Engine<br>
  Nexus: P2P Mesh Networking<br>
  Sentinel: AI Security Auditor<br>
  Q-Shell: Semantic Command Palette
</div>
<div style="font-size:11px;color:rgba(255,255,255,.2);margin-top:8px">Built March 2026 • Rust • x86_64 • MIT License</div>
</div>`;
}

// ═══════════ SYNAPSE AI ═══════════
const synapseResponses = {
    'hello': 'Hello! I\'m Synapse, the neural AI assistant built into Qindows. I can help you with system commands, file management, code analysis, and general questions. What would you like to know?',
    'what is qindows': 'Qindows is a next-generation operating system built from scratch in Rust. It features a microkernel architecture (Qernel), SDF-based vector rendering (Aether), semantic object storage (Prism), P2P mesh networking (Nexus), and me — Synapse, your neural AI assistant. It\'s designed to be more secure, performant, and intuitive than traditional operating systems.',
    'help': 'I can help with:\n• <span class="prompt">System Info</span> — ask about silos, memory, PCI, network\n• <span class="prompt">File Search</span> — "find my presentation" or "search for photos"\n• <span class="prompt">Commands</span> — "how do I list silos?" or "what commands are available?"\n• <span class="prompt">Architecture</span> — "explain the kernel" or "what is Prism?"\n• <span class="prompt">General</span> — ask me anything!',
    'what is prism': 'Prism is Qindows\' semantic object storage engine. Unlike traditional file systems with folders and files, Prism stores everything as content-addressed objects with semantic tags. Key features:\n\n• <span class="green">B-Tree indexed</span> — O(log n) lookups\n• <span class="green">Global deduplication</span> — saves 25% storage\n• <span class="green">Shadow copies</span> — infinite undo\n• <span class="green">Encryption</span> — AES-256-GCM at rest\n• <span class="green">Semantic search</span> — find files by meaning, not just name',
    'what is aether': 'Aether is the Qindows compositor engine. Instead of rendering pixels, it uses GPU-accelerated Signed Distance Fields (SDF). Every UI element — buttons, text, icons — is a mathematical formula. Benefits:\n\n• <span class="green">Infinite resolution</span> — perfect at any DPI\n• <span class="green">Zero-copy scanout</span> — frames go directly to display\n• <span class="green">Scene graph</span> — UI independent of app state\n• <span class="green">Q-Glass material</span> — real-time glassmorphism',
    'what are silos': 'Q-Silos are Qindows\' process isolation mechanism. Each app runs in a hardware-isolated sandbox with its own:\n\n• Virtual address space\n• Capability tokens (what it\'s allowed to do)\n• IPC channels (how it communicates)\n• Resource quotas\n\nUnlike traditional processes, silos can\'t access anything they weren\'t explicitly granted. This is called "zero ambient authority" — Law I of Qindows.',
};

function createSynapse(container) {
    container.innerHTML = `<div class="synapse-chat">
<div class="synapse-messages" id="synapseMessages">
    <div class="synapse-msg ai"><div class="msg-label">🧠 Synapse</div>Hi! I'm <strong>Synapse</strong>, your neural AI assistant. Ask me anything about Qindows, or type <span style="color:var(--cyan)">help</span> to see what I can do.</div>
</div>
<div class="synapse-input-bar">
    <input class="synapse-input" id="synapseInput" placeholder="Ask Synapse anything..." autocomplete="off">
    <button class="synapse-send" onclick="sendSynapseMsg()">Send</button>
</div>
</div>`;
    document.getElementById('synapseInput').addEventListener('keydown', e => { if (e.key === 'Enter') sendSynapseMsg(); });
}

function sendSynapseMsg() {
    const input = document.getElementById('synapseInput');
    const msgs = document.getElementById('synapseMessages');
    const text = input.value.trim();
    if (!text) return;
    input.value = '';
    msgs.innerHTML += `<div class="synapse-msg user"><div class="msg-label">You</div>${text}</div>`;
    msgs.scrollTop = msgs.scrollHeight;
    setTimeout(() => {
        const lower = text.toLowerCase();
        let response = null;
        for (const [key, val] of Object.entries(synapseResponses)) {
            if (lower.includes(key)) { response = val; break; }
        }
        if (!response) {
            const responses = [
                `That's an interesting question! In the Qindows architecture, this would be handled by the ${['Qernel', 'Prism', 'Aether', 'Nexus', 'Sentinel'][Math.floor(Math.random() * 5)]} subsystem. Would you like me to explain more about how it works?`,
                `Great question! Qindows handles this through its capability-based security model. Every operation must be explicitly authorized through Q-Ring syscalls. Try running <span class="prompt">sentinel laws</span> in Q-Shell to see the 10 Laws.`,
                `I'd be happy to help with that. In Qindows, you can use Q-Shell to explore the system. Try commands like <span class="prompt">silo list</span>, <span class="prompt">prism stats</span>, or <span class="prompt">neofetch</span> to learn more about your system.`,
            ];
            response = responses[Math.floor(Math.random() * responses.length)];
        }
        msgs.innerHTML += `<div class="synapse-msg ai"><div class="msg-label">🧠 Synapse</div>${response}</div>`;
        msgs.scrollTop = msgs.scrollHeight;
    }, 500 + Math.random() * 800);
}

// ═══════════ START MENU ═══════════
function toggleStart() {
    const menu = document.getElementById('startMenu');
    menu.classList.toggle('show');
    if (menu.classList.contains('show')) document.getElementById('startSearch').focus();
    closeTray();
}
function filterApps(q) {
    document.querySelectorAll('.start-app').forEach(a => {
        a.style.display = a.textContent.toLowerCase().includes(q.toLowerCase()) ? '' : 'none';
    });
}

// ═══════════ CONTEXT MENU ═══════════
document.getElementById('desktop').addEventListener('contextmenu', (e) => {
    if (e.target.closest('.q-window') || e.target.closest('#taskbar') || e.target.closest('#startMenu')) return;
    e.preventDefault();
    const ctx = document.getElementById('contextMenu');
    ctx.style.display = 'block';
    ctx.style.left = Math.min(e.clientX, innerWidth - 220) + 'px';
    ctx.style.top = Math.min(e.clientY, innerHeight - 200) + 'px';
});
function hideContext() { document.getElementById('contextMenu').style.display = 'none'; }
document.addEventListener('click', (e) => {
    if (!e.target.closest('#contextMenu')) hideContext();
    if (!e.target.closest('#startMenu') && !e.target.closest('.start-btn')) document.getElementById('startMenu').classList.remove('show');
    if (!e.target.closest('.tray-popup') && !e.target.closest('.tray-item')) closeTray();
});

// ═══════════ NOTIFICATIONS ═══════════
let notifStack = 0;
function showNotification(icon, title, body) {
    const n = document.createElement('div');
    n.className = 'notification';
    n.style.top = (16 + notifStack * 80) + 'px';
    notifStack++;
    n.innerHTML = `<span class="notif-icon">${icon}</span><div class="notif-content"><div class="notif-title">${title}</div><div class="notif-body">${body}</div></div>`;
    document.body.appendChild(n);
    setTimeout(() => { n.classList.add('hiding'); notifStack = Math.max(0, notifStack - 1); setTimeout(() => n.remove(), 300); }, 4000);
}

// ═══════════ TRAY POPUP ═══════════
function toggleTray() {
    const popup = document.getElementById('trayPopup');
    popup.classList.toggle('show');
    document.getElementById('startMenu').classList.remove('show');
}
function closeTray() { document.getElementById('trayPopup').classList.remove('show'); }

// ═══════════ LOCK SCREEN ═══════════
function lockScreen() {
    updateClock();
    document.getElementById('lockScreen').classList.add('show');
    setTimeout(() => document.getElementById('lockInput').focus(), 100);
}
document.getElementById('lockInput').addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
        document.getElementById('lockScreen').classList.remove('show');
        showNotification('🔓', 'Unlocked', 'Welcome back, root');
    }
});

// ═══════════ ALT+TAB ═══════════
let altTabActive = false;
let altTabIdx = 0;
function showAltTab() {
    const ids = Object.keys(windows);
    if (ids.length < 2) return;
    altTabActive = true;
    altTabIdx = 1;
    const overlay = document.getElementById('altTabOverlay');
    overlay.innerHTML = ids.map((id, i) => `<div class="alt-tab-item${i === altTabIdx ? ' selected' : ''}" data-id="${id}"><div class="at-icon">${windows[id].def.icon}</div><div class="at-title">${windows[id].def.title}</div></div>`).join('');
    overlay.classList.add('show');
}
function cycleAltTab() {
    const ids = Object.keys(windows);
    altTabIdx = (altTabIdx + 1) % ids.length;
    document.querySelectorAll('.alt-tab-item').forEach((el, i) => el.classList.toggle('selected', i === altTabIdx));
}
function commitAltTab() {
    const ids = Object.keys(windows);
    if (ids[altTabIdx]) {
        const id = ids[altTabIdx];
        if (windows[id].el.classList.contains('minimized')) windows[id].el.classList.remove('minimized');
        focusWindow(id);
    }
    document.getElementById('altTabOverlay').classList.remove('show');
    altTabActive = false;
}

// ═══════════ KEYBOARD SHORTCUTS ═══════════
document.addEventListener('keydown', (e) => {
    // Alt+Tab
    if (e.altKey && e.key === 'Tab') { e.preventDefault(); if (!altTabActive) showAltTab(); else cycleAltTab(); }
    // Ctrl+T — new terminal
    if (e.ctrlKey && e.key === 't') { e.preventDefault(); openWindow('terminal'); }
    // Ctrl+W — close active window
    if (e.ctrlKey && e.key === 'w') { e.preventDefault(); if (activeWindow) closeWindow(activeWindow); }
    // F1 — About
    if (e.key === 'F1') { e.preventDefault(); openWindow('about'); }
    // Super/Meta — Start menu
    if (e.key === 'Meta') { e.preventDefault(); toggleStart(); }
    // Ctrl+L — Lock
    if (e.ctrlKey && e.key === 'l') { e.preventDefault(); lockScreen(); }
    // Escape
    if (e.key === 'Escape') { document.getElementById('startMenu').classList.remove('show'); hideContext(); closeTray(); }
});
document.addEventListener('keyup', (e) => {
    if (e.key === 'Alt' && altTabActive) commitAltTab();
});

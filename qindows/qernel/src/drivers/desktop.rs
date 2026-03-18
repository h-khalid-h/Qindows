//! # Aether Desktop Renderer
//!
//! Renders the Qindows desktop environment directly on the framebuffer.
//! This is the kernel-space visual shell. Instead of hardcoding rectangles,
//! this now builds a Scene Graph using `aether::renderer::RenderCommand`
//! and then software-rasterizes it directly, ensuring visual consistency
//! with the user-space Aether vector engine.

use crate::drivers::gpu::AetherFrameBuffer;
use crate::drivers::console::FramebufferConsole;
use aether::renderer::{RenderFrame, RenderCommand, Color};

// ── Color Palette ──────────────────────────────────────────────
// The Qindows Aether design system, as ARGB u32 values.
pub const BG_DEEP: u32 = 0xFF_06_06_0E;
pub const BG_SURFACE: u32 = 0xFF_0C_0C_1A; // Panel background
pub const BG_TASKBAR: u32 = 0xFF_0A_0E_17; // Taskbar background
pub const ACCENT_CYAN: u32 = 0xFF_00_F0_FF;
pub const ACCENT_BLUE: u32 = 0xFF_10_70_FF;
pub const ACCENT_GOLD: u32 = 0xFF_FF_D7_00;
pub const TEXT_PRIMARY: u32 = 0xFF_E0_E6_ED;
pub const TEXT_DIM: u32 = 0xFF_8A_96_A6;
pub const BORDER: u32 = 0xFF_1A_20_30; // Subtle borders
pub const STATUS_GREEN: u32 = 0xFF_00_FF_AA;
pub const ALERT_RED: u32 = 0xFF_FF_33_66;
pub const STATUS_YELLOW: u32 = 0xFF_FF_BD_2E; // Warning
pub const STATUS_RED: u32 = 0xFF_EF_47_6F; // Error

// ── Taskbar Constants ──────────────────────────────────────────
const TASKBAR_HEIGHT: usize = 40;
const DESKTOP_ICON_SIZE: usize = 48;

/// Render the full Qindows desktop environment.
///
/// This draws:
/// 1. Desktop background (deep black with subtle gradient)
/// 2. Taskbar at bottom (dark bar with Q button, status indicators, clock)
/// 3. Desktop icons (placeholder visual)
/// 4. System status panel (boot info, silo status)
/// 5. Centered Q logo
pub fn render_desktop(fb: &mut AetherFrameBuffer) {
    let w = fb.width();
    let h = fb.height();

    // ── 1. Desktop Background (Deep Black + Glowing Orbs) ──────
    fb.clear(BG_DEEP);

    let dot_color = 0xFF_20_24_38;
    let spacing = 48;
    for y in (0..h).step_by(spacing) {
        for x in (0..w).step_by(spacing) {
            fb.draw_pixel(x, y, dot_color);
        }
    }

    draw_orb(fb, w / 4, h / 3, 600, ACCENT_CYAN, 30);
    draw_orb(fb, (w * 3) / 4, (h * 2) / 3, 800, ACCENT_BLUE, 25);
    draw_orb(fb, w - 200, 100, 400, ACCENT_GOLD, 15);
    draw_large_q_watermark(fb, w / 2 - 150, h / 2 - 150);

    // ── 2. Build Aether Scene Graph ─────────────────────────────
    let mut frame = RenderFrame::new(w as f32, h as f32, 1.0);

    // Window 1: Q-Shell
    frame.draw_window(100.0, 120.0, 600.0, 400.0, 40.0, true);
    frame.push(RenderCommand::Text { x: 300.0, y: 140.0, text: alloc::string::String::from("Q-Shell - admin@qindows: ~"), size: 14.0, color: Color::from_hex(TEXT_PRIMARY) });

    // Window 2: Monitor
    frame.draw_window(750.0, 200.0, 500.0, 350.0, 40.0, false);
    frame.push(RenderCommand::Text { x: 1000.0, y: 220.0, text: alloc::string::String::from("System Monitor"), size: 14.0, color: Color::from_hex(TEXT_DIM) });

    // Taskbar (Glassy Blur + Stroke)
    let tb_y = (h - TASKBAR_HEIGHT) as f32;
    frame.push(RenderCommand::GlassBlur { x: 0.0, y: tb_y, width: w as f32, height: TASKBAR_HEIGHT as f32, radius: 0.0, blur_radius: 20.0, tint: Color::rgba(0.04, 0.05, 0.09, 0.86) });
    frame.push(RenderCommand::RoundedRect { x: 0.0, y: tb_y, width: w as f32, height: 1.0, radius: 0.0, fill: Color::rgba(1.0, 1.0, 1.0, 0.1), border: None });

    // Q Start button
    frame.push(RenderCommand::RoundedRect { x: 16.0, y: tb_y + 4.0, width: 32.0, height: 32.0, radius: 8.0, fill: Color::from_hex(ACCENT_CYAN), border: None });
    
    // Status panel (floating, top right)
    frame.push(RenderCommand::GlassBlur { x: w as f32 - 300.0, y: 60.0, width: 280.0, height: 140.0, radius: 12.0, blur_radius: 12.0, tint: Color::rgba(0.04, 0.08, 0.08, 0.1) });
    frame.push(RenderCommand::RoundedRect { x: w as f32 - 300.0, y: 60.0, width: 280.0, height: 140.0, radius: 12.0, fill: Color::rgba(0.0,0.0,0.0,0.0), border: Some((1.0, Color::rgba(1.0,1.0,1.0,0.1))) });

    // ── 3. Software Rasterize Scene ─────────────────────────────
    rasterize_aether_frame(fb, &frame);
    
    // Draw raw text overlays for fake window content (until font renderer works)
    // These are now handled by the window manager's render_scene function.
    
    draw_q_letter(fb, 26, (tb_y + 7.0) as usize, BG_DEEP);
}

/// The Aether CPU Software Rasterizer.
/// Resolves the Vector Scene Graph into an ARGB pixel buffer by delegating
/// to the cross-platform Aether compositor library.
fn rasterize_aether_frame(fb: &mut AetherFrameBuffer, frame: &RenderFrame) {
    // We pass a closure to Aether's rasterizer so Aether (no_std, userspace)
    // can write physical pixels to the kernel's framebuffer without linking
    // the kernel or knowing physical memory layout.
    frame.rasterize(|x, y, color| {
        let u32_color = color_to_u32(&color);
        let alpha = (color.a * 255.0) as u32;
        
        let bg = fb.read_pixel(x as usize, y as usize);
        let blended = blend(bg, u32_color, alpha);
        fb.draw_pixel(x as usize, y as usize, blended);
    });
}

#[inline(always)]
fn color_to_u32(color: &Color) -> u32 {
    let r = (color.r * 255.0) as u32;
    let g = (color.g * 255.0) as u32;
    let b = (color.b * 255.0) as u32;
    (r << 16) | (g << 8) | b
}

/// Render the system status text in the status panel area.
/// Called separately because it needs the FramebufferConsole for text.
pub fn render_status_text(
    fb: &mut AetherFrameBuffer,
    console: &mut FramebufferConsole,
    silo_count: usize,
    ipc_channels: usize,
) {
    let w = fb.width();
    let _h = fb.height();
    // We won't draw a solid panel anymore; the new UI dictates
    // floating text or a glassy panel in the top right.
    let panel_w = 280;
    let panel_h = 140;
    let panel_x = w - panel_w - 20;
    let panel_y = 60; // Move to top right

    // Glassy panel background
    draw_rounded_rect_alpha(fb, panel_x, panel_y, panel_w, panel_h, 12, BG_SURFACE, 180);
    // Border
    draw_rounded_rect_alpha(fb, panel_x, panel_y, panel_w, panel_h, 12, 0xFF_FF_FF_FF, 20);

    // Position console cursor at the panel location
    let col = panel_x / 8 + 1;
    let row = panel_y / 16 + 1;

    console.set_cursor(col, row);
    console.set_fg(ACCENT_CYAN);
    console.set_bg(BG_SURFACE);
    console.write_str(fb, " System Status");

    console.set_cursor(col, row + 1);
    console.set_fg(TEXT_DIM);
    console.write_str(fb, " ─────────────────────");

    console.set_cursor(col, row + 2);
    console.set_fg(STATUS_GREEN);
    console.write_str(fb, " [OK]");
    console.set_fg(TEXT_PRIMARY);
    console.write_str(fb, " Kernel: Online");

    console.set_cursor(col, row + 3);
    console.set_fg(STATUS_GREEN);
    console.write_str(fb, " [OK]");
    console.set_fg(TEXT_PRIMARY);
    write_num(console, fb, " Silos: ", silo_count);

    console.set_cursor(col, row + 4);
    console.set_fg(STATUS_GREEN);
    console.write_str(fb, " [OK]");
    console.set_fg(TEXT_PRIMARY);
    write_num(console, fb, " IPC: ", ipc_channels);
    console.write_str(fb, " channels");

    console.set_cursor(col, row + 5);
    console.set_fg(STATUS_GREEN);
    console.write_str(fb, " [OK]");
    console.set_fg(TEXT_PRIMARY);
    console.write_str(fb, " Sentinel: Active");

    console.set_cursor(col, row + 6);
    console.set_fg(ACCENT_CYAN);
    console.write_str(fb, " 15/15 Phases Complete");

    // Reset console colors back to default
    console.set_fg(ACCENT_CYAN);
    console.set_bg(BG_DEEP);
}

/// Render the clock text in the taskbar tray area.
pub fn render_clock(
    fb: &mut AetherFrameBuffer,
    console: &mut FramebufferConsole,
    hours: u8,
    minutes: u8,
    month: u8,
    day: u8,
) {
    let w = fb.width();
    let h = fb.height();
    let tb_y = h - TASKBAR_HEIGHT;
    let clock_col = (w - 80) / 8;
    let clock_row = tb_y / 16 + 1;

    console.set_fg(TEXT_PRIMARY);
    console.set_bg(BG_TASKBAR);
    console.set_cursor(clock_col, clock_row);

    // Format: HH:MM
    let h10 = (hours / 10) as u8 + b'0';
    let h1 = (hours % 10) as u8 + b'0';
    let m10 = (minutes / 10) as u8 + b'0';
    let m1 = (minutes % 10) as u8 + b'0';
    console.write_char(fb, h10 as char);
    console.write_char(fb, h1 as char);
    console.write_char(fb, ':');
    console.write_char(fb, m10 as char);
    console.write_char(fb, m1 as char);

    // Date below (if space)
    console.set_cursor(clock_col, clock_row + 1);
    console.set_fg(TEXT_DIM);
    let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
    if (month as usize) >= 1 && (month as usize) <= 12 {
        console.write_str(fb, months[(month - 1) as usize]);
    }
    console.write_char(fb, ' ');
    let d10 = (day / 10) as u8 + b'0';
    let d1 = (day % 10) as u8 + b'0';
    if d10 != b'0' { console.write_char(fb, d10 as char); }
    console.write_char(fb, d1 as char);

    // Reset
    console.set_fg(ACCENT_CYAN);
    console.set_bg(BG_DEEP);
}

// ── Desktop Interaction Loop ───────────────────────────────────

/// Enters the interactive GUI event loop.
///
/// Enables interrupts and begins processing PS/2 mouse and keyboard
/// events. Renders a hardware cursor natively and echoes typed keys
/// via the framebuffer console.
pub fn run_desktop_loop(fb: &mut AetherFrameBuffer, console: &mut FramebufferConsole) -> ! {
    // Enable hardware interrupts so IRQ1 and IRQ12 fire!
    unsafe { core::arch::asm!("sti") };

    use aether::window::WindowManager;
    use aether::input::{InputRouter, InputEvent, MouseBtn, Modifiers, HotkeyAction, InputResult};

    let mut wm = WindowManager::new(fb.width() as f32, fb.height() as f32);
    let mut router = InputRouter::new();

    wm.create_window(5, alloc::string::String::from("Q-Shell - admin@qindows: ~"), 5.0, 74.0, 622.0, 630.0);
    wm.create_window(0, alloc::string::String::from("System Monitor"), 636.0, 74.0, 380.0, 340.0);

    let (mut cursor_x, mut cursor_y) = crate::drivers::mouse::get_position();
    let mut dragging_win: Option<(u64, f32, f32)> = None;

    const CUR_W: i32 = 12;
    const CUR_H: i32 = 18;
    let mut saved_bg = [0u32; (CUR_W * CUR_H) as usize];

    let mut qshell_lines = alloc::vec::Vec::new();
    qshell_lines.push(alloc::string::String::from("Genesis Protocol Initiated..."));
    qshell_lines.push(alloc::string::String::from("[OK] Aether Display Active"));
    qshell_lines.push(alloc::string::String::from("admin@qindows:~$ "));

    // Initial render
    render_scene(fb, &wm, console, &qshell_lines, cursor_x as f32, cursor_y as f32);

    // (Test rectangle removed)

    save_bg(fb, cursor_x, cursor_y, &mut saved_bg);
    draw_cursor(fb, cursor_x, cursor_y);

    crate::serial_println!("[GUI] Initial render complete. Entering main loop.");

    loop {
        let mut needs_redraw = false;
        let mut drew_cursor = false;

        // Process Mouse Events
        while let Some(mev) = crate::drivers::mouse::poll_event() {
            if !drew_cursor {
                restore_bg(fb, cursor_x, cursor_y, &saved_bg);
            }
            let (nx, ny) = crate::drivers::mouse::get_position();
            cursor_x = nx;
            cursor_y = ny;

            let bx = cursor_x as f32;
            let by = cursor_y as f32;

            // Route standard movement and clicks to Aether InputRouter
            if mev.buttons.left {
                router.route(&InputEvent::MouseButton { button: MouseBtn::Left, pressed: true, x: bx, y: by });
            } else if mev.buttons.right {
                router.route(&InputEvent::MouseButton { button: MouseBtn::Right, pressed: true, x: bx, y: by });
            } else {
                router.route(&InputEvent::MouseMove { x: bx, y: by });
            }

            if mev.buttons.left {
                if let Some((id, off_x, off_y)) = dragging_win {
                    if let Some(w) = wm.windows.iter_mut().find(|w| w.id == id) {
                        w.x = bx - off_x;
                        w.y = by - off_y;
                        needs_redraw = true;
                    }
                } else if let Some(id) = wm.window_at_point(bx, by) {
                    wm.focus_window(id);
                    if let Some(w) = wm.windows.iter().find(|w| w.id == id) {
                        // Only allow drag if clicking the header (top 40px)
                        if by < w.y + 40.0 {
                            dragging_win = Some((id, bx - w.x, by - w.y));
                        }
                    }
                    needs_redraw = true;
                }
            } else {
                if dragging_win.is_some() {
                    router.route(&InputEvent::MouseButton { button: MouseBtn::Left, pressed: false, x: bx, y: by });
                }
                dragging_win = None;
            }
            drew_cursor = true;
        }

        // Process Keyboard Events
        while let Some(kev) = crate::drivers::keyboard::poll_key() {
            let mods = Modifiers {
                ctrl: kev.modifiers.ctrl(),
                alt: kev.modifiers.alt(),
                shift: kev.modifiers.shift(),
                meta: kev.modifiers.meta,
            };
            let ev = InputEvent::Key { scancode: kev.scancode as u16, pressed: kev.pressed, modifiers: mods };
            let result = router.route(&ev);
            
            if let InputResult::System(action) = result {
                if let HotkeyAction::SwitchWindow = action {
                    needs_redraw = true;
                }
            }

            if kev.pressed {
                if kev.keycode == crate::drivers::keyboard::KeyCode::Enter {
                    if let Some(cmd_line) = qshell_lines.last() {
                        let cmd = cmd_line.trim_start_matches("admin@qindows:~$ ");
                        if !cmd.trim().is_empty() {
                            let output = crate::syscall::qshell_dispatch(cmd);
                            if output == "\x0C" {
                                // Clear command — reset to empty shell
                                qshell_lines.clear();
                                qshell_lines.push(alloc::string::String::from("Genesis Protocol Initiated..."));
                                qshell_lines.push(alloc::string::String::from("[OK] Aether Display Active"));
                            } else {
                                for line in output.lines() {
                                    qshell_lines.push(alloc::string::String::from(line));
                                }
                            }
                        }
                    }
                    qshell_lines.push(alloc::string::String::from("admin@qindows:~$ "));
                    while qshell_lines.len() > 18 {
                        qshell_lines.remove(0);
                    }
                    needs_redraw = true;
                } else if kev.keycode == crate::drivers::keyboard::KeyCode::Backspace {
                    if let Some(last) = qshell_lines.last_mut() {
                        const PROMPT: &str = "admin@qindows:~$ ";
                        if last.len() > PROMPT.len() { // Protect prompt exactly
                            last.pop();
                            needs_redraw = true;
                        }
                    }
                } else if let Some(c) = crate::drivers::keyboard::keycode_to_char(kev.keycode, kev.modifiers.shift()) {
                    if let Some(last) = qshell_lines.last_mut() {
                        last.push(c);
                        needs_redraw = true;
                    }
                }
            }
        }

        // Apply visual updates asynchronously
        if needs_redraw {
            render_scene(fb, &wm, console, &qshell_lines, cursor_x as f32, cursor_y as f32);
            save_bg(fb, cursor_x, cursor_y, &mut saved_bg);
            draw_cursor(fb, cursor_x, cursor_y);
            // Background newly saved
        } else if drew_cursor {
            save_bg(fb, cursor_x, cursor_y, &mut saved_bg);
            draw_cursor(fb, cursor_x, cursor_y);
        }

        // Fiber scheduler yield
        unsafe { core::arch::asm!("hlt") };
    }
}

/// Re-compose the Aether desktop using the real SDF renderer.
///
/// Phase 1: Builds an Aether `RenderFrame` (wallpaper gradient, Q-Glass windows
///          with drop-shadows and focus glow, GlassBlur taskbar) and calls
///          `rasterize_aether_frame()` — the actual Aether SDF software rasterizer.
/// Phase 2: Overlays 2× bitmap-font text for the shell, monitor, and log content.
fn render_scene(fb: &mut AetherFrameBuffer, wm: &aether::window::WindowManager, _console: &mut FramebufferConsole, qshell_lines: &[alloc::string::String], mx: f32, my: f32) {
    // (RenderFrame, RenderCommand, Color are imported at file top — aether::renderer)
    let w = fb.width();
    let h = fb.height();
    let tb_y = h - TASKBAR_HEIGHT;

    // Derive window positions from the WindowManager so dragging persists.
    let screen_w = w as f32;
    let screen_h = (h - TASKBAR_HEIGHT) as f32;

    let find_win = |keyword: &str| -> (usize,usize,usize,usize) {
        wm.windows.iter()
            .find(|ww| ww.title.contains(keyword))
            .map(|ww| (
                ww.x.max(0.0).min(screen_w - 40.0) as usize,
                ww.y.max(0.0).min(screen_h - 40.0) as usize,
                ww.width.max(100.0) as usize,
                ww.height.max(60.0) as usize,
            ))
            .unwrap_or((5, 74, 622, 630))
    };

    let (w1_x, w1_y, w1_w, w1_h) = find_win("Q-Shell");
    let (w2_x, w2_y, w2_w, w2_h) = find_win("System Monitor");
    let w3_x = w2_x; let w3_y = w2_y + w2_h + 4;
    let w3_w = w2_w; let w3_h = (screen_h as usize).saturating_sub(w3_y + 4).min(280);

    let shell_focused  = wm.windows.iter().any(|ww| ww.focused && ww.title.contains("Q-Shell"));
    let mon_focused    = wm.windows.iter().any(|ww| ww.focused && ww.title.contains("System Monitor"));

    // ── PHASE 1: AETHER SDF RENDER FRAME ─────────────────────────────
    // Build the RenderFrame scene graph, then rasterize_aether_frame()
    // sends every pixel through the SDF math and alpha-blends it into fb.
    let mut frame = RenderFrame::new(w as f32, h as f32, 1.0);

    // 1a. Wallpaper — deep midnight gradient
    frame.push(RenderCommand::Gradient {
        x: 0.0, y: 0.0, width: w as f32, height: h as f32,
        start_color: Color::rgba(0.04, 0.05, 0.11, 1.0),
        end_color:   Color::rgba(0.02, 0.02, 0.07, 1.0),
        angle: 180.0,
    });

    // 1b. Glowing orbs — Aether SDF Circles at visible opacity for visual depth
    // Cyan orb — left third
    frame.push(RenderCommand::Circle {
        cx: (w / 4) as f32, cy: (h / 3) as f32,
        radius: 240.0,
        fill: Color::rgba(0.0, 0.70, 0.95, 0.14),
    });
    // Blue orb — right two-thirds
    frame.push(RenderCommand::Circle {
        cx: (w * 3 / 4) as f32, cy: (h * 2 / 3) as f32,
        radius: 300.0,
        fill: Color::rgba(0.06, 0.28, 0.95, 0.12),
    });
    // Gold accent orb — top right
    frame.push(RenderCommand::Circle {
        cx: (w as f32 - 180.0), cy: 100.0,
        radius: 140.0,
        fill: Color::rgba(1.0, 0.84, 0.0, 0.09),
    });
    // Bottom vignette gradient for depth toward taskbar
    frame.push(RenderCommand::Gradient {
        x: 0.0, y: (h as f32 * 0.7),
        width: w as f32, height: (h as f32 * 0.3),
        start_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
        end_color: Color::rgba(0.0, 0.0, 0.0, 0.25),
        angle: 180.0,
    });

    // 1c. Q-Shell window — Q-Glass + drop-shadow + focus glow
    frame.draw_window(
        w1_x as f32, w1_y as f32,
        w1_w as f32, w1_h as f32,
        40.0, // title bar height
        shell_focused,
    );

    // 1d. System Monitor window
    frame.draw_window(
        w2_x as f32, w2_y as f32,
        w2_w as f32, w2_h as f32,
        40.0,
        mon_focused,
    );

    // 1e. Compositor Log window
    if w3_h > 60 {
        frame.draw_window(
            w3_x as f32, w3_y as f32,
            w3_w as f32, w3_h as f32,
            40.0,
            false, // never focused
        );
    }

    // 1f. Taskbar — Q-Glass strip
    frame.push(RenderCommand::GlassBlur {
        x: 0.0, y: tb_y as f32,
        width: w as f32, height: TASKBAR_HEIGHT as f32,
        radius: 0.0, blur_radius: 20.0,
        tint: Color::rgba(0.05, 0.06, 0.10, 0.92),
    });
    // Taskbar top separator line
    frame.push(RenderCommand::RoundedRect {
        x: 0.0, y: tb_y as f32,
        width: w as f32, height: 1.0,
        radius: 0.0,
        fill: Color::rgba(1.0, 1.0, 1.0, 0.08),
        border: None,
    });
    // Q Start button (cyan pill)
    let hover_q = mx >= 8.0 && mx <= 48.0 && my >= (tb_y + 4) as f32 && my <= (tb_y + 36) as f32;
    frame.push(RenderCommand::RoundedRect {
        x: 8.0, y: (tb_y + 4) as f32,
        width: 36.0, height: 32.0,
        radius: 8.0,
        fill: if hover_q {
            Color::rgba(0.063, 0.9, 0.69, 1.0)  // brighter on hover
        } else {
            Color::rgba(0.0, 0.94, 1.0, 0.9)    // ACCENT_CYAN
        },
        border: None,
    });
    // App taskbar pills
    let app_pills: &[(&str, Color)] = &[
        ("Q-Shell",    Color::rgba(0.06, 0.44, 0.78, 0.8)),
        ("Monitor",    Color::rgba(0.06, 0.24, 0.90, 0.7)),
        ("Compositor", Color::rgba(1.0, 0.84, 0.0, 0.6)),
    ];
    let mut bpx = 56.0f32;
    for (name, col) in app_pills.iter() {
        let pw = (name.len() * 8 + 20) as f32;
        frame.push(RenderCommand::RoundedRect {
            x: bpx, y: (tb_y + 5) as f32,
            width: pw, height: 30.0,
            radius: 6.0,
            fill: *col,
            border: None,
        });
        bpx += pw + 6.0;
    }

    // Window titles — rendered via Aether Text in Phase 1 (alpha-blended through rasterize_aether_frame)
    let title_color = Color::from_hex(TEXT_PRIMARY);
    let dim_color   = Color::from_hex(TEXT_DIM);
    frame.push(RenderCommand::Text {
        x: (w1_x + 16) as f32, y: (w1_y + 13) as f32,
        text: alloc::string::String::from("Q-Shell  [admin@qindows ~]"),
        size: 14.0, color: title_color,
    });
    frame.push(RenderCommand::Text {
        x: (w2_x + 16) as f32, y: (w2_y + 13) as f32,
        text: alloc::string::String::from("System Monitor"),
        size: 14.0, color: dim_color,
    });
    if w3_h > 60 {
        frame.push(RenderCommand::Text {
            x: (w3_x + 16) as f32, y: (w3_y + 13) as f32,
            text: alloc::string::String::from("Compositor Log"),
            size: 14.0, color: dim_color,
        });
    }

    // ── Rasterize the Aether frame ─────────────────────────────────────────
    rasterize_aether_frame(fb, &frame);

    // ── PHASE 2: 2× BITMAP TEXT OVERLAYS ─────────────────────────────
    // (These are drawn directly on top of the rasterized Aether chrome)

    // (Window titles are in Phase 1 — no duplication here)

    // Q-Shell content at 2x
    let sh_y0 = w1_y + 50;
    let line_h = 34usize;
    let max_lines = (w1_h - 60) / line_h;
    let start = qshell_lines.len().saturating_sub(max_lines);
    for (i, line) in qshell_lines[start..].iter().enumerate() {
        let ty = sh_y0 + i * line_h;
        if ty + 32 < w1_y + w1_h {
            let c = if line.starts_with('[') { STATUS_GREEN }
                    else if line.starts_with("admin@") { ACCENT_CYAN }
                    else { TEXT_PRIMARY };
            draw_text_scaled(fb, w1_x + 14, ty, line, c, 2);
        }
    }
    if shell_focused {
        if let Some(last) = qshell_lines.last() {
            let row = (qshell_lines.len() - 1 - start).min(max_lines - 1);
            let cx = w1_x + 14 + last.len() * 16;
            let cy = sh_y0 + row * line_h;
            if cx + 16 < w1_x + w1_w && cy + 26 < w1_y + w1_h {
                fill_rect_alpha(fb, cx, cy, 12, 26, ACCENT_CYAN, 220);
            }
        }
    }

    // System Monitor content
    let sm_x = w2_x + 14; let sm_y = w2_y + 50;
    draw_text_scaled(fb, sm_x, sm_y, "  MEMORY", ACCENT_CYAN, 2);
    draw_separator(fb, w2_x + 8, sm_y + 36, w2_w.saturating_sub(16));
    let free_mb = crate::memory::page_alloc::free_bytes() / (1024 * 1024);
    let total_mb = crate::memory::page_alloc::total_count() * 4096 / (1024 * 1024);
    draw_kv_2x(fb, sm_x, sm_y + 44,  "Used: ", total_mb.saturating_sub(free_mb), "MB", STATUS_GREEN);
    draw_kv_2x(fb, sm_x, sm_y + 84,  "Free: ", free_mb, "MB", TEXT_PRIMARY);
    draw_kv_2x(fb, sm_x, sm_y + 124, "Total:", total_mb, "MB", TEXT_DIM);
    draw_separator(fb, w2_x + 8, sm_y + 165, w2_w.saturating_sub(16));
    draw_text_scaled(fb, sm_x, sm_y + 174, "  SILOS", ACCENT_CYAN, 2);
    let silo_count = crate::kstate::silos().silos.len() as u64;
    draw_kv_2x(fb, sm_x, sm_y + 212, "Active:", silo_count, "", STATUS_GREEN);
    let uptime_ms = crate::kstate::global_tick();
    draw_kv_2x(fb, sm_x, sm_y + 252, "Uptime:", uptime_ms, "ms", TEXT_DIM);

    // Compositor Log content
    let lg_x = w3_x + 14; let lg_y = w3_y + 50;
    let tick = crate::kstate::global_tick();
    let logs: &[(&str, u32)] = &[
        ("[ok] BootInfo@0x5FF000", STATUS_GREEN),
        ("[ok] FB 1024x768 MMIO",  STATUS_GREEN),
        ("[ok] PCI bochs-display", STATUS_GREEN),
        ("[ok] Desktop loop live", STATUS_GREEN),
        ("[>>] Compositor active", ACCENT_CYAN),
    ];
    for (i, (line, c)) in logs.iter().enumerate() {
        let ty = lg_y + i * 34;
        if w3_h > 60 && ty + 32 < w3_y + w3_h { draw_text_scaled(fb, lg_x, ty, line, *c, 2); }
    }
    if w3_h > 220 {
        let mut tick_str = alloc::string::String::from("[>>] tick=");
        let mut v = tick; let mut d = [b'0'; 10]; let mut di = 10;
        if v == 0 { di -= 1; d[di] = b'0'; } else { while v > 0 { di -= 1; d[di] = (v % 10) as u8 + b'0'; v /= 10; } }
        for b in &d[di..] { tick_str.push(*b as char); }
        let tick_ty = lg_y + logs.len() * 34;
        if tick_ty + 32 < w3_y + w3_h { draw_text_scaled(fb, lg_x, tick_ty, &tick_str, ACCENT_GOLD, 2); }
    }

    // Taskbar text overlays (Q letter + app names + clock)
    draw_q_letter(fb, 18, tb_y + 10, BG_DEEP);
    let apps: &[(&str, u32)] = &[
        ("Q-Shell", ACCENT_CYAN), ("Monitor", ACCENT_BLUE), ("Compositor", ACCENT_GOLD),
    ];
    let mut bx = 56usize;
    for (name, col) in apps.iter() {
        let pw = name.len() * 8 + 22;
        draw_text_exact(fb, bx + 11, tb_y + 13, name, *col);
        bx += pw + 6;
    }
    // Clock (RTC)
    let mut rtc = crate::rtc::Rtc::new();
    let time = rtc.read_time();
    let h12 = if time.hour == 0 { 12u8 } else if time.hour > 12 { time.hour - 12 } else { time.hour };
    let mut tstr = alloc::string::String::new();
    tstr.push((h12 / 10 + b'0') as char); tstr.push((h12 % 10 + b'0') as char); tstr.push(':');
    tstr.push((time.minute / 10 + b'0') as char); tstr.push((time.minute % 10 + b'0') as char);
    if time.hour >= 12 { tstr.push_str(" PM"); } else { tstr.push_str(" AM"); }
    draw_text_exact(fb, w.saturating_sub(76), tb_y + 7, &tstr, TEXT_PRIMARY);
    let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
    if (time.month as usize) >= 1 && (time.month as usize) <= 12 {
        let mut dstr = alloc::string::String::from(months[(time.month - 1) as usize]);
        dstr.push(' ');
        if time.day >= 10 { dstr.push((time.day / 10 + b'0') as char); }
        dstr.push((time.day % 10 + b'0') as char);
        draw_text_exact(fb, w.saturating_sub(64), tb_y + 23, &dstr, TEXT_DIM);
    }
}

/// Horizontal separator
fn draw_separator(fb: &mut AetherFrameBuffer, x: usize, y: usize, w: usize) {
    for px in x..x.saturating_add(w).min(fb.width()) { fb.draw_pixel(px, y, 0xFF_2A_2D_39); }
}

/// Draw key: value metric line at 2x scale
fn draw_kv_2x(fb: &mut AetherFrameBuffer, x: usize, y: usize, label: &str, val: u64, unit: &str, vc: u32) {
    draw_text_scaled(fb, x, y, label, TEXT_DIM, 2);
    let mut lx = x + label.len() * 16; // 2x = 16px per char
    let mut digits = [b'0'; 12];
    let mut d = val; let mut di = 12usize;
    if d == 0 { di -= 1; digits[di] = b'0'; }
    else { while d > 0 { di -= 1; digits[di] = (d % 10) as u8 + b'0'; d /= 10; } }
    for &byte in &digits[di..] {
        let s = [byte];
        draw_text_scaled(fb, lx, y, core::str::from_utf8(&s).unwrap_or("?"), vc, 2);
        lx += 16;
    }
    draw_text_scaled(fb, lx + 4, y, unit, TEXT_DIM, 2);
}

/// Draw key: value metric line
fn draw_kv(fb: &mut AetherFrameBuffer, x: usize, y: usize, label: &str, val: u64, unit: &str, vc: u32) {
    draw_text_exact(fb, x, y, label, TEXT_DIM);
    let mut lx = x + label.len() * 8;
    let mut digits = [b'0'; 12];
    let mut d = val; let mut di = 12usize;
    if d == 0 { di -= 1; digits[di] = b'0'; }
    else { while d > 0 { di -= 1; digits[di] = (d % 10) as u8 + b'0'; d /= 10; } }
    for &byte in &digits[di..] {
        let s = [byte];
        draw_text_exact(fb, lx, y, core::str::from_utf8(&s).unwrap_or("?"), vc);
        lx += 8;
    }
    draw_text_exact(fb, lx, y, unit, TEXT_DIM);
}

/// Window panel: shadow + dark glass body + gradient header + traffic lights + focus ring
fn draw_window_pane(fb: &mut AetherFrameBuffer, x: usize, y: usize, w: usize, h: usize, focused: bool, mx: f32, my: f32) {
    draw_drop_shadow(fb, x, y, w, h, 10, if focused { 20 } else { 10 }, if focused { 150 } else { 70 });
    fill_rect_alpha(fb, x, y, w, h, 0xFF_0B_0E_16, 255);
    let bc = if focused { 0xFF_1E_6F_5C } else { 0xFF_1A_1D_28 };
    draw_rounded_rect_alpha(fb, x, y, w, h, 10, bc, if focused { 200 } else { 120 });
    draw_gradient_rect_alpha(fb, x + 1, y + 1, w - 2, 40, 10, 0xFF_1C_20_30, 0xFF_12_15_1F, 255);
    fill_rect_alpha(fb, x + 1, y + 40, w - 2, 1, 0xFF_00_00_00, 180);
    let hy = y + 15;
    let is_hov = |bx: usize| mx >= bx as f32 && mx < (bx + 14) as f32 && my >= hy as f32 && my < (hy + 14) as f32;
    draw_rounded_rect_alpha(fb, x + 14, hy, 13, 13, 6, if is_hov(x+14) { 0xFF_FF_7F_76 } else { 0xFF_FF_5F_56 }, 255);
    draw_rounded_rect_alpha(fb, x + 35, hy, 13, 13, 6, if is_hov(x+35) { 0xFF_FF_CD_4E } else { 0xFF_FF_BD_2E }, 255);
    draw_rounded_rect_alpha(fb, x + 56, hy, 13, 13, 6, if is_hov(x+56) { 0xFF_47_E9_5F } else { 0xFF_27_C9_3F }, 255);
    if focused {
        for dy in 10..h.saturating_sub(10) {
            let t = libm::sinf((dy as f32 / h as f32) * core::f32::consts::PI);
            let a = (t * 60.0) as u32;
            let bg = fb.read_pixel(x, y + dy);
            fb.draw_pixel(x, y + dy, blend(bg, ACCENT_CYAN, a));
        }
    }
}

/// Render text at an integer scale multiplier (scale=2 gives 16x32 chars)
fn draw_text_scaled(fb: &mut AetherFrameBuffer, x: usize, y: usize, text: &str, color: u32, scale: usize) {
    if scale <= 1 { draw_text_exact(fb, x, y, text, color); return; }
    let font = crate::drivers::console::FONT_8X16;
    let mut cx = x;
    for ch in text.chars() {
        let ascii = ch as u8;
        if ascii >= 0x20 && ascii <= 0x7E {
            let go = ((ascii - 0x20) as usize) * 16;
            if go + 16 <= font.len() {
                for dy in 0..16usize {
                    let rb = font[go + dy];
                    for dx in 0..8usize {
                        if rb & (0x80 >> dx) != 0 {
                            for sy in 0..scale { for sx in 0..scale {
                                fb.draw_pixel(cx + dx * scale + sx, y + dy * scale + sy, color);
                            }}
                        }
                    }
                }
            }
        }
        cx += 8 * scale;
    }
}

fn save_bg(fb: &AetherFrameBuffer, cx: i32, cy: i32, buf: &mut [u32]) {
    let mut i = 0;
    for y in 0..18 {
        for x in 0..12 {
            buf[i] = fb.read_pixel((cx + x) as usize, (cy + y) as usize);
            i += 1;
        }
    }
}

fn restore_bg(fb: &mut AetherFrameBuffer, cx: i32, cy: i32, buf: &[u32]) {
    let mut i = 0;
    for y in 0..18 {
        for x in 0..12 {
            fb.draw_pixel((cx + x) as usize, (cy + y) as usize, buf[i]);
            i += 1;
        }
    }
}

/// Draw a minimal arrow cursor (white with black outline)
fn draw_cursor(fb: &mut AetherFrameBuffer, x: i32, y: i32) {
    let ux = x as usize;
    let uy = y as usize;
    let main_color = 0xFF_FF_FF_FF; // White
    let outline = 0xFF_00_00_00;    // Black

    // Simple pixel-art cursor shape
    #[rustfmt::skip]
    let shape = [
        "12          ",
        "112         ",
        "1112        ",
        "11112       ",
        "111112      ",
        "1111112     ",
        "11111112    ",
        "111111112   ",
        "1111111112  ",
        "11111111112 ",
        "111111222222",
        "1112112     ",
        "112 112     ",
        "12  2112    ",
        "2   2112    ",
        "     2112   ",
        "     2112   ",
        "      22    ",
    ];

    for (dy, row) in shape.iter().enumerate() {
        for (dx, pixel) in row.chars().enumerate() {
            if pixel == '1' {
                fb.draw_pixel(ux + dx, uy + dy, main_color);
            } else if pixel == '2' {
                fb.draw_pixel(ux + dx, uy + dy, outline);
            }
        }
    }
}

/// Convert r,g,b to ARGB u32
#[inline]
fn rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Draw a small "Q" letter (8x10 pixels) for the taskbar button
fn draw_q_letter(fb: &mut AetherFrameBuffer, x: usize, y: usize, color: u32) {
    // Simplified Q glyph as pixel pattern
    let pattern: [u16; 10] = [
        0b0011_1100,
        0b0110_0110,
        0b1100_0011,
        0b1100_0011,
        0b1100_0011,
        0b1100_0011,
        0b1100_1011,
        0b0110_0110,
        0b0011_1100,
        0b0000_0011,
    ];
    for (dy, row_bits) in pattern.iter().enumerate() {
        for dx in 0..8 {
            if row_bits & (0x80 >> dx) != 0 {
                fb.draw_pixel(x + dx, y + dy, color);
                fb.draw_pixel(x + dx + 1, y + dy, color); // 2x width for visibility
            }
        }
    }
}

/// Draw a massive faint watermark Q in the center of the screen
fn draw_large_q_watermark(fb: &mut AetherFrameBuffer, cx: usize, cy: usize) {
    let color = 0xFF_FF_FF_FF;
    let alpha = 6; // incredibly faint
    
    // Draw thick lines forming a Q pattern
    let r = 150;
    for t in 0..360 {
        let rad = (t as f32) * 0.0174533;
        let x = cx as f32 + (r as f32) * libm::cosf(rad);
        let y = cy as f32 + (r as f32) * libm::sinf(rad);
        fill_rect_alpha(fb, x as usize, y as usize, 12, 12, color, alpha);
    }
    // Tail
    for i in 0..80 {
        fill_rect_alpha(fb, cx + 80 + i, cy + 80 + i, 12, 12, color, alpha);
    }
}

// ── Alpha Blending & Rich Graphics Utilities ───────────────────

/// Blend a foreground color onto a background color using an alpha value (0-255).
#[inline(always)]
fn blend(bg: u32, fg: u32, a: u32) -> u32 {
    if a == 0 { return bg; }
    if a == 255 { return fg; }

    let inv_a = 255 - a;

    let br = (bg >> 16) & 0xFF;
    let bg_g = (bg >> 8) & 0xFF;
    let bb = bg & 0xFF;

    let fr = (fg >> 16) & 0xFF;
    let fg_g = (fg >> 8) & 0xFF;
    let fb_b = fg & 0xFF;

    let out_r = ((fr * a) + (br * inv_a)) / 255;
    let out_g = ((fg_g * a) + (bg_g * inv_a)) / 255;
    let out_b = ((fb_b * a) + (bb * inv_a)) / 255;

    (out_r << 16) | (out_g << 8) | out_b
}

/// Draw a heavily blended radial glowing orb
fn draw_orb(fb: &mut AetherFrameBuffer, cx: usize, cy: usize, radius: usize, color: u32, max_alpha: u32) {
    let r2 = (radius * radius) as i64;
    let cw = cx as i64;
    let cy_i = cy as i64;

    for y in cy.saturating_sub(radius)..cy.saturating_add(radius) {
        if y >= fb.height() { break; }
        let dy = (y as i64) - cy_i;
        let dy2 = dy * dy;

        for x in cx.saturating_sub(radius)..cx.saturating_add(radius) {
            if x >= fb.width() { break; }
            let dx = (x as i64) - cw;
            let dist2 = dx * dx + dy2;

            if dist2 < r2 {
                // Calculate fading alpha based on distance
                let dist = libm::sqrtf(dist2 as f32);
                let intensity = 1.0 - (dist / (radius as f32));
                // Cubic fade for smoother glow
                let alpha = (max_alpha as f32 * intensity * intensity * intensity) as u32;

                if alpha > 0 {
                    let bg = fb.read_pixel(x, y);
                    let final_color = blend(bg, color, alpha);
                    fb.draw_pixel(x, y, final_color);
                }
            }
        }
    }
}

/// Fill a rectangle with alpha blending
fn fill_rect_alpha(fb: &mut AetherFrameBuffer, x: usize, y: usize, w: usize, h: usize, color: u32, alpha: u32) {
    if alpha == 255 {
        fb.fill_rect(x, y, w, h, color);
        return;
    }
    for py in y..y.saturating_add(h).min(fb.height()) {
        for px in x..x.saturating_add(w).min(fb.width()) {
            let bg = fb.read_pixel(px, py);
            fb.draw_pixel(px, py, blend(bg, color, alpha));
        }
    }
}

/// Draw a rounded rectangle with alpha blending
fn draw_rounded_rect_alpha(fb: &mut AetherFrameBuffer, x: usize, y: usize, w: usize, h: usize, r: usize, color: u32, alpha: u32) {
    let r2 = (r * r) as i64;
    for py in y..y.saturating_add(h).min(fb.height()) {
        for px in x..x.saturating_add(w).min(fb.width()) {
            // Check corners
            let mut draw = true;
            let cx = if px < x + r { x + r } else if px >= x + w - r { x + w - r - 1 } else { px };
            let cy = if py < y + r { y + r } else if py >= y + h - r { y + h - r - 1 } else { py };

            if px < x + r || px >= x + w - r {
                if py < y + r || py >= y + h - r {
                    let dx = (px as i64) - (cx as i64);
                    let dy = (py as i64) - (cy as i64);
                    if dx * dx + dy * dy > r2 {
                        draw = false;
                    }
                }
            }

            if draw {
                if alpha == 255 {
                    fb.draw_pixel(px, py, color);
                } else {
                    let bg = fb.read_pixel(px, py);
                    fb.draw_pixel(px, py, blend(bg, color, alpha));
                }
            }
        }
    }
}

/// Draw a realistic window with robust SDF algorithms for shadows and gradients
fn draw_window(fb: &mut AetherFrameBuffer, x: usize, y: usize, w: usize, h: usize, _title: &str, focused: bool, mx: f32, my: f32) {
    let shadow_spread = if focused { 24 } else { 12 };
    let shadow_opac = if focused { 180 } else { 100 };
    draw_drop_shadow(fb, x, y, w, h, 12, shadow_spread, shadow_opac);

    // Main window body (Glassy Dark)
    fill_rect_alpha(fb, x, y, w, h, 0xFF_0B_0E_14, 250);
    // Top border light
    draw_rounded_rect_alpha(fb, x, y, w, 1, 0, 0xFF_2A_2D_39, 180);
    // Subtle gradient outline
    draw_rounded_rect_alpha(fb, x, y, w, h, 12, 0xFF_1F_22_2D, if focused { 180 } else { 150 });
    draw_rounded_rect_alpha(fb, x+1, y+1, w-2, h-2, 11, 0xFF_0D_0E_15, 255); // Reset inside

    // Header bar (Gradient)
    draw_gradient_rect_alpha(fb, x+1, y+1, w-2, 40, 11, 0xFF_1F_22_32, 0xFF_18_1A_24, 255);
    fill_rect_alpha(fb, x+1, y+20, w-2, 21, 0xFF_18_1A_24, 255); // Straighten bottom
    fill_rect_alpha(fb, x, y + 40, w, 1, 0xFF_00_00_00, 180);

    // Window controls with Hover states
    let cx = x + 16; let cy = y + 14;
    let hover_close = mx >= cx as f32 && mx <= (cx+12) as f32 && my >= cy as f32 && my <= (cy+12) as f32;
    draw_rounded_rect_alpha(fb, cx, cy, 12, 12, 6, if hover_close { 0x00_FF_7F_76 } else { 0x00_FF_5F_56 }, 255); 
    let hover_min = mx >= (x+36) as f32 && mx <= (x+48) as f32 && my >= cy as f32 && my <= (cy+12) as f32;
    draw_rounded_rect_alpha(fb, x + 36, y + 14, 12, 12, 6, if hover_min { 0x00_FF_CD_4E } else { 0x00_FF_BD_2E }, 255); 
    let hover_max = mx >= (x+56) as f32 && mx <= (x+68) as f32 && my >= cy as f32 && my <= (cy+12) as f32;
    draw_rounded_rect_alpha(fb, x + 56, y + 14, 12, 12, 6, if hover_max { 0x00_47_E9_5F } else { 0x00_27_C9_3F }, 255); 
}

fn draw_drop_shadow(fb: &mut AetherFrameBuffer, cx: usize, cy: usize, w: usize, h: usize, r: usize, spread: usize, max_alpha: u32) {
    let shadow_color = 0x00_00_00_00;
    let left = cx.saturating_sub(spread);
    let top = cy.saturating_sub(spread);
    let right = (cx + w + spread).min(fb.width());
    let bottom = (cy + h + spread).min(fb.height());
    
    let inner_l = cx + r;
    let inner_r = cx + w - r;
    let inner_t = cy + r;
    let inner_b = cy + h - r;

    for py in top..bottom {
        for px in left..right {
            let cp_x = px.max(inner_l).min(inner_r - 1);
            let cp_y = py.max(inner_t).min(inner_b - 1);
            let dx = (px as i64) - (cp_x as i64);
            let dy = (py as i64) - (cp_y as i64);
            let dist = libm::sqrtf((dx * dx + dy * dy) as f32);
            let dist_from_edge = dist - r as f32;
            
            if dist_from_edge > 0.0 && dist_from_edge < spread as f32 {
                let intensity = 1.0 - (dist_from_edge / spread as f32);
                let alpha = ((max_alpha as f32) * intensity * intensity) as u32;
                if alpha > 0 {
                    let bg_col = fb.read_pixel(px, py);
                    fb.draw_pixel(px, py, blend(bg_col, shadow_color, alpha));
                }
            }
        }
    }
}

fn draw_gradient_rect_alpha(fb: &mut AetherFrameBuffer, x: usize, y: usize, w: usize, h: usize, r: usize, c_top: u32, c_bot: u32, alpha: u32) {
    let r2 = (r * r) as i64;
    let tr = (c_top >> 16) & 0xFF; let tg = (c_top >> 8) & 0xFF; let tb = c_top & 0xFF;
    let br = (c_bot >> 16) & 0xFF; let bg = (c_bot >> 8) & 0xFF; let bb = c_bot & 0xFF;
    
    for py in y..y.saturating_add(h).min(fb.height()) {
        let t = (py - y) as f32 / h as f32;
        let pr = ((tr as f32) * (1.0 - t) + (br as f32) * t) as u32;
        let pg = ((tg as f32) * (1.0 - t) + (bg as f32) * t) as u32;
        let pb = ((tb as f32) * (1.0 - t) + (bb as f32) * t) as u32;
        let p_col = (pr << 16) | (pg << 8) | pb;
        
        for px in x..x.saturating_add(w).min(fb.width()) {
            let mut draw = true;
            let cx_c = if px < x + r { x + r } else if px >= x + w - r { x + w - r - 1 } else { px };
            let cy_c = if py < y + r { y + r } else if py >= y + h - r { y + h - r - 1 } else { py };
            if px < x + r || px >= x + w - r {
                if py < y + r || py >= y + h - r {
                    let dx = (px as i64) - (cx_c as i64);
                    let dy = (py as i64) - (cy_c as i64);
                    if dx * dx + dy * dy > r2 { draw = false; }
                }
            }
            if draw {
                let bg_col = fb.read_pixel(px, py);
                fb.draw_pixel(px, py, blend(bg_col, p_col, alpha));
            }
        }
    }
}

/// Render perfect native text directly from the 8x16 font bitmap
fn draw_text_exact(fb: &mut AetherFrameBuffer, x: usize, y: usize, text: &str, color: u32) {
    let mut cx = x;
    for ch in text.chars() {
        let ascii = ch as u8;
        if ascii >= 0x20 && ascii <= 0x7E {
            let glyph_offset = ((ascii - 0x20) as usize) * 16;
            if glyph_offset + 16 <= crate::drivers::console::FONT_8X16.len() {
                for dy in 0..16 {
                    let rbits = crate::drivers::console::FONT_8X16[glyph_offset + dy];
                    for dx in 0..8 {
                        if rbits & (0x80 >> dx) != 0 {
                            fb.draw_pixel(cx + dx, y + dy, color);
                        }
                    }
                }
            }
        }
        cx += 8;
    }
}

/// Write a number as text (simple itoa for small numbers)
fn write_num(console: &mut FramebufferConsole, fb: &mut AetherFrameBuffer, prefix: &str, n: usize) {
    console.write_str(fb, prefix);
    if n >= 10 {
        let d = ((n / 10) as u8 + b'0') as char;
        console.write_char(fb, d);
    }
    let d = ((n % 10) as u8 + b'0') as char;
    console.write_char(fb, d);
}

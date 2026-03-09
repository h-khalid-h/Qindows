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
const BG_DEEP:       u32 = 0x00_06_06_0E; // Desktop background
const BG_SURFACE:    u32 = 0x00_0C_0C_1A; // Panel background
const BG_TASKBAR:    u32 = 0x00_0A_0E_17; // Taskbar background
const ACCENT_CYAN:   u32 = 0x00_06_D6_A0; // Primary accent (Q green-cyan)
const ACCENT_BLUE:   u32 = 0x00_00_C8_FF; // Secondary accent
const ACCENT_GOLD:   u32 = 0x00_FF_D7_00; // Warning/highlight
const TEXT_PRIMARY:  u32 = 0x00_E0_E8_F0; // Main text
const TEXT_DIM:      u32 = 0x00_60_68_78; // Muted text
const BORDER:        u32 = 0x00_1A_20_30; // Subtle borders
const STATUS_GREEN:  u32 = 0x00_06_D6_A0; // Active/OK
const STATUS_YELLOW: u32 = 0x00_FF_BD_2E; // Warning
const STATUS_RED:    u32 = 0x00_EF_47_6F; // Error

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

    let dot_color = 0x00_20_24_38;
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
/// Resolves the Vector Scene Graph into an ARGB pixel buffer.
fn rasterize_aether_frame(fb: &mut AetherFrameBuffer, frame: &RenderFrame) {
    for cmd in &frame.commands {
        match cmd {
            RenderCommand::RoundedRect { x, y, width, height, radius, fill, border } => {
                let fill_u32 = color_to_u32(fill);
                draw_rounded_rect_alpha(fb, *x as usize, *y as usize, *width as usize, *height as usize, *radius as usize, fill_u32, (fill.a * 255.0) as u32);
                if let Some((stroke_w, stroke_c)) = border {
                    // Simple hacky stroke outline
                    draw_rounded_rect_alpha(fb, *x as usize, *y as usize, *width as usize, *stroke_w as usize, *radius as usize, color_to_u32(stroke_c), (stroke_c.a * 255.0) as u32);
                }
            }
            RenderCommand::GlassBlur { x, y, width, height, radius, tint, .. } => {
                // We can't actually run a 20px gaussian blur on software in real-time,
                // so we approximate Q-Glass with highly translucent dark tint.
                let tint_u32 = color_to_u32(tint);
                draw_rounded_rect_alpha(fb, *x as usize, *y as usize, *width as usize, *height as usize, *radius as usize, tint_u32, (tint.a * 255.0) as u32);
            }
            RenderCommand::Shadow { x, y, width, height, radius, offset_y, color, .. } => {
                let col = color_to_u32(color);
                let alpha = (color.a * 255.0) as u32;
                // Layer drop shadows
                draw_rounded_rect_alpha(fb, (*x) as usize, (*y + *offset_y) as usize, *width as usize, *height as usize, *radius as usize, col, alpha);
            }
            RenderCommand::Gradient { x, y, width, height, start_color, end_color, .. } => {
                // Simple vertical gradient fill
                for py in (*y as usize)..(*y as usize + *height as usize) {
                    let t = (py as f32 - *y) / *height;
                    let c = start_color.lerp(end_color, t);
                    let cu32 = color_to_u32(&c);
                    let a = (c.a * 255.0) as u32;
                    fill_rect_alpha(fb, *x as usize, py, *width as usize, 1, cu32, a);
                }
            }
            RenderCommand::Text { x, y, text, color, .. } => {
                draw_text_exact(fb, *x as f32 as usize, *y as f32 as usize, text, color_to_u32(color));
            }
            _ => {} // Ignore images/clips for the prototype
        }
    }
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
    draw_rounded_rect_alpha(fb, panel_x, panel_y, panel_w, panel_h, 12, 0x00_FF_FF_FF, 20);

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

    wm.create_window(5, alloc::string::String::from("Q-Shell - admin@qindows: ~"), 100.0, 120.0, 600.0, 400.0);
    wm.create_window(0, alloc::string::String::from("System Monitor"), 750.0, 200.0, 500.0, 350.0);

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

    save_bg(fb, cursor_x, cursor_y, &mut saved_bg);
    draw_cursor(fb, cursor_x, cursor_y);

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
                            for line in output.lines() {
                                qshell_lines.push(alloc::string::String::from(line));
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
                        if last.len() > 17 { // Protect prompt
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

/// Helper function to re-compose the Aether desktop given window manager state
fn render_scene(fb: &mut AetherFrameBuffer, wm: &aether::window::WindowManager, _console: &mut FramebufferConsole, qshell_lines: &[alloc::string::String], mx: f32, my: f32) {
    let w = fb.width();
    let h = fb.height();

    // Redraw wallpaper
    fb.clear(BG_DEEP);
    for y in (0..h).step_by(48) { for x in (0..w).step_by(48) { fb.draw_pixel(x, y, 0x00_20_24_38); } }
    draw_orb(fb, w / 4, h / 3, 600, ACCENT_CYAN, 30);
    draw_orb(fb, (w * 3) / 4, (h * 2) / 3, 800, ACCENT_BLUE, 25);
    draw_orb(fb, w - 200, 100, 400, ACCENT_GOLD, 15);
    draw_large_q_watermark(fb, w / 2 - 150, h / 2 - 150);

    // Dynamic windows based on Z-order
    for win in wm.visible_windows() {
        draw_window(fb, win.x as usize, win.y as usize, win.width as usize, win.height as usize, &win.title, win.focused, mx, my);
        
        let title_c = if win.focused { TEXT_PRIMARY } else { TEXT_DIM };
        draw_text_exact(fb, win.x as usize + 20, win.y as usize + 13, &win.title, title_c);
        
        if win.title.contains("Q-Shell") {
            for (i, line) in qshell_lines.iter().enumerate() {
                let ty = win.y as usize + 50 + (i * 20);
                if ty + 16 < (win.y + win.height) as usize {
                    draw_text_exact(fb, win.x as usize + 20, ty, line, ACCENT_CYAN);
                }
            }
            if win.focused {
                let last = qshell_lines.last().unwrap();
                let cx = win.x as usize + 20 + (last.len() * 8);
                let cy = win.y as usize + 50 + ((qshell_lines.len() - 1) * 20);
                fill_rect_alpha(fb, cx, cy, 8, 16, ACCENT_CYAN, 200); // Block cursor
            }
        } else {
            draw_text_exact(fb, win.x as usize + 20, win.y as usize + 60, "Genesis Protocol Initiated...", STATUS_GREEN);
            draw_text_exact(fb, win.x as usize + 20, win.y as usize + 90, "[OK] Aether Display Active", TEXT_PRIMARY);
        }
    }

    // Taskbar Glass (Enhanced Gradient)
    let tb_y = (h - TASKBAR_HEIGHT) as f32;
    draw_gradient_rect_alpha(fb, 0, tb_y as usize, w, TASKBAR_HEIGHT, 0, 0x00_15_18_22, 0x00_0A_0E_17, 240);
    draw_rounded_rect_alpha(fb, 0, tb_y as usize, w, 1, 0, 0x00_2A_2B_36, 150); // Top border
    
    // Start button (Hover state + glow)
    let hover_start = mx >= 16.0 && mx <= 48.0 && my >= tb_y + 4.0 && my <= tb_y + 36.0;
    draw_rounded_rect_alpha(fb, 16, tb_y as usize + 4, 32, 32, 8, if hover_start { 0x00_10_E6_B0 } else { ACCENT_CYAN }, 255);
    draw_q_letter(fb, 26, tb_y as usize + 7, BG_DEEP);

    // Re-render clock using native font
    let mut rtc = crate::rtc::Rtc::new();
    let time = rtc.read_time();
    
    let mut time_str = alloc::string::String::new();
    let h_12 = if time.hour == 0 { 12 } else if time.hour > 12 { time.hour - 12 } else { time.hour };
    if h_12 < 10 { time_str.push('0'); }
    let h1 = (h_12 / 10) as u8 + b'0';
    let h2 = (h_12 % 10) as u8 + b'0';
    time_str.push(h1 as char); time_str.push(h2 as char); time_str.push(':');
    
    let m1 = (time.minute / 10) as u8 + b'0';
    let m2 = (time.minute % 10) as u8 + b'0';
    time_str.push(m1 as char); time_str.push(m2 as char);
    if time.hour >= 12 { time_str.push_str(" PM"); } else { time_str.push_str(" AM"); }
    
    draw_text_exact(fb, w - 80, tb_y as usize + 14, &time_str, TEXT_PRIMARY);
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
    let main_color = 0x00_FF_FF_FF; // White
    let outline = 0x00_00_00_00;    // Black

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
    let color = 0x00_FF_FF_FF;
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
                let bg = fb.read_pixel(px, py);
                fb.draw_pixel(px, py, blend(bg, color, alpha));
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
    draw_rounded_rect_alpha(fb, x, y, w, h, 12, 0x00_0D_0E_15, 240); // 94% opaque
    
    // 1px Border highlight
    let border_col = if focused { 0x00_06_D6_A0 } else { 0x00_2A_2B_36 }; 
    draw_rounded_rect_alpha(fb, x, y, w, h, 12, border_col, if focused { 180 } else { 150 });
    draw_rounded_rect_alpha(fb, x+1, y+1, w-2, h-2, 11, 0x00_0D_0E_15, 255); // Reset inside

    // Header bar (Gradient)
    draw_gradient_rect_alpha(fb, x+1, y+1, w-2, 40, 11, 0x00_1F_22_32, 0x00_18_1A_24, 255);
    fill_rect_alpha(fb, x+1, y+20, w-2, 21, 0x00_18_1A_24, 255); // Straighten bottom
    fill_rect_alpha(fb, x, y + 40, w, 1, 0x00_00_00_00, 180);

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

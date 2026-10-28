#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // Hide console in release

mod link;
mod midi;
mod ui;
mod shutdown;

use eframe::egui;
use link::LinkManager;
use midi::MidiContext;
use ui::ClockApp;
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem}, Icon};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use once_cell::sync::Lazy;
use crate::shutdown::log_exit_attempt;

static AUTO_QUIT: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));
static AUTO_SHOW: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let link = LinkManager::new(120.0);
    let midi = MidiContext::new(link.clone())?;

    // Tray Setup
    let tray_menu = Menu::new();
    let show_i = MenuItem::new("Show", true, None);
    let quit_i = MenuItem::new("Quit", true, None);
    tray_menu.append_items(&[&show_i, &quit_i]).unwrap();

    let icon = create_tray_icon();
    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Ableton Link MIDI Clock")
        .with_icon(icon)
        .build()?;

    // If AUTO_QUIT_MS is set, spawn a thread to trigger shutdown after given ms (debug helper)
    if let Ok(ms) = std::env::var("AUTO_QUIT_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            println!("AUTO_QUIT_MS set: will trigger quit after {} ms", ms);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(ms));
                AUTO_QUIT.store(true, Ordering::SeqCst);
            });
        }
    }

    // Register Ctrl-C handler to log attempts and trigger immediate shutdown hooks
    if ctrlc::set_handler(|| {
        log_exit_attempt("ctrl_c");
        // Invoke immediate shutdown hooks (best-effort)
        crate::shutdown::invoke_shutdown_hooks();
        AUTO_QUIT.store(true, Ordering::SeqCst);
    }).is_err() {
        println!("Failed to set Ctrl-C handler");
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([320.0, 240.0])
            .with_icon(create_window_icon()),
        ..Default::default()
    };

    let show_id = show_i.id().clone();
    let quit_id = quit_i.id().clone();

    // Spawn a background thread that listens for tray menu events (blocking) and triggers shutdown
    // or show requests even if the UI thread misses the event for some reason.
    {
        let show_id_bg = show_id.clone();
        let quit_id_bg = quit_id.clone();
        std::thread::spawn(move || {
            loop {
                match tray_icon::menu::MenuEvent::receiver().recv() {
                    Ok(event) => {
                        if event.id == quit_id_bg {
                            crate::shutdown::log_exit_attempt("tray_quit_bg");
                            println!("Tray BG: Quit requested — invoking shutdown hooks");
                            crate::shutdown::invoke_shutdown_hooks();
                            AUTO_QUIT.store(true, Ordering::SeqCst);
                        } else if event.id == show_id_bg {
                            println!("Tray BG: Show requested");
                            AUTO_SHOW.store(true, Ordering::SeqCst);
                        } else {
                            println!("Tray BG: unknown menu id: {:?}", event.id);
                        }
                    }
                    Err(e) => {
                        println!("Tray BG: receiver error: {:?}", e);
                        break;
                    }
                }
            }
        });
    }

    eframe::run_native(
        "Rust Link MIDI",
        options,
        Box::new(move |cc| {
            let app = ClockApp::new(cc, link, midi);
            // Inject tray IDs into app (we need to modify ClockApp to accept them or set them)
            // Since ClockApp::new is defined in ui.rs, I'll modify ui.rs to accept them or add a setter.
            // For now, I'll wrap the app or modify ui.rs.
            // Let's modify ui.rs to handle tray logic if I can pass the IDs.
            // Or I can handle tray logic in a wrapper in main, but eframe expects one App.
            // I will modify ClockApp in ui.rs to accept these IDs.
            Ok(Box::new(ClockAppWithTray {
                app,
                show_id,
                quit_id,
                should_exit: false,
                tray_empty_polls: 0,
            }))
        }),
    )?;

    // Mark shutdown complete to stop watchdog if everything cleaned up successfully
    crate::shutdown::mark_shutdown_complete();
    println!("run_native returned — main exiting");
    Ok(())
}

struct ClockAppWithTray {
    app: ClockApp,
    show_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
    should_exit: bool,
    tray_empty_polls: u32,
}

impl eframe::App for ClockAppWithTray {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle Tray Events
        match tray_icon::menu::MenuEvent::receiver().try_recv() {
            Ok(event) => {
                // reset empty poll counter
                self.tray_empty_polls = 0;
                if event.id == self.quit_id {
                    log_exit_attempt("tray_quit");
                    println!("Tray menu: Quit requested — performing controlled shutdown");
                    // Invoke shutdown hooks immediately so background threads receive Stop right away
                    crate::shutdown::invoke_shutdown_hooks();
                    // Perform app-level shutdown to drop resources deterministically
                    self.app.shutdown();
                    self.should_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    // Close requested; will allow close to proceed on the next update cycle.
                } else if event.id == self.show_id {
                    println!("Tray menu: Show requested");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else {
                    println!("Tray menu: unknown menu id: {:?}", event.id);
                }
            }
            Err(e) => {
                use crossbeam_channel::TryRecvError;
                match e {
                    TryRecvError::Empty => {
                        self.tray_empty_polls = self.tray_empty_polls.saturating_add(1);
                        if self.tray_empty_polls % 600 == 0 {
                            println!("Tray menu receiver empty for {} polls", self.tray_empty_polls);
                        }
                    }
                    TryRecvError::Disconnected => {
                        println!("Tray menu receiver disconnected");
                    }
                }
            }
        }
        
        if let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
             if let tray_icon::TrayIconEvent::Click { button: tray_icon::MouseButton::Left, .. } = event {
                 println!("Tray icon clicked — showing window");
                 ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                 ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
             }
        }

        // Handle show requests originating from the background tray listener
        if AUTO_SHOW.swap(false, Ordering::SeqCst) {
            println!("AUTO_SHOW: showing window due to background tray event");
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        // Auto-quit helper (debug): if set by env var, trigger quit here
        if AUTO_QUIT.swap(false, Ordering::SeqCst) {
            log_exit_attempt("auto_quit");
            println!("AUTO_QUIT: triggering quit now");
            self.should_exit = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Handle Close Request
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.should_exit {
                log_exit_attempt("window_close_hide");
                println!("Close requested: hiding to tray (CancelClose)");
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                log_exit_attempt("window_close_proceed");
                println!("Close requested: proceeding with close (should_exit)");
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Delegate to main app
        self.app.draw(ui);
    }
}

fn create_tray_icon() -> Icon {
    let width = 64;
    let height = 64;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        // Greenish
        rgba.push(0);
        rgba.push(255);
        rgba.push(0);
        rgba.push(255);
    }
    Icon::from_rgba(rgba, width, height).unwrap()
}

fn create_window_icon() -> Arc<egui::IconData> {
    let width = 64;
    let height = 64;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        rgba.push(0);
        rgba.push(255);
        rgba.push(0);
        rgba.push(255);
    }
    Arc::new(egui::IconData {
        rgba,
        width,
        height,
    })
}

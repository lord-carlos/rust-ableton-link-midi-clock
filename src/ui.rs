use eframe::egui;
use crate::link::LinkManager;
use crate::midi::MidiContext;

pub struct ClockApp {
    link: Option<LinkManager>,
    midi: Option<MidiContext>,
    selected_midi_port: Option<String>,
    available_ports: Vec<String>,
}

impl ClockApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, link: LinkManager, mut midi: MidiContext) -> Self {
        let available_ports = midi.refresh_ports();
        Self {
            link: Some(link),
            midi: Some(midi),
            selected_midi_port: None,
            available_ports,
        }
    }

    pub fn shutdown(&mut self) {
        crate::shutdown::log_exit_attempt("app_shutdown");
        println!("ClockApp shutdown: dropping MIDI and Link managers");
        self.midi = None;
        self.link = None;
    }
}

impl ClockApp {
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Ableton Link MIDI Clock");
            ui.add_space(10.0);

            // Link Status
            ui.group(|ui| {
                ui.label("Ableton Link Status");
                if let Some(link) = &self.link {
                    if let Some(snapshot) = link.capture_snapshot() {
                        ui.label(format!("Tempo: {:.2} BPM", snapshot.session.tempo()));
                        ui.label(format!("Peers: {}", snapshot.num_peers));
                        
                        // Optional: Phase visualization
                        let phase = snapshot.session.phase_at_time(snapshot.clock.micros(), 4.0);
                        ui.label(format!("Phase: {:.2}", phase));
                    } else {
                        ui.label("Initializing Link...");
                    }
                } else {
                    ui.label("Link shut down");
                }
            });

            ui.add_space(10.0);

            // MIDI Settings
            ui.group(|ui| {
                ui.label("MIDI Output");
                
                let selected_text = self.selected_midi_port.clone().unwrap_or_else(|| "Select Device".to_string());
                
                egui::ComboBox::from_label("Device")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        for port in &self.available_ports {
                            if ui.selectable_value(&mut self.selected_midi_port, Some(port.clone()), port).clicked() {
                                if let Some(p) = &self.selected_midi_port {
                                    if let Some(m) = &mut self.midi {
                                        m.select_port(p.clone());
                                    }
                                }
                            }
                        }
                    });
                
                if ui.button("Refresh Ports").clicked() {
                    if let Some(m) = &mut self.midi {
                        self.available_ports = m.refresh_ports();
                    } else {
                        self.available_ports = Vec::new();
                    }
                }
            });
            
            ui.add_space(20.0);
            ui.label("Close window to minimize to tray.");
        });

        // Continuous update for BPM/Phase display
        ui.request_repaint();
    }
}

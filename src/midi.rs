use midir::{MidiOutput, MidiOutputConnection};
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;
use crate::link::LinkManager;
use anyhow::Result;

pub enum MidiCommand {
    SelectPort(String),
    Stop,
}

pub struct MidiContext {
    command_sender: Sender<MidiCommand>,
    available_ports: Vec<String>,
}

impl Drop for MidiContext {
    fn drop(&mut self) {
        println!("MidiContext Drop: sending Stop to MIDI thread");
        let _ = self.command_sender.send(MidiCommand::Stop);
    }
}

impl MidiContext {
    pub fn new(link: LinkManager) -> Result<Self> {
        let midi_out = MidiOutput::new("RustLinkMidiClock")?;
        let available_ports = Self::list_ports(&midi_out);
        
        let (sender, receiver) = unbounded();
        
        // Spawn the clock thread
        thread::spawn(move || run_midi_thread(receiver, link));

        // Register a shutdown hook so that a forced immediate shutdown will stop the MIDI thread
        crate::shutdown::register_shutdown_hook({
            let sender = sender.clone();
            move || {
                println!("shutdown hook: sending Stop to MIDI thread");
                let _ = sender.send(MidiCommand::Stop);
            }
        });

        Ok(Self {
            command_sender: sender,
            available_ports,
        })
    }

    fn list_ports(midi_out: &MidiOutput) -> Vec<String> {
        let mut ports = Vec::new();
        for p in midi_out.ports() {
            if let Ok(name) = midi_out.port_name(&p) {
                ports.push(name);
            }
        }
        ports
    }

    pub fn refresh_ports(&mut self) -> Vec<String> {
        // Re-create MidiOutput to refresh ports if needed, or just use the existing one
        // midir ports are dynamic?
        // Actually, creating a new MidiOutput is cheap enough for refresh
        if let Ok(new_out) = MidiOutput::new("RustLinkMidiClock_Probe") {
             self.available_ports = Self::list_ports(&new_out);
        }
        self.available_ports.clone()
    }

    pub fn select_port(&self, port_name: String) {
        self.command_sender.send(MidiCommand::SelectPort(port_name)).ok();
    }
}

fn run_midi_thread(receiver: Receiver<MidiCommand>, link: LinkManager) {

    let mut midi_out = MidiOutput::new("RustLinkMidiClock_Thread").ok();
    let mut conn: Option<MidiOutputConnection> = None;
    let mut next_tick_index = -1i64;
    
    loop {
        // 1. Process Commands
        match receiver.try_recv() {
            Ok(MidiCommand::SelectPort(name)) => {
                conn = None;
                if midi_out.is_none() {
                    midi_out = MidiOutput::new("RustLinkMidiClock_Thread").ok();
                }
                let mut port_to_connect = None;
                if let Some(ref out) = midi_out {
                    for p in out.ports() {
                        if let Ok(p_name) = out.port_name(&p) {
                            if p_name == name {
                                port_to_connect = Some(p);
                                break;
                            }
                        }
                    }
                }
                if let Some(p) = port_to_connect {
                    if let Some(out) = midi_out.take() {
                        if let Ok(c) = out.connect(&p, "clock-out") {
                            conn = Some(c);
                            println!("Connected to MIDI port: {}", name);
                        }
                    }
                }
                next_tick_index = -1;
            },
            Ok(MidiCommand::Stop) => {
                println!("MIDI thread: received Stop, exiting");
                break
            },
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {},
        }

        // 2. Sync and Send
        if let Some(snapshot) = link.capture_snapshot() {
            let now = snapshot.clock.micros();
            
            // Current beat
            let beat = snapshot.session.beat_at_time(now, 1.0);
            
            if next_tick_index == -1 {
                next_tick_index = (beat * 24.0).ceil() as i64;
            }
            
            // Calculate target time for next tick
            let target_beat = next_tick_index as f64 / 24.0;
            let target_time = snapshot.session.time_at_beat(target_beat, 1.0);
            
            let wait = target_time - now;
            let wait_micros = wait.num_microseconds().unwrap_or(0);

            // If we have more than 2ms to wait, sleep but wake up 1ms early and re-evaluate
            if wait_micros > 2000 {
                thread::sleep(Duration::from_micros((wait_micros - 1000) as u64));
                // Re-evaluate timing on next loop iteration
                continue;
            } else if wait_micros > 0 {
                // Busy-wait until the clock reaches the target time
                while snapshot.clock.micros() < target_time {
                    std::hint::spin_loop();
                }
                if let Some(c) = &mut conn {
                    let _ = c.send(&[0xF8]);
                }
                next_tick_index += 1;
            } else {
                // We're late. Avoid bursting many immediate ticks which causes external devices to see large BPM jumps.
                // If we're way too late, resync the tick index to the upcoming tick and skip sending now.
                if wait_micros < -100_000 {
                    next_tick_index = (beat * 24.0).ceil() as i64;
                    continue;
                }

                // If we're only slightly late (within 2ms), it's okay to send the tick immediately.
                if wait_micros > -2000 {
                    if let Some(c) = &mut conn {
                        let _ = c.send(&[0xF8]);
                    }
                    next_tick_index += 1;
                } else {
                    // For moderate lateness, resync to the next appropriate tick to keep spacing stable.
                    next_tick_index = (beat * 24.0).ceil() as i64;
                    continue;
                }
            }
        } else {
            thread::sleep(Duration::from_millis(10));
        }
    }
    
    println!("MIDI thread: exiting");

}


use ableton_link_rs::link::{BasicLink, SessionState};
use ableton_link_rs::link::clock::Clock;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use tokio::runtime::Runtime;

#[derive(Clone)]
pub struct LinkStateSnapshot {
    pub session: SessionState,
    pub clock: Clock,
    pub num_peers: usize,
}

#[derive(Clone)]
pub struct LinkManager {
    state: Arc<RwLock<Option<LinkStateSnapshot>>>,
    command_tx: crossbeam_channel::Sender<LinkCommand>,
}

enum LinkCommand {
    #[allow(dead_code)] // Part of the public set_tempo() command API
    SetTempo(f64),
    Stop,
}

impl LinkManager {
    pub fn new(initial_bpm: f64) -> Self {
        let state = Arc::new(RwLock::new(None));
        let (tx, rx) = crossbeam_channel::unbounded();
        
        let state_clone = state.clone();
        
        // Register shutdown hook to stop the link background thread immediately when shutdown is invoked
        let tx_clone = tx.clone();
        crate::shutdown::register_shutdown_hook(move || {
            println!("shutdown hook: sending Stop to Link background thread");
            let _ = tx_clone.send(LinkCommand::Stop);
        });

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let mut link = BasicLink::new(initial_bpm).await;
                link.enable().await;
                link.enable_start_stop_sync(true);

                let mut interval = tokio::time::interval(Duration::from_millis(10));

                'outer: loop {
                    // 1. Handle Commands
                    while let Ok(cmd) = rx.try_recv() {
                        match cmd {
                            LinkCommand::SetTempo(bpm) => {
                                // Apply tempo change requested by the application
                                let mut session = link.capture_app_session_state();
                                let time = link.clock().micros();
                                session.set_tempo(bpm, time);
                                link.commit_app_session_state(session).await;
                            }
                            LinkCommand::Stop => {
                                println!("Link background: received Stop, exiting");
                                // Gracefully exit the background task
                                break 'outer;
                            }
                        }
                    }

                    // 2. Update State
                    interval.tick().await;

                    let session = link.capture_app_session_state();
                    let clock = link.clock();
                    let num_peers = link.num_peers();

                    if let Ok(mut guard) = state_clone.write() {
                        *guard = Some(LinkStateSnapshot {
                            session,
                            clock,
                            num_peers,
                        });
                    }
                }
            });
            println!("Link background thread: exiting");
        });

        Self {
            state,
            command_tx: tx,
        }
    }

    pub fn capture_snapshot(&self) -> Option<LinkStateSnapshot> {
        self.state.read().ok()?.clone()
    }

    /// Request the Link thread to set the global tempo (BPM)
    #[allow(dead_code)] // Fully implemented API; tempo control in the UI is pending
    pub fn set_tempo(&self, bpm: f64) {
        let _ = self.command_tx.send(LinkCommand::SetTempo(bpm));
    }
}

impl Drop for LinkManager {
    fn drop(&mut self) {
        println!("LinkManager Drop: sending Stop to background thread");
        // Signal the background thread to stop; ignore errors
        let _ = self.command_tx.send(LinkCommand::Stop);
    }
}

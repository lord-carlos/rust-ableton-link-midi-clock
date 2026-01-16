use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};

static HOOKS: OnceLock<Mutex<Vec<Box<dyn Fn() + Send + 'static>>>> = OnceLock::new();
static INVOKED: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_COMPLETE: AtomicBool = AtomicBool::new(false);

/// Log an exit attempt with a timestamp to stdout and an append-only file in the system temp dir.
pub fn log_exit_attempt(reason: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let line = format!("{}.{} {}\n", now.as_secs(), now.subsec_nanos(), reason);
    println!("Exit attempt: {}", line.trim());

    let path = std::env::temp_dir().join("rust_link_midi_exit_attempts.log");

    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Register a shutdown hook to be called when shutdown is invoked.
pub fn register_shutdown_hook<F: Fn() + Send + 'static>(f: F) {
    let m = HOOKS.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut guard) = m.lock() {
        guard.push(Box::new(f));
    }
}

/// Invoke all registered shutdown hooks once and start a watchdog that force-exits after 8s if shutdown
/// hasn't been marked complete.
pub fn invoke_shutdown_hooks() {
    if INVOKED.swap(true, Ordering::SeqCst) {
        return;
    }

    // Log the invocation
    log_exit_attempt("shutdown_invoke");

    if let Some(m) = HOOKS.get() {
        if let Ok(guard) = m.lock() {
            for hook in guard.iter() {
                // Run each hook in its own thread to avoid blocking other hooks
                let h = hook as *const _;
                // SAFETY: we're only calling the closure immutably from the same process; clone pointer
                let closure = unsafe { &*h };
                let closure = closure as &dyn Fn();
                // Call directly (in current thread) to keep ordering
                closure();
            }
        }
    }

    // Start watchdog to force-exit if shutdown does not complete in time
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(8));
        if !SHUTDOWN_COMPLETE.load(Ordering::SeqCst) {
            println!("Shutdown watchdog: timeout, forcing exit");
            std::process::exit(1);
        }
    });
}

/// Mark that shutdown completed successfully; prevents the watchdog from exiting the process.
pub fn mark_shutdown_complete() {
    SHUTDOWN_COMPLETE.store(true, Ordering::SeqCst);
}
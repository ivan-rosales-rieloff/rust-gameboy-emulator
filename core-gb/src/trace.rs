//! Trace logging utilities for the Game Boy emulator.
//!
//! Provides debug tracing capabilities that can write to:
//! - Standard error output (stderr)
//! - A file on disk (optional)
//!
//! Configuration via environment variables:
//! - `GB_TRACE`: Set to any value to enable tracing
//! - `GB_TRACE_FILE`: Path to file for trace output (optional, appends to file)
//!
//! Example:
//! ```bash
//! set GB_TRACE=1
//! set GB_TRACE_FILE=trace.log
//! ```

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

static GB_TRACE_ENABLED: OnceLock<bool> = OnceLock::new();
static TRACE_FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

/// Checks if tracing is enabled via the GB_TRACE environment variable.
pub fn trace_enabled() -> bool {
    *GB_TRACE_ENABLED.get_or_init(|| std::env::var_os("GB_TRACE").is_some())
}

/// Initializes the trace file if GB_TRACE_FILE environment variable is set.
///
/// This is called automatically on first trace() call.
/// Returns true if file was successfully initialized or doesn't need to be.
fn init_trace_file() -> bool {
    TRACE_FILE.get_or_init(|| match std::env::var("GB_TRACE_FILE") {
        Ok(path) => match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                eprintln!("[GB TRACE] Logging to file: {}", path);
                Some(Mutex::new(file))
            }
            Err(e) => {
                eprintln!("[GB TRACE] Failed to open trace file '{}': {}", path, e);
                None
            }
        },
        Err(_) => None,
    });
    true
}

/// Writes a trace message to stderr and/or trace file.
///
/// Messages are prefixed with "[GB TRACE]" for easy filtering.
/// Only writes if tracing is enabled via GB_TRACE environment variable.
///
/// # Arguments
/// * `message` - The trace message to write
///
/// # Example
/// ```ignore
/// trace("CPU step: PC=0x0100, A=0x01");
/// ```
pub fn trace(message: &str) {
    if trace_enabled() {
        let formatted = format!("[GB TRACE] {}", message);

        // Always write to stderr
        // eprintln!("{}", formatted); // removed to avoid locks on running code, can be uncommented for debugging purposes

        // Also write to file if configured
        init_trace_file();
        if let Some(file_opt) = TRACE_FILE.get() {
            if let Some(file_mutex) = file_opt {
                if let Ok(mut file) = file_mutex.lock() {
                    let _ = writeln!(file, "{}", formatted);
                    let _ = file.flush();
                }
            }
        }
    }
}

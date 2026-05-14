use std::sync::OnceLock;

static GB_TRACE_ENABLED: OnceLock<bool> = OnceLock::new();

pub fn trace_enabled() -> bool {
    *GB_TRACE_ENABLED.get_or_init(|| std::env::var_os("GB_TRACE").is_some())
}

pub fn trace(message: &str) {
    if trace_enabled() {
        eprintln!("[GB TRACE] {message}");
    }
}

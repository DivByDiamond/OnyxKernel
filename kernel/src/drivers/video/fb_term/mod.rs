use super::fb;

pub mod ansi;
pub mod writer;
pub use writer::FbWriter;

static mut G_WRITER: FbWriter = FbWriter::new();

pub fn writer() -> Option<&'static mut FbWriter> {
    // SAFETY: G_WRITER is a kernel-lifetime static; SIE=0 prevents same-hart preemption (see crate::sync). Cross-hart concurrent writer() callers would alias the returned &'static mut — tolerated by console convention (no reallocation, output may interleave); the reference must not outlive the caller's statement.
    unsafe {
        if fb::enabled() {
            Some(&mut G_WRITER)
        } else {
            None
        }
    }
}

pub fn write_str(s: &str) {
    if let Some(w) = writer() {
        w.puts(s);
    }
}

pub fn write_char(c: u8) {
    if let Some(w) = writer() {
        w.putc(c);
    }
}

pub fn write_unicode(cp: u32) {
    if let Some(w) = writer() {
        w.put_unicode(cp);
    }
}

pub fn write_unicode_str(s: &str) {
    if let Some(w) = writer() {
        w.puts_unicode(s);
    }
}

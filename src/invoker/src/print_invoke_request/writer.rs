use std::{
    cell::{Cell, RefCell},
    fmt::Display,
};

pub struct Writer {
    buf: RefCell<String>,
    depth: Cell<usize>,
}

impl Writer {
    pub fn new() -> Self {
        Writer {
            buf: RefCell::new(String::new()),
            depth: Cell::new(0),
        }
    }

    fn write_padded(&self, msg: &str) {
        let mut buf = self.buf.borrow_mut();
        for _ in 0..self.depth.get() {
            buf.push_str("    ");
        }
        buf.push_str(msg);
        buf.push('\n');
    }

    pub fn write_key_value(&self, k: &str, v: impl Display) {
        let msg = format!("{}: {}", k, v);
        self.write_padded(&msg);
    }

    pub fn begin(&self, section: &str) -> ScopeGuard<'_> {
        self.write_padded(&format!("{} {{", section));
        self.depth.set(self.depth.get() + 1);
        ScopeGuard(self)
    }

    pub fn into_inner(self) -> String {
        self.buf.into_inner()
    }
}

pub struct ScopeGuard<'a>(&'a Writer);

impl Drop for ScopeGuard<'_> {
    fn drop(&mut self) {
        self.0.depth.set(self.0.depth.get() - 1);
        self.0.write_padded("}");
    }
}

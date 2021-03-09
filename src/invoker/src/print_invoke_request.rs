//! Implements pretty-printing of invocation request
mod writer;

use invoker_api::invoke::{InputSource, InvokeRequest};
use writer::Writer;

pub struct PrintWrapper<'a>(pub &'a InvokeRequest);

impl PrintWrapper<'_> {
    fn print_config(&self, w: &mut Writer) {
        let _s = w.begin("globals");
        w.write_key_value("request-id", self.0.id.to_hyphenated());
    }

    fn print_inputs(&self, w: &mut Writer) {
        let _s = w.begin("inputs");
        for inp in &self.0.inputs {
            let _s = w.begin("input");
            w.write_key_value("file-id", &inp.file_id);
            match &inp.source {
                InputSource::Inline { data } => {
                    w.write_key_value("inline", format_args!("{} bytes", data.len()))
                }
                InputSource::LocalFile { path } => {
                    w.write_key_value("local-path", path.display());
                }
            }
        }
    }

    pub fn print(&self) -> String {
        let mut w = Writer::new();
        self.print_config(&mut w);
        self.print_inputs(&mut w);
        w.into_inner()
    }
}

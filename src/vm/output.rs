//! Template output: the caller's sink and the capture stack.

use std::io;

/// Where rendered bytes go. Twig yields string chunks that `display()` echoes; twig-rs writes them to a sink.
///
/// Every [`std::io::Write`] is a sink: a `Vec<u8>` collects the output in memory, a file, a socket or a locked
/// stdout writes it out (`write` is `write_all`). Implement the trait for other destinations.
///
/// `flush` is called for `{% flush %}` when the template is displayed (never when it is rendered to a string, where
/// Twig's output buffer swallows the flush).
pub trait Sink {
    /// Writes a chunk of output.
    ///
    /// # Errors
    ///
    /// An I/O error stops the rendering.
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// Flushes buffered output (`{% flush %}`).
    ///
    /// # Errors
    ///
    /// An I/O error stops the rendering.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A writer is a sink: `write` writes all of the chunk, `{% flush %}` flushes the writer.
impl<W: io::Write + ?Sized> Sink for W {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        io::Write::write_all(self, bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(self)
    }
}

/// A borrowed sink is a sink, so that code holding a `&mut dyn Sink` can lend it on.
impl Sink for &mut dyn Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        (**self).write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        (**self).flush()
    }
}

/// A sink over any [`std::io::Write`]. Writers are sinks themselves, so this wrapper is not needed any more; it is
/// kept for code written before.
#[derive(Debug)]
pub struct IoSink<W: io::Write>(pub W);

impl<W: io::Write> Sink for IoSink<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// The output of a running render: the sink plus the stack of active captures (`CoreExtension::captureOutput()`'s
/// output buffers). Writes go to the innermost capture, or to the sink when nothing is captured.
pub(crate) struct Output<'a> {
    sink: Option<&'a mut dyn Sink>,
    captures: Vec<Vec<u8>>,
    flushes: bool,
}

impl<'a> Output<'a> {
    /// An output writing to `sink`; `flushes` passes `{% flush %}` on to the sink (display) or not (render).
    pub(crate) fn new(sink: &'a mut dyn Sink, flushes: bool) -> Self {
        Output {
            sink: Some(sink),
            captures: Vec::new(),
            flushes,
        }
    }

    /// An output without sink, for code that cannot write outside a capture (arrow functions).
    pub(crate) fn detached() -> Output<'static> {
        Output {
            sink: None,
            captures: Vec::new(),
            flushes: false,
        }
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        match self.captures.last_mut() {
            Some(capture) => {
                capture.extend_from_slice(bytes);
                Ok(())
            }
            None => match &mut self.sink {
                Some(sink) => sink.write(bytes),
                None => Ok(()),
            },
        }
    }

    pub(crate) fn flush(&mut self) -> io::Result<()> {
        if !self.flushes || !self.captures.is_empty() {
            return Ok(());
        }
        match &mut self.sink {
            Some(sink) => sink.flush(),
            None => Ok(()),
        }
    }

    pub(crate) fn start_capture(&mut self) {
        self.captures.push(Vec::new());
    }

    pub(crate) fn end_capture(&mut self) -> Vec<u8> {
        self.captures.pop().unwrap_or_default()
    }

    pub(crate) fn capture_depth(&self) -> usize {
        self.captures.len()
    }

    /// Drops the captures above `depth` (`ob_end_clean()` after an exception).
    pub(crate) fn truncate_captures(&mut self, depth: usize) {
        self.captures.truncate(depth);
    }
}

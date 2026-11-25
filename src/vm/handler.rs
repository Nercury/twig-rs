//! Exception handling of templates: `Template::handleException()` and `Error::guess()`.

use super::exception::{Exception, TraceFrame};
use super::state::VmState;
use crate::error::Error;
use crate::lltl::{FileId, Program};

/// Twig: `Template::handleException()` (TWIG/src/Template.php): a Twig error gets the handling template's source when
/// it has none and its line guessed when unknown (`-1`); any other exception is wrapped in
/// `RuntimeError('An exception has been thrown during the rendering of a template ("%s").', -1, $source, $e)` whose
/// line is guessed.
///
/// `program` is the template that handles the exception (`$this`: the template whose body runs in `yield()`, the owner
/// of the block in `yieldBlock()`, ...); `state`'s template stack must already be back at the handler (the frames of
/// the failed code popped).
pub(crate) fn handle_exception(
    state: &VmState,
    exception: Exception,
    program: &Program,
) -> Exception {
    let handler_trace = state.trace();
    if exception.is_twig_error() {
        let trace = exception.trace().cloned();
        let mut exception = exception;
        if let Some(error) = exception.kind_twig_mut() {
            if error.source_context().is_none() {
                error.set_source_context(Some(program.source().clone()));
            }
            if error.template_line() == -1 {
                let traces: Vec<&[TraceFrame]> = trace.as_deref().into_iter().collect();
                guess(error, state, program, &traces);
            }
        }
        return exception;
    }
    let message = exception.message();
    let (original, trace) = exception.into_parts();
    let mut wrapped = Error::runtime(format!(
        "An exception has been thrown during the rendering of a template (\"{message}\")."
    ))
    .with_line(-1)
    .with_source(program.source().clone());
    wrapped = match original {
        Ok(error) => wrapped.with_previous(error),
        Err(error) => wrapped.with_previous(error),
    };
    // the previous exception's trace first, then the wrapper's own (thrown from the handler)
    let mut traces: Vec<&[TraceFrame]> = Vec::new();
    if let Some(trace) = trace.as_deref() {
        traces.push(trace);
    }
    traces.push(&handler_trace);
    guess(&mut wrapped, state, program, &traces);
    Exception::from(wrapped).with_trace(Some(handler_trace))
}

/// Twig: `Error::guess()` / `guessTemplateInfo()` (TWIG/src/Error/Error.php): the template named like the error's
/// source is looked up on the current stack (the handler first, then its callers); its compiled file is then looked
/// up in the traces of the exception chain (innermost previous exception first, each from its throw site outwards);
/// the first frame of that file gives the line of the last marker before its current instruction. Without such a
/// template or frame the line becomes `0` (no line in the message).
///
/// Files are told apart as with a filesystem cache: with `cache => false`, PHP eval()s every template from the same
/// line of `Environment.php` and `guessTemplateInfo()` also matches the frames of other templates (an exception in a
/// macro imported from another template then takes the handling template's marker at or before the macro's PHP
/// line), which depends on the layout of the generated PHP code (reference/deviations.md).
pub(crate) fn guess(
    error: &mut Error,
    state: &VmState,
    handler: &Program,
    traces: &[&[TraceFrame]],
) {
    error.guess(|error| {
        let name = error.source_context()?.name().to_owned();
        let file: FileId = if handler.name() == name {
            handler.file()
        } else {
            state
                .frames()
                .iter()
                .rev()
                .find(|frame| frame.program.name() == name)?
                .program
                .file()
        };
        traces
            .iter()
            .flat_map(|trace| trace.iter())
            .filter(|frame| frame.file == file)
            .find_map(|frame| frame.line)
    });
}

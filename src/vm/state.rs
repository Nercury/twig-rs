//! The state of a running render that outlives frames: the environment, the template stack (for traces and the
//! depth limit) and the diagnostics routing.

use std::any::Any;
use std::sync::Arc;

use twig_value::{Diagnostic, Diagnostics, PhpError, ThrowableClass};

use super::exception::{Exception, Trace, TraceFrame};
use crate::lltl::{Program, UnitId};
use crate::runtime::Environment;

/// One active unit execution: the program, the unit and the instruction being executed.
pub(crate) struct ActiveFrame {
    pub(crate) program: Arc<Program>,
    pub(crate) unit: UnitId,
    pub(crate) ip: u32,
}

/// The state of a render shared by every frame: owned (`'static`), so that template closures called back from
/// `twig-value` code (sort comparators, host object methods) can reach it through [`Diagnostics::host`].
///
/// It is also the diagnostics sink of every PHP-level operation the VM performs: warnings, notices and deprecations go
/// to the environment's policy.
pub(crate) struct VmState {
    pub(crate) env: Environment,
    frames: Vec<ActiveFrame>,
    max_depth: usize,
}

impl VmState {
    pub(crate) fn new(env: Environment) -> Self {
        let max_depth = env.max_depth();
        VmState {
            env,
            frames: Vec::new(),
            max_depth,
        }
    }

    /// Enters a unit: pushes its frame on the template stack.
    ///
    /// The depth limit is a twig-rs addition (PHP recurses until it runs out of memory or stack); see
    /// `reference/deviations.md`.
    pub(crate) fn enter(&mut self, program: &Arc<Program>, unit: UnitId) -> Result<(), Exception> {
        if self.frames.len() >= self.max_depth {
            return Err(PhpError::new(
                ThrowableClass::Error,
                format!(
                    "Maximum template call depth of {} reached (twig-rs limit).",
                    self.max_depth
                ),
            )
            .into());
        }
        self.frames.push(ActiveFrame {
            program: Arc::clone(program),
            unit,
            ip: 0,
        });
        Ok(())
    }

    /// Leaves the current unit.
    pub(crate) fn leave(&mut self) {
        self.frames.pop();
    }

    /// Records the instruction the current frame executes.
    #[inline]
    pub(crate) fn set_ip(&mut self, ip: u32) {
        if let Some(frame) = self.frames.last_mut() {
            frame.ip = ip;
        }
    }

    /// The template stack, outermost frame first.
    pub(crate) fn frames(&self) -> &[ActiveFrame] {
        &self.frames
    }

    /// The template stack as a trace, innermost frame first, with the line of each frame's current instruction.
    pub(crate) fn trace(&self) -> Trace {
        self.frames
            .iter()
            .rev()
            .map(|frame| TraceFrame {
                file: frame.program.file(),
                line: frame.program.unit(frame.unit).lines().line_at(frame.ip),
            })
            .collect::<Vec<_>>()
            .into()
    }
}

impl Diagnostics for VmState {
    fn report(&mut self, diagnostic: Diagnostic) -> Result<(), PhpError> {
        self.env.report_diagnostic(diagnostic)
    }

    fn host(&mut self) -> Option<&mut dyn Any> {
        Some(self)
    }
}

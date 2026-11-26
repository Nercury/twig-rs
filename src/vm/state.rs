//! The state of a running render that outlives frames: the environment, the template stack (for traces and the
//! depth limit) and the diagnostics routing.

use std::any::Any;
use std::sync::Arc;

use twig_value::{Diagnostic, Diagnostics, PhpError, ThrowableClass, Value};

use super::exception::{Exception, Trace, TraceFrame};
use super::frame::FrameVecs;
use crate::extension::runtime::EscaperRuntime;
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
    /// Emptied argument vectors of finished calls, reused by the next calls.
    arg_pool: Vec<Vec<Value>>,
    /// The emptied vectors of the frames of finished units, reused by the next units.
    frame_pool: Vec<FrameVecs>,
    /// The environment's escaper runtime once a call needed it (`Environment::escaper_runtime()` never changes once
    /// created).
    escaper: Option<Arc<EscaperRuntime>>,
}

/// How many argument vectors a render keeps for reuse, and the largest capacity it keeps.
const ARG_POOL_SIZE: usize = 16;
const ARG_POOL_CAPACITY: usize = 16;
/// How many sets of frame vectors a render keeps for reuse, and the largest operand stack it keeps.
const FRAME_POOL_SIZE: usize = 16;
const FRAME_POOL_CAPACITY: usize = 64;

impl VmState {
    pub(crate) fn new(env: Environment) -> Self {
        let max_depth = env.max_depth();
        VmState {
            env,
            frames: Vec::new(),
            max_depth,
            arg_pool: Vec::new(),
            frame_pool: Vec::new(),
            escaper: None,
        }
    }

    /// Empty vectors for the frame of a unit.
    pub(crate) fn frame_vecs(&mut self) -> FrameVecs {
        self.frame_pool.pop().unwrap_or_else(|| FrameVecs {
            stack: Vec::with_capacity(8),
            ..FrameVecs::default()
        })
    }

    /// Takes back the vectors of the frame of a finished unit (their values are dropped).
    pub(crate) fn recycle_frame_vecs(&mut self, mut vecs: FrameVecs) {
        if vecs.stack.capacity() <= FRAME_POOL_CAPACITY
            && vecs.args.capacity() <= FRAME_POOL_CAPACITY
            && vecs.saved_macros.capacity() <= FRAME_POOL_CAPACITY
            && self.frame_pool.len() < FRAME_POOL_SIZE
        {
            vecs.stack.clear();
            vecs.args.clear();
            vecs.saved_macros.clear();
            self.frame_pool.push(vecs);
        }
    }

    /// An empty vector for the positional arguments of a call.
    pub(crate) fn arg_vec(&mut self) -> Vec<Value> {
        self.arg_pool.pop().unwrap_or_default()
    }

    /// Takes back the argument vector of a finished call (its values are dropped).
    pub(crate) fn recycle_arg_vec(&mut self, mut args: Vec<Value>) {
        if args.capacity() != 0
            && args.capacity() <= ARG_POOL_CAPACITY
            && self.arg_pool.len() < ARG_POOL_SIZE
        {
            args.clear();
            self.arg_pool.push(args);
        }
    }

    /// The environment's escaper runtime (Twig's `getRuntime(EscaperRuntime::class)`), taken out of the render's
    /// cache: give it back with [`restore_escaper_runtime`](Self::restore_escaper_runtime).
    pub(crate) fn take_escaper_runtime(&mut self) -> Arc<EscaperRuntime> {
        match self.escaper.take() {
            Some(runtime) => runtime,
            None => self.env.escaper_runtime(),
        }
    }

    /// Puts the escaper runtime back into the render's cache.
    pub(crate) fn restore_escaper_runtime(&mut self, runtime: Arc<EscaperRuntime>) {
        if self.escaper.is_none() {
            self.escaper = Some(runtime);
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

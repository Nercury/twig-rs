//! The request-level state of `ext/date`: clock, default timezone, zone cache and the last parse errors.
//!
//! PHP keeps this state in module globals (`DATEG(timezone)`, `DATEG(tzcache)`, `DATEG(last_errors)`) and reads
//! the clock with `gettimeofday()`. Here it lives in a [`DateContext`] that the embedding code owns (a Twig
//! environment holds one), so nothing is hidden in process globals and "now" can be made deterministic with a
//! [`FixedClock`].

use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use twig_timelib::tz::timezone_id_is_valid;
use twig_timelib::{ErrorContainer, TzCache, TzDb, TzInfo};
use twig_value::{Diagnostics, PhpError, PhpResult, PhpString};

use crate::errors;

/// The source of "now": PHP's `gettimeofday()` (`php_time()`,
/// `php_date_get_current_time_with_fraction()`).
pub trait Clock: Send + Sync {
    /// Whole seconds since the Unix epoch and the microseconds (`0..=999_999`) of the current second.
    fn now(&self) -> (i64, i64);
}

/// The system clock (`gettimeofday()`).
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> (i64, i64) {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => (d.as_secs() as i64, i64::from(d.subsec_micros())),
            Err(e) => {
                // Before 1970: `gettimeofday()` keeps tv_usec in 0..=999999 and floors tv_sec.
                let d = e.duration();
                let secs = -(d.as_secs() as i64);
                let us = i64::from(d.subsec_micros());
                if us == 0 {
                    (secs, 0)
                } else {
                    (secs - 1, 1_000_000 - us)
                }
            }
        }
    }
}

/// A clock stopped at a settable instant, for deterministic "now".
#[derive(Debug, Default)]
pub struct FixedClock {
    sec: AtomicI64,
    usec: AtomicI64,
}

impl FixedClock {
    /// A clock that always returns `(sec, usec)`.
    pub fn new(sec: i64, usec: i64) -> Self {
        FixedClock {
            sec: AtomicI64::new(sec),
            usec: AtomicI64::new(usec),
        }
    }

    /// Moves the clock to `(sec, usec)`.
    pub fn set(&self, sec: i64, usec: i64) {
        self.sec.store(sec, Ordering::SeqCst);
        self.usec.store(usec, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> (i64, i64) {
        (
            self.sec.load(Ordering::SeqCst),
            self.usec.load(Ordering::SeqCst),
        )
    }
}

impl<C: Clock + ?Sized> Clock for Arc<C> {
    fn now(&self) -> (i64, i64) {
        (**self).now()
    }
}

struct Inner {
    clock: RwLock<Arc<dyn Clock>>,
    /// The `date.timezone` INI setting (`DATEG(default_timezone)`); PHP's built-in default is `"UTC"`.
    ini_timezone: RwLock<String>,
    /// `DATEG(timezone)`: the zone set with `date_default_timezone_set()`.
    timezone: RwLock<Option<String>>,
    /// `DATEG(last_errors)`.
    last_errors: Mutex<Option<ErrorContainer>>,
    /// `DATEG(tzcache)` in front of the timezone database.
    tz_cache: &'static TzCache,
}

/// The state PHP keeps in `ext/date` module globals, owned by the embedding code.
///
/// Cloning is cheap and shares the state (date objects keep a clone of the context that created them, which is
/// where static methods such as `DateTime::getLastErrors()` called through an instance find it).
#[derive(Clone)]
pub struct DateContext(Arc<Inner>);

impl fmt::Debug for DateContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DateContext")
            .field("ini_timezone", &self.ini_timezone())
            .field("timezone", &*read(&self.0.timezone))
            .finish_non_exhaustive()
    }
}

impl Default for DateContext {
    fn default() -> Self {
        DateContext::new()
    }
}

fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The bytes of a C string: up to the first NUL.
pub(crate) fn c_str(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

impl DateContext {
    /// A context with the system clock and PHP's defaults (`date.timezone=UTC`, no default timezone set).
    pub fn new() -> Self {
        DateContext::with_clock(SystemClock)
    }

    /// A context reading "now" from `clock`.
    pub fn with_clock(clock: impl Clock + 'static) -> Self {
        DateContext(Arc::new(Inner {
            clock: RwLock::new(Arc::new(clock)),
            ini_timezone: RwLock::new("UTC".to_owned()),
            timezone: RwLock::new(None),
            last_errors: Mutex::new(None),
            tz_cache: TzCache::builtin(),
        }))
    }

    /// Replaces the clock.
    pub fn set_clock(&self, clock: impl Clock + 'static) {
        *write(&self.0.clock) = Arc::new(clock);
    }

    /// The clock.
    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&read(&self.0.clock))
    }

    /// Port of `php_date_get_current_time_with_fraction()` (PHPSRC/ext/date/php_date.c): seconds and microseconds.
    pub fn current_time(&self) -> (i64, i64) {
        self.clock().now()
    }

    /// Port of `php_time()` (PHPSRC/ext/date/php_date.c): `time()`.
    pub fn time(&self) -> i64 {
        self.current_time().0
    }

    /// The zone cache (PHP's `DATEG(tzcache)`; zone data is immutable, so the process-wide cache of
    /// [`TzCache::builtin`] is shared).
    pub fn tz_cache(&self) -> &'static TzCache {
        self.0.tz_cache
    }

    /// The timezone database (`DATE_TIMEZONEDB`).
    pub fn tzdb(&self) -> &'static TzDb {
        self.0.tz_cache.tzdb()
    }

    /// Port of `php_date_parse_tzfile()` (PHPSRC/ext/date/php_date.c): zone data for an identifier (a C string,
    /// matched case-insensitively; the zone keeps the name as written).
    pub fn parse_tzfile(&self, name: &[u8]) -> Option<Arc<TzInfo>> {
        let name = std::str::from_utf8(c_str(name)).ok()?;
        self.0.tz_cache.get(name)
    }

    /// The `date.timezone` INI value.
    pub fn ini_timezone(&self) -> String {
        read(&self.0.ini_timezone).clone()
    }

    /// Sets the `date.timezone` INI value like `OnUpdate_date_timezone()` (PHPSRC/ext/date/php_date.c): an
    /// identifier unknown to the database (or empty) is rejected and `false` is returned; PHP additionally warns
    /// `Invalid date.timezone value '%s', using '%s' instead` from the INI handler.
    pub fn set_ini_timezone(&self, zone: &str) -> bool {
        if !timezone_id_is_valid(zone.as_bytes(), self.tzdb()) {
            return false;
        }
        *write(&self.0.ini_timezone) = zone.to_owned();
        true
    }

    /// Port of `guess_timezone()` (PHPSRC/ext/date/php_date.c): the zone set by `date_default_timezone_set()`,
    /// else the `date.timezone` INI value, else `"UTC"`.
    pub fn guess_timezone(&self) -> String {
        // Checking whether timezone has been set with date_default_timezone_set()
        if let Some(tz) = read(&self.0.timezone).as_ref()
            && !tz.is_empty()
        {
            return tz.clone();
        }
        // Check config setting for default timezone
        let ini = read(&self.0.ini_timezone);
        if !ini.is_empty() {
            return ini.clone();
        }
        // Fallback to UTC
        "UTC".to_owned()
    }

    /// Port of `get_timezone_info()` (PHPSRC/ext/date/php_date.c): the default timezone's data.
    pub fn timezone_info(&self) -> PhpResult<Arc<TzInfo>> {
        let tz = self.guess_timezone();
        self.parse_tzfile(tz.as_bytes()).ok_or_else(|| {
            PhpError::new(
                errors::date_error(),
                "Timezone database is corrupt. Please file a bug report as this should never happen",
            )
        })
    }

    /// Port of `PHP_FUNCTION(date_default_timezone_get)` (PHPSRC/ext/date/php_date.c): the default timezone's
    /// name, as it was set.
    pub fn default_timezone_get(&self) -> PhpResult<String> {
        Ok(self.timezone_info()?.name.clone())
    }

    /// Port of `PHP_FUNCTION(date_default_timezone_set)` (PHPSRC/ext/date/php_date.c): validates the identifier
    /// (a C string) against the database; an unknown one raises the notice
    /// `date_default_timezone_set(): Timezone ID '%s' is invalid` and returns `false`.
    pub fn default_timezone_set(&self, zone: &[u8], diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        let zone = c_str(zone);
        if !timezone_id_is_valid(zone, self.tzdb()) {
            diag.notice(PhpString::concat(&[
                b"date_default_timezone_set(): Timezone ID '",
                zone,
                b"' is invalid",
            ]))?;
            return Ok(false);
        }
        // Valid identifiers are ASCII.
        *write(&self.0.timezone) = Some(String::from_utf8_lossy(zone).into_owned());
        Ok(true)
    }

    /// `DATEG(last_errors)`: the warnings and errors of the last `DateTime` parse, when there were any.
    pub fn last_errors(&self) -> Option<ErrorContainer> {
        lock(&self.0.last_errors).clone()
    }

    /// Port of `update_errors_warnings()` (PHPSRC/ext/date/php_date.c): keeps `errors` when it holds warnings or
    /// errors, else clears the last errors.
    pub(crate) fn update_errors_warnings(&self, errors: Option<ErrorContainer>) {
        let mut last = lock(&self.0.last_errors);
        *last = errors.filter(|e| e.warning_count() > 0 || e.error_count() > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timezone() {
        let ctx = DateContext::with_clock(FixedClock::new(1, 2));
        assert_eq!(ctx.current_time(), (1, 2));
        assert_eq!(ctx.default_timezone_get().unwrap(), "UTC");
        let mut diags = Vec::new();
        assert!(
            ctx.default_timezone_set(b"europe/paris", &mut diags)
                .unwrap()
        );
        assert_eq!(ctx.default_timezone_get().unwrap(), "europe/paris");
        assert!(
            !ctx.default_timezone_set(b"Nowhere/City", &mut diags)
                .unwrap()
        );
        assert_eq!(
            diags[0].message,
            "date_default_timezone_set(): Timezone ID 'Nowhere/City' is invalid"
        );
        assert_eq!(ctx.default_timezone_get().unwrap(), "europe/paris");
        assert!(!ctx.set_ini_timezone("Nowhere/City"));
        assert!(ctx.set_ini_timezone("Asia/Tokyo"));
        assert_eq!(ctx.ini_timezone(), "Asia/Tokyo");
    }

    #[test]
    fn system_clock_is_sane() {
        let (s, us) = SystemClock.now();
        assert!(s > 1_700_000_000);
        assert!((0..1_000_000).contains(&us));
    }
}

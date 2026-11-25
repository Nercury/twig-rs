//! Broken-down time to Unix timestamp: normalisation, relative arithmetic and zone adjustment (port of
//! `PHPSRC/ext/date/lib/tm2unixtime.c`).

use std::sync::Arc;

use crate::dow::day_of_week;
use crate::timelib::hms_to_seconds;
use crate::types::{
    DAYS_PER_ERA, DAYS_PER_YEAR, HINNANT_EPOCH_SHIFT, RelTime, SECS_PER_DAY, SECS_PER_HOUR,
    SPECIAL_DAY_OF_WEEK_IN_MONTH, SPECIAL_FIRST_DAY_OF_MONTH, SPECIAL_LAST_DAY_OF_MONTH,
    SPECIAL_LAST_DAY_OF_WEEK_IN_MONTH, SPECIAL_WEEKDAY, Special, Time, UNSET, YEARS_PER_ERA,
    ZONETYPE_ABBR, ZONETYPE_ID, ZONETYPE_OFFSET, is_leap,
};
use crate::tz::TzInfo;
use crate::unixtime2tm::date_from_epoch_days;

//                                    dec  jan  feb  mrt  apr  may  jun  jul  aug  sep  oct  nov  dec
const DAYS_IN_MONTH_LEAP: [i64; 13] = [31, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const DAYS_IN_MONTH: [i64; 13] = [31, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// `do_range_limit()` (PHPSRC/ext/date/lib/tm2unixtime.c): brings `a` into `[start, end)`, carrying into `b`.
pub(crate) fn do_range_limit(start: i64, end: i64, adj: i64, a: &mut i64, b: &mut i64) {
    if *a < start {
        // We calculate 'a + 1' first as 'start - *a - 1' causes an int64_t overflows if *a is LONG_MIN. 'start'
        // is 0 in this context, and '0 - LONG_MIN > LONG_MAX'.
        let a_plus_1 = a.wrapping_add(1);

        *b = b.wrapping_sub(start.wrapping_sub(a_plus_1) / adj + 1);

        // This code add the extra 'adj' separately, as otherwise this can overflow int64_t in situations where
        // *b is near LONG_MIN.
        *a = a.wrapping_add(adj.wrapping_mul(start.wrapping_sub(a_plus_1) / adj));
        *a = a.wrapping_add(adj);
    }
    if *a >= end {
        *b = b.wrapping_add(*a / adj);
        *a = a.wrapping_sub(adj.wrapping_mul(*a / adj));
    }
}

fn inc_month(y: &mut i64, m: &mut i64) {
    *m += 1;
    if *m > 12 {
        *m -= 12;
        *y = y.wrapping_add(1);
    }
}

fn dec_month(y: &mut i64, m: &mut i64) {
    *m -= 1;
    if *m < 1 {
        *m += 12;
        *y = y.wrapping_sub(1);
    }
}

fn month_len(leap: bool, month: i64) -> i64 {
    let idx = month.clamp(0, 12) as usize;
    if leap {
        DAYS_IN_MONTH_LEAP[idx]
    } else {
        DAYS_IN_MONTH[idx]
    }
}

/// `do_range_limit_days_relative()` (PHPSRC/ext/date/lib/tm2unixtime.c). Normalises the base month in place.
fn do_range_limit_days_relative(
    base_y: &mut i64,
    base_m: &mut i64,
    m: &mut i64,
    d: &mut i64,
    invert: i64,
) {
    do_range_limit(1, 13, 12, base_m, base_y);

    let mut year = *base_y;
    let mut month = *base_m;

    if invert == 0 {
        while *d < 0 {
            dec_month(&mut year, &mut month);
            let days = month_len(is_leap(year), month);
            *d += days;
            *m = m.wrapping_sub(1);
        }
    } else {
        while *d < 0 {
            let days = month_len(is_leap(year), month);
            *d += days;
            *m = m.wrapping_sub(1);
            inc_month(&mut year, &mut month);
        }
    }
}

/// `do_range_limit_days()` (PHPSRC/ext/date/lib/tm2unixtime.c): one normalisation step of the day field.
fn do_range_limit_days(y: &mut i64, m: &mut i64, d: &mut i64) -> bool {
    let mut retval = false;

    // can jump an entire leap year period quickly
    if *d >= DAYS_PER_ERA || *d <= -DAYS_PER_ERA {
        *y = y.wrapping_add(YEARS_PER_ERA.wrapping_mul(*d / DAYS_PER_ERA));
        *d -= DAYS_PER_ERA * (*d / DAYS_PER_ERA);
    }

    do_range_limit(1, 13, 12, m, y);

    // The month table is chosen once, for the starting year (as in timelib).
    let current_year_leap = is_leap(*y);

    while *d <= 0 && *m > 0 {
        let mut previous_month = *m - 1;
        let previous_year = if previous_month < 1 {
            previous_month += 12;
            y.wrapping_sub(1)
        } else {
            *y
        };
        let days_in_previous_month = month_len(is_leap(previous_year), previous_month);

        *d += days_in_previous_month;
        *m -= 1;
        retval = true;
    }
    while *d > 0 && *m <= 12 && *d > month_len(current_year_leap, *m) {
        *d -= month_len(current_year_leap, *m);
        *m += 1;
        retval = true;
    }
    retval
}

/// `do_adjust_for_weekday()` (PHPSRC/ext/date/lib/tm2unixtime.c).
fn do_adjust_for_weekday(time: &mut Time) {
    let current_dow = day_of_week(time.y, time.m, time.d);
    if time.relative.weekday_behavior == 2 {
        // To make "this week" work, where the current DOW is a "sunday"
        if current_dow == 0 && time.relative.weekday != 0 {
            time.relative.weekday = time.relative.weekday.wrapping_sub(7);
        }

        // To make "sunday this week" work, where the current DOW is not a "sunday"
        if time.relative.weekday == 0 && current_dow != 0 {
            time.relative.weekday = 7;
        }

        time.d = time.d.wrapping_sub(current_dow);
        time.d = time.d.wrapping_add(i64::from(time.relative.weekday));
        return;
    }
    let mut difference = i64::from(time.relative.weekday).wrapping_sub(current_dow);
    if (time.relative.d < 0 && difference < 0)
        || (time.relative.d >= 0 && difference <= -i64::from(time.relative.weekday_behavior))
    {
        difference = difference.wrapping_add(7);
    }
    if time.relative.weekday >= 0 {
        time.d = time.d.wrapping_add(difference);
    } else {
        // `abs()` of an `int`, then `int` - `timelib_sll` arithmetic.
        time.d = time
            .d
            .wrapping_sub(7 - (i64::from(time.relative.weekday.wrapping_abs()) - current_dow));
    }
    time.relative.have_weekday_relative = 0;
}

/// Port of `timelib_do_rel_normalize()` (PHPSRC/ext/date/lib/tm2unixtime.c): normalises a relative time
/// against `base` (whose month is normalised in place, as in C).
pub fn do_rel_normalize(base: &mut Time, rt: &mut RelTime) {
    do_range_limit(0, 1_000_000, 1_000_000, &mut rt.us, &mut rt.s);
    do_range_limit(0, 60, 60, &mut rt.s, &mut rt.i);
    do_range_limit(0, 60, 60, &mut rt.i, &mut rt.h);
    do_range_limit(0, 24, 24, &mut rt.h, &mut rt.d);
    do_range_limit(0, 12, 12, &mut rt.m, &mut rt.y);

    do_range_limit_days_relative(
        &mut base.y,
        &mut base.m,
        &mut rt.m,
        &mut rt.d,
        i64::from(rt.invert),
    );
    do_range_limit(0, 12, 12, &mut rt.m, &mut rt.y);
}

impl Time {
    /// Port of `timelib_do_normalize()` (PHPSRC/ext/date/lib/tm2unixtime.c): carries out-of-range fields into
    /// the next larger unit, taking leap years into account.
    pub fn do_normalize(&mut self) {
        if self.us != UNSET {
            do_range_limit(0, 1_000_000, 1_000_000, &mut self.us, &mut self.s);
        }
        if self.s != UNSET {
            do_range_limit(0, 60, 60, &mut self.s, &mut self.i);
        }
        if self.s != UNSET {
            do_range_limit(0, 60, 60, &mut self.i, &mut self.h);
        }
        if self.s != UNSET {
            do_range_limit(0, 24, 24, &mut self.h, &mut self.d);
        }
        do_range_limit(1, 13, 12, &mut self.m, &mut self.y);

        // Short cut if we're doing things against the Epoch
        if self.y == 1970 && self.m == 1 {
            let (y, m, d) = date_from_epoch_days(self.d.wrapping_sub(1));
            self.y = y;
            self.m = m;
            self.d = d;
            return;
        }

        while do_range_limit_days(&mut self.y, &mut self.m, &mut self.d) {}
        do_range_limit(1, 13, 12, &mut self.m, &mut self.y);
    }

    /// `do_adjust_relative()` (PHPSRC/ext/date/lib/tm2unixtime.c).
    fn do_adjust_relative(&mut self) {
        if self.relative.have_weekday_relative != 0 {
            do_adjust_for_weekday(self);
        }
        self.do_normalize();

        if self.have_relative != 0 {
            self.us = self.us.wrapping_add(self.relative.us);

            self.s = self.s.wrapping_add(self.relative.s);
            self.i = self.i.wrapping_add(self.relative.i);
            self.h = self.h.wrapping_add(self.relative.h);

            self.d = self.d.wrapping_add(self.relative.d);
            self.m = self.m.wrapping_add(self.relative.m);
            self.y = self.y.wrapping_add(self.relative.y);
        }

        match self.relative.first_last_day_of {
            SPECIAL_FIRST_DAY_OF_MONTH => self.d = 1,
            SPECIAL_LAST_DAY_OF_MONTH => {
                self.d = 0;
                self.m = self.m.wrapping_add(1);
            }
            _ => {}
        }

        self.do_normalize();
    }

    /// `do_adjust_special_weekday()` (PHPSRC/ext/date/lib/tm2unixtime.c): "+N weekdays".
    fn do_adjust_special_weekday(&mut self) {
        let count = self.relative.special.amount;
        let dow = day_of_week(self.y, self.m, self.d);

        // Add increments of 5 weekdays as a week, leaving the DOW unchanged.
        self.d = self.d.wrapping_add((count / 5).wrapping_mul(7));

        // Deal with the remainder.
        let rem = count % 5;

        if count > 0 {
            if rem == 0 {
                // Head back to Friday if we stop on the weekend.
                if dow == 0 {
                    self.d = self.d.wrapping_sub(2);
                } else if dow == 6 {
                    self.d = self.d.wrapping_sub(1);
                }
            } else if dow == 6 {
                // We ended up on Saturday, but there's still work to do, so move to Sunday and continue from
                // there.
                self.d = self.d.wrapping_add(1);
            } else if dow + rem > 5 {
                // We're on a weekday, but we're going past Friday, so skip right over the weekend.
                self.d = self.d.wrapping_add(2);
            }
        } else {
            // Completely mirror the forward direction. This also covers the 0 case, since if we start on the
            // weekend, we want to move forward as if we stopped there while going backwards.
            if rem == 0 {
                if dow == 6 {
                    self.d = self.d.wrapping_add(2);
                } else if dow == 0 {
                    self.d = self.d.wrapping_add(1);
                }
            } else if dow == 0 {
                self.d = self.d.wrapping_sub(1);
            } else if dow + rem < 1 {
                self.d = self.d.wrapping_sub(2);
            }
        }

        self.d = self.d.wrapping_add(rem);
    }

    /// `do_adjust_special()` (PHPSRC/ext/date/lib/tm2unixtime.c).
    fn do_adjust_special(&mut self) {
        if self.relative.have_special_relative != 0 && self.relative.special.kind == SPECIAL_WEEKDAY
        {
            self.do_adjust_special_weekday();
        }
        self.do_normalize();
        self.relative.special = Special::default();
    }

    /// `do_adjust_special_early()` (PHPSRC/ext/date/lib/tm2unixtime.c).
    fn do_adjust_special_early(&mut self) {
        if self.relative.have_special_relative != 0 {
            match self.relative.special.kind {
                SPECIAL_DAY_OF_WEEK_IN_MONTH => {
                    self.d = 1;
                    self.m = self.m.wrapping_add(self.relative.m);
                    self.relative.m = 0;
                }
                SPECIAL_LAST_DAY_OF_WEEK_IN_MONTH => {
                    self.d = 1;
                    self.m = self.m.wrapping_add(self.relative.m).wrapping_add(1);
                    self.relative.m = 0;
                }
                _ => {}
            }
        }
        match self.relative.first_last_day_of {
            SPECIAL_FIRST_DAY_OF_MONTH => self.d = 1,
            SPECIAL_LAST_DAY_OF_MONTH => {
                self.d = 0;
                self.m = self.m.wrapping_add(1);
            }
            _ => {}
        }
        self.do_normalize();
    }

    /// `do_adjust_timezone()` (PHPSRC/ext/date/lib/tm2unixtime.c): turns the local `sse` into UTC.
    fn do_adjust_timezone(&mut self, tzi: Option<&Arc<TzInfo>>) {
        match self.zone_type {
            ZONETYPE_OFFSET => {
                self.is_localtime = 1;
                self.sse = self.sse.wrapping_add(-i64::from(self.z));
            }
            ZONETYPE_ABBR => {
                self.is_localtime = 1;
                // `-tz->z - tz->dst * SECS_PER_HOUR` is `int` arithmetic in C.
                let adjustment = self
                    .z
                    .wrapping_neg()
                    .wrapping_sub(self.dst.wrapping_mul(SECS_PER_HOUR as i32));
                self.sse = self.sse.wrapping_add(i64::from(adjustment));
            }
            zone_type => {
                // TIMELIB_ZONETYPE_ID falls through to the default branch with the struct's own zone.
                let tzi = if zone_type == ZONETYPE_ID {
                    self.tz_info.clone()
                } else {
                    tzi.cloned()
                };
                // No timezone in struct, fallback to reference if possible
                let Some(tzi) = tzi else { return };

                let (current_offset, current_transition_time, current_is_dst) =
                    tzi.get_time_zone_offset_info(self.sse).unwrap_or((0, 0, 0));
                let _ = current_transition_time;
                let (after_offset, after_transition_time, _) = tzi
                    .get_time_zone_offset_info(self.sse.wrapping_sub(i64::from(current_offset)))
                    .unwrap_or((0, 0, 0));
                let mut actual_offset = after_offset;
                let mut actual_transition_time = after_transition_time;
                if current_offset == after_offset && self.have_zone != 0 {
                    // Make sure we're not missing a DST change because we don't know the actual offset yet
                    if current_offset >= 0 && self.dst != 0 && current_is_dst == 0 {
                        // Timezone or its DST at or east of UTC, so the local time, interpreted as UTC, leaves
                        // DST (as defined in the actual timezone) before the actual local time
                        let (earlier_offset, earlier_transition_time, _) = tzi
                            .get_time_zone_offset_info(
                                self.sse
                                    .wrapping_sub(i64::from(current_offset))
                                    .wrapping_sub(7200),
                            )
                            .unwrap_or((0, 0, 0));
                        if earlier_offset != after_offset
                            && self.sse.wrapping_sub(i64::from(earlier_offset))
                                < after_transition_time
                        {
                            // Looking behind a bit clarified the actual offset to use
                            actual_offset = earlier_offset;
                            actual_transition_time = earlier_transition_time;
                        }
                    } else if current_offset <= 0 && current_is_dst != 0 && self.dst == 0 {
                        // Timezone west of UTC, so the local time, interpreted as UTC, leaves DST (as defined in
                        // the actual timezone) after the actual local time
                        let (later_offset, later_transition_time, _) = tzi
                            .get_time_zone_offset_info(
                                self.sse
                                    .wrapping_sub(i64::from(current_offset))
                                    .wrapping_add(7200),
                            )
                            .unwrap_or((0, 0, 0));
                        if later_offset != after_offset
                            && self.sse.wrapping_sub(i64::from(later_offset))
                                >= later_transition_time
                        {
                            // Looking ahead a bit clarified the actual offset to use
                            actual_offset = later_offset;
                            actual_transition_time = later_transition_time;
                        }
                    }
                }

                self.is_localtime = 1;

                let local = self.sse.wrapping_sub(i64::from(actual_offset));
                let in_transition = actual_transition_time != i64::MIN
                    && local
                        >= actual_transition_time
                            .wrapping_add(i64::from(current_offset) - i64::from(actual_offset))
                    && local < actual_transition_time;

                let adjustment = if current_offset != actual_offset && !in_transition {
                    -i64::from(actual_offset)
                } else {
                    -i64::from(current_offset)
                };

                self.sse = self.sse.wrapping_add(adjustment);
                self.set_timezone(&tzi);
            }
        }
    }

    /// Port of `timelib_epoch_days_from_time()` (PHPSRC/ext/date/lib/tm2unixtime.c): days since the epoch for
    /// the y/m/d fields.
    pub fn epoch_days_from_time(&self) -> i64 {
        let mut y = self.y; // Make copy, as we don't want to change the original one
        y = y.wrapping_sub(i64::from(self.m <= 2));
        let era = (if y >= 0 { y } else { y.wrapping_sub(399) }) / YEARS_PER_ERA;
        let year_of_era = y.wrapping_sub(era.wrapping_mul(YEARS_PER_ERA)); // [0, 399]
        let month_part = 153i64
            .wrapping_mul(self.m.wrapping_add(if self.m > 2 { -3 } else { 9 }))
            .wrapping_add(2)
            / 5;
        let day_of_year = month_part.wrapping_add(self.d.wrapping_sub(1)); // [0, 365]
        let day_of_era = (year_of_era * DAYS_PER_YEAR + year_of_era / 4 - year_of_era / 100)
            .wrapping_add(day_of_year); // [0, 146096]

        era.wrapping_mul(DAYS_PER_ERA)
            .wrapping_add(day_of_era)
            .wrapping_sub(HINNANT_EPOCH_SHIFT)
    }

    /// Port of `timelib_update_ts()` (PHPSRC/ext/date/lib/tm2unixtime.c): applies the pending relative part,
    /// normalises, and computes `sse` using the struct's zone (or `tzi` when the struct has none).
    pub fn update_ts(&mut self, tzi: Option<&Arc<TzInfo>>) {
        self.do_adjust_special_early();
        self.do_adjust_relative();
        self.do_adjust_special();

        // The time element is added first and then the day count twice as half a day, so that the earliest
        // representable day does not overflow int64_t.
        self.sse = hms_to_seconds(self.h, self.i, self.s);
        let days = self.epoch_days_from_time();
        self.sse = self.sse.wrapping_add(days.wrapping_mul(SECS_PER_DAY / 2));
        self.sse = self.sse.wrapping_add(days.wrapping_mul(SECS_PER_DAY / 2));

        // This modifies time->sse, if needed
        self.do_adjust_timezone(tzi);

        self.sse_uptodate = 1;
        self.have_relative = 0;
        self.relative.have_weekday_relative = 0;
        self.relative.have_special_relative = 0;
        self.relative.first_last_day_of = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_limit_extremes() {
        let (mut a, mut b) = (i64::MIN, 0);
        do_range_limit(0, 60, 60, &mut a, &mut b);
        assert!((0..60).contains(&a));
    }

    #[test]
    fn month_overflow() {
        let mut t = Time::new();
        (t.y, t.m, t.d) = (2021, 1, 31);
        t.relative.m = 1;
        t.have_relative = 1;
        t.update_ts(None);
        assert_eq!((t.y, t.m, t.d), (2021, 3, 3));
    }
}

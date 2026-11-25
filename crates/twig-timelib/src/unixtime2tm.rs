//! Unix timestamp to broken-down time, and zone assignment (port of `PHPSRC/ext/date/lib/unixtime2tm.c`).

use std::sync::Arc;

use crate::types::{
    AbbrInfo, DAYS_PER_ERA, DAYS_PER_YEAR, HINNANT_EPOCH_SHIFT, SECS_PER_DAY, Time, YEARS_PER_ERA,
    ZONETYPE_ABBR, ZONETYPE_ID, ZONETYPE_OFFSET,
};
use crate::tz::{TzInfo, get_time_zone_offset_info};

/// Port of `timelib_date_from_epoch_days()` (PHPSRC/ext/date/lib/unixtime2tm.c): days since 1970-01-01 to
/// `(y, m, d)` with Howard Hinnant's `civil_from_days` algorithm.
pub fn date_from_epoch_days(epoch_days: i64) -> (i64, i64, i64) {
    // Calculate days since algorithm's epoch (0000-03-01)
    let days = epoch_days.wrapping_add(HINNANT_EPOCH_SHIFT);

    let era = (if days >= 0 {
        days
    } else {
        days.wrapping_sub(DAYS_PER_ERA).wrapping_add(1)
    }) / DAYS_PER_ERA;
    let day_of_era = days.wrapping_sub(era.wrapping_mul(DAYS_PER_ERA)) as u64;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096)
        / DAYS_PER_YEAR as u64;
    let mut y = (year_of_era as i64).wrapping_add(era.wrapping_mul(YEARS_PER_ERA));
    let day_of_year =
        day_of_era - (DAYS_PER_YEAR as u64 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_portion = (5 * day_of_year + 2) / 153;
    let d = (day_of_year - (153 * month_portion + 2) / 5 + 1) as i64;
    let m = if month_portion < 10 {
        month_portion + 3
    } else {
        month_portion.wrapping_sub(9)
    } as i64;
    y = y.wrapping_add(i64::from(m <= 2));
    (y, m, d)
}

/// Port of `timelib_unixtime2date()` (PHPSRC/ext/date/lib/unixtime2tm.c): the proleptic Gregorian date of `ts`.
pub fn unixtime2date(ts: i64) -> (i64, i64, i64) {
    let mut epoch_days = ts / SECS_PER_DAY;
    // Adjustment for a negative time portion
    let t = ts % SECS_PER_DAY;
    epoch_days += if t < 0 { -1 } else { 0 };
    date_from_epoch_days(epoch_days)
}

impl Time {
    /// Port of `timelib_unixtime2gmt()` (PHPSRC/ext/date/lib/unixtime2tm.c): broken-down UTC time of `ts`.
    pub fn unixtime2gmt(&mut self, ts: i64) {
        let (y, m, d) = unixtime2date(ts);
        self.y = y;
        self.m = m;
        self.d = d;
        let mut remainder = ts % SECS_PER_DAY;
        remainder += i64::from(remainder < 0) * SECS_PER_DAY;

        // That was the date, now we do the time
        let hours = remainder / 3600;
        let minutes = (remainder - hours * 3600) / 60;
        let seconds = remainder % 60;

        self.h = hours;
        self.i = minutes;
        self.s = seconds;
        self.z = 0;
        self.dst = 0;
        self.sse = ts;
        self.sse_uptodate = 1;
        self.tim_uptodate = 1;
        self.is_localtime = 0;
    }

    /// Port of `timelib_update_from_sse()` (PHPSRC/ext/date/lib/unixtime2tm.c): recomputes the broken-down
    /// fields from `sse` in the attached zone, keeping `sse`, `z` and `dst`.
    pub fn update_from_sse(&mut self) {
        let z = self.z;
        let dst = self.dst;
        let sse = self.sse;

        match self.zone_type {
            ZONETYPE_ABBR | ZONETYPE_OFFSET => {
                // `tm->dst * 3600` is an `int` multiplication in C.
                let ts = self
                    .sse
                    .wrapping_add(i64::from(self.z))
                    .wrapping_add(i64::from(self.dst.wrapping_mul(3600)));
                self.unixtime2gmt(ts);
            }
            ZONETYPE_ID => {
                let offset = get_time_zone_offset_info(self.sse, self.tz_info.as_deref())
                    .map_or(0, |(o, _, _)| o);
                self.unixtime2gmt(self.sse.wrapping_add(i64::from(offset)));
            }
            _ => self.unixtime2gmt(self.sse),
        }

        self.sse = sse;
        self.is_localtime = 1;
        self.have_zone = 1;
        self.z = z;
        self.dst = dst;
    }

    /// Port of `timelib_unixtime2local()` (PHPSRC/ext/date/lib/unixtime2tm.c): broken-down local time of `ts`
    /// in the attached zone.
    pub fn unixtime2local(&mut self, ts: i64) {
        match (self.zone_type, self.tz_info.clone()) {
            (ZONETYPE_ABBR | ZONETYPE_OFFSET, _) => {
                let z = self.z;
                let dst = self.dst;
                self.unixtime2gmt(
                    ts.wrapping_add(i64::from(self.z))
                        .wrapping_add(i64::from(self.dst.wrapping_mul(3600))),
                );
                self.sse = ts;
                self.z = z;
                self.dst = dst;
            }
            (ZONETYPE_ID, Some(tz)) => {
                let gmt_offset = tz.get_time_zone_info(ts);
                self.unixtime2gmt(ts.wrapping_add(i64::from(gmt_offset.offset)));

                // we need to reset the sse here as unixtime2gmt modifies it
                self.sse = ts;
                self.dst = gmt_offset.is_dst as i32;
                self.z = gmt_offset.offset;
                self.tz_info = Some(tz);
                self.tz_abbr_update(&gmt_offset.abbr);
            }
            // A zone type ID without zone data would crash timelib; treated like "no zone".
            _ => {
                self.is_localtime = 0;
                self.have_zone = 0;
                return;
            }
        }
        self.is_localtime = 1;
        self.have_zone = 1;
    }

    /// Port of `timelib_set_timezone_from_offset()` (PHPSRC/ext/date/lib/unixtime2tm.c).
    pub fn set_timezone_from_offset(&mut self, utc_offset: i64) {
        self.tz_abbr = None;
        self.z = utc_offset as i32;
        self.have_zone = 1;
        self.zone_type = ZONETYPE_OFFSET;
        self.dst = 0;
        self.tz_info = None;
    }

    /// Port of `timelib_set_timezone_from_abbr()` (PHPSRC/ext/date/lib/unixtime2tm.c).
    pub fn set_timezone_from_abbr(&mut self, abbr_info: &AbbrInfo) {
        self.tz_abbr = Some(abbr_info.abbr.clone());
        self.z = abbr_info.utc_offset as i32;
        self.have_zone = 1;
        self.zone_type = ZONETYPE_ABBR;
        self.dst = abbr_info.dst;
        self.tz_info = None;
    }

    /// Port of `timelib_set_timezone()` (PHPSRC/ext/date/lib/unixtime2tm.c): attaches `tz` and sets `z`,
    /// `dst` and `tz_abbr` (not upper-cased) for the current `sse`.
    pub fn set_timezone(&mut self, tz: &Arc<TzInfo>) {
        let gmt_offset = tz.get_time_zone_info(self.sse);
        self.z = gmt_offset.offset;
        self.dst = gmt_offset.is_dst as i32;
        self.tz_info = Some(Arc::clone(tz));
        self.tz_abbr = Some(gmt_offset.abbr);
        self.have_zone = 1;
        self.zone_type = ZONETYPE_ID;
    }

    /// Port of `timelib_apply_localtime()` (PHPSRC/ext/date/lib/unixtime2tm.c): converts between local time and
    /// GMT. Returns -1 when local time is requested without zone data.
    pub fn apply_localtime(&mut self, localtime: bool) -> i32 {
        if localtime {
            // Converting from GMT time to local time
            if self.tz_info.is_none() {
                return -1;
            }
            self.unixtime2local(self.sse);
        } else {
            // Converting from local time to GMT time
            self.unixtime2gmt(self.sse);
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_days() {
        assert_eq!(date_from_epoch_days(0), (1970, 1, 1));
        assert_eq!(date_from_epoch_days(-1), (1969, 12, 31));
        assert_eq!(date_from_epoch_days(19_723), (2024, 1, 1));
        assert_eq!(unixtime2date(-1), (1969, 12, 31));
    }

    #[test]
    fn gmt() {
        let mut t = Time::new();
        t.unixtime2gmt(1_700_000_000);
        assert_eq!((t.y, t.m, t.d, t.h, t.i, t.s), (2023, 11, 14, 22, 13, 20));
        t.unixtime2gmt(-1);
        assert_eq!((t.y, t.m, t.d, t.h, t.i, t.s), (1969, 12, 31, 23, 59, 59));
    }
}

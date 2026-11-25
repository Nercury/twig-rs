//! Calendar arithmetic: day of week, day of year, ISO weeks (port of `PHPSRC/ext/date/lib/dow.c`).

use crate::types::is_leap;

const M_TABLE_COMMON: [i64; 13] = [-1, 0, 3, 3, 6, 1, 4, 6, 2, 5, 0, 3, 5];
const M_TABLE_LEAP: [i64; 13] = [-1, 6, 2, 3, 6, 1, 4, 6, 2, 5, 0, 3, 5];

//                                    jan  feb  mar  apr  may  jun  jul  aug  sep  oct  nov  dec
const D_TABLE_COMMON: [i64; 13] = [0, 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
const D_TABLE_LEAP: [i64; 13] = [0, 0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];
pub(crate) const ML_TABLE_COMMON: [i64; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
pub(crate) const ML_TABLE_LEAP: [i64; 13] = [0, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// Index into a 13-entry month table. timelib indexes with the raw month; out-of-range months are undefined
/// behaviour in C and never reach these functions from PHP (callers normalise first). We clamp to stay safe.
#[inline]
fn month_index(m: i64) -> usize {
    m.clamp(0, 12) as usize
}

fn positive_mod(x: i64, y: i64) -> i64 {
    let mut tmp = x % y;
    if tmp < 0 {
        tmp += y;
    }
    tmp
}

fn century_value(j: i64) -> i64 {
    6 - positive_mod(j, 4) * 2
}

/// Port of `timelib_day_of_week_ex()` (PHPSRC/ext/date/lib/dow.c).
fn day_of_week_ex(y: i64, m: i64, d: i64, iso: bool) -> i64 {
    // Only valid for the Gregorian calendar; timelib returns the "wrong" day for Julian dates, consistently.
    let c1 = century_value(positive_mod(y, 400) / 100);
    let y1 = positive_mod(y, 100);
    let m1 = if is_leap(y) {
        M_TABLE_LEAP[month_index(m)]
    } else {
        M_TABLE_COMMON[month_index(m)]
    };
    let mut dow = positive_mod(
        c1.wrapping_add(y1)
            .wrapping_add(m1)
            .wrapping_add(y1 / 4)
            .wrapping_add(d),
        7,
    );
    if iso && dow == 0 {
        dow = 7;
    }
    dow
}

/// Port of `timelib_day_of_week()` (PHPSRC/ext/date/lib/dow.c): 0 = Sunday .. 6 = Saturday.
pub fn day_of_week(y: i64, m: i64, d: i64) -> i64 {
    day_of_week_ex(y, m, d, false)
}

/// Port of `timelib_iso_day_of_week()` (PHPSRC/ext/date/lib/dow.c): 1 = Monday .. 7 = Sunday.
pub fn iso_day_of_week(y: i64, m: i64, d: i64) -> i64 {
    day_of_week_ex(y, m, d, true)
}

/// Port of `timelib_day_of_year()` (PHPSRC/ext/date/lib/dow.c): 0 = Jan 1st .. 364/365 = Dec 31st.
pub fn day_of_year(y: i64, m: i64, d: i64) -> i64 {
    let base = if is_leap(y) {
        D_TABLE_LEAP[month_index(m)]
    } else {
        D_TABLE_COMMON[month_index(m)]
    };
    base.wrapping_add(d).wrapping_sub(1)
}

/// Port of `timelib_days_in_month()` (PHPSRC/ext/date/lib/dow.c): 28..31.
pub fn days_in_month(y: i64, m: i64) -> i64 {
    if is_leap(y) {
        ML_TABLE_LEAP[month_index(m)]
    } else {
        ML_TABLE_COMMON[month_index(m)]
    }
}

/// Port of `timelib_isoweek_from_date()` (PHPSRC/ext/date/lib/dow.c): returns `(iw, iy)`.
pub fn isoweek_from_date(y: i64, m: i64, d: i64) -> (i64, i64) {
    // The C code keeps these in `int`.
    let y_leap = i32::from(is_leap(y));
    let prev_y_leap = is_leap(y.wrapping_sub(1));
    let mut doy = day_of_year(y, m, d).wrapping_add(1) as i32;
    if y_leap != 0 && m > 2 {
        doy = doy.wrapping_add(1);
    }
    let mut jan1weekday = day_of_week(y, 1, 1) as i32;
    let mut weekday = day_of_week(y, m, d) as i32;
    if weekday == 0 {
        weekday = 7;
    }
    if jan1weekday == 0 {
        jan1weekday = 7;
    }
    let mut iw: i64 = 0;
    let iy: i64;
    // Find if Y M D falls in YearNumber Y-1, WeekNumber 52 or 53
    if doy <= (8 - jan1weekday) && jan1weekday > 4 {
        iy = y.wrapping_sub(1);
        if jan1weekday == 5 || (jan1weekday == 6 && prev_y_leap) {
            iw = 53;
        } else {
            iw = 52;
        }
    } else {
        iy = y;
    }
    // 8. Find if Y M D falls in YearNumber Y+1, WeekNumber 1
    if iy == y {
        let i: i32 = if y_leap != 0 { 366 } else { 365 };
        if i.wrapping_sub(doy.wrapping_sub(y_leap)) < (4 - weekday) {
            return (1, y.wrapping_add(1));
        }
    }
    // 9. Find if Y M D falls in YearNumber Y, WeekNumber 1 through 53
    if iy == y {
        let j = doy.wrapping_add(7 - weekday).wrapping_add(jan1weekday - 1);
        iw = i64::from(j / 7);
        if jan1weekday > 4 {
            iw -= 1;
        }
    }
    (iw, iy)
}

/// Port of `timelib_isodate_from_date()` (PHPSRC/ext/date/lib/dow.c): returns `(iy, iw, id)`.
pub fn isodate_from_date(y: i64, m: i64, d: i64) -> (i64, i64, i64) {
    let (iw, iy) = isoweek_from_date(y, m, d);
    (iy, iw, day_of_week_ex(y, m, d, true))
}

/// Port of `timelib_daynr_from_weeknr()` (PHPSRC/ext/date/lib/dow.c): day of year for ISO year/week/day.
pub fn daynr_from_weeknr(iy: i64, iw: i64, id: i64) -> i64 {
    // Figure out the dayofweek for y-1-1
    let dow = day_of_week(iy, 1, 1);
    // then use that to figure out the offset for day 1 of week 1
    let day = -(if dow > 4 { dow - 7 } else { dow });
    // Add weeks and days
    day.wrapping_add(iw.wrapping_sub(1).wrapping_mul(7))
        .wrapping_add(id)
}

/// Port of `timelib_date_from_isodate()` (PHPSRC/ext/date/lib/dow.c): returns `(y, m, d)`.
pub fn date_from_isodate(iy: i64, iw: i64, id: i64) -> (i64, i64, i64) {
    let mut daynr = daynr_from_weeknr(iy, iw, id).wrapping_add(1);
    let mut y = iy;
    let mut is_leap_year = is_leap(y);

    // Establish invariant that daynr >= 0
    while daynr <= 0 {
        y = y.wrapping_sub(1);
        is_leap_year = is_leap(y);
        daynr += if is_leap_year { 366 } else { 365 };
    }

    // Establish invariant that daynr <= number of days in *yr
    while daynr > if is_leap_year { 366 } else { 365 } {
        daynr -= if is_leap_year { 366 } else { 365 };
        y = y.wrapping_add(1);
        is_leap_year = is_leap(y);
    }

    let table = if is_leap_year {
        &ML_TABLE_LEAP
    } else {
        &ML_TABLE_COMMON
    };

    // Establish invariant that daynr <= number of days in *m
    let mut m = 1;
    while daynr > table[m] {
        daynr -= table[m];
        m += 1;
    }

    (y, m as i64, daynr)
}

/// Port of `timelib_valid_time()` (PHPSRC/ext/date/lib/dow.c).
pub fn valid_time(h: i64, i: i64, s: i64) -> bool {
    (0..=23).contains(&h) && (0..=59).contains(&i) && (0..=59).contains(&s)
}

/// Port of `timelib_valid_date()` (PHPSRC/ext/date/lib/dow.c).
pub fn valid_date(y: i64, m: i64, d: i64) -> bool {
    (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_examples() {
        // From the `#if 0` main() at the end of dow.c.
        assert_eq!(day_of_week(1978, 12, 22), 5);
        assert_eq!(day_of_week(2005, 2, 19), 6);
    }

    #[test]
    fn iso_weeks() {
        assert_eq!(isoweek_from_date(2024, 12, 30), (1, 2025));
        assert_eq!(isoweek_from_date(2021, 1, 3), (53, 2020));
        assert_eq!(date_from_isodate(2020, 53, 7), (2021, 1, 3));
    }
}

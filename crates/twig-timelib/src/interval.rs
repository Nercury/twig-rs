//! Differences between times and interval arithmetic (port of `PHPSRC/ext/date/lib/interval.c`).

use crate::timelib::{hms_to_seconds, hmsf_to_decimal_hour};
use crate::tm2unixtime::do_rel_normalize;
use crate::types::{
    RelTime, SECS_PER_DAY, SECS_PER_HOUR, Time, ZONETYPE_ABBR, ZONETYPE_ID, ZONETYPE_OFFSET,
};

fn same_tzid(one: &Time, two: &Time) -> bool {
    one.zone_type == ZONETYPE_ID
        && two.zone_type == ZONETYPE_ID
        && match (&one.tz_info, &two.tz_info) {
            (Some(a), Some(b)) => a.name == b.name,
            _ => false,
        }
}

/// `sort_old_to_new()` (PHPSRC/ext/date/lib/interval.c): returns `true` when the times must be swapped (and
/// the result inverted).
fn needs_swap(one: &Time, two: &Time) -> bool {
    // Check whether date/times need to be inverted. If both times are TIMELIB_ZONETYPE_ID times with the same
    // TZID, we use the y-s + us fields.
    if same_tzid(one, two) {
        return (one.y, one.m, one.d, one.h, one.i, one.s, one.us)
            > (two.y, two.m, two.d, two.h, two.i, two.s, two.us);
    }
    // Fall back to using the SSE instead to rearrange
    one.sse > two.sse || (one.sse == two.sse && one.us > two.us)
}

/// `timelib_diff_with_tzid()` (PHPSRC/ext/date/lib/interval.c).
fn diff_with_tzid(one: &mut Time, two: &mut Time) -> RelTime {
    let mut rt = RelTime {
        invert: 0,
        ..RelTime::default()
    };

    let (one, two) = if needs_swap(one, two) {
        rt.invert = 1;
        (two, one)
    } else {
        (one, two)
    };

    // Calculate correction for UTC offset changes between first and second SSE
    let mut dst_corr = i64::from(two.z) - i64::from(one.z);
    let dst_h_corr = dst_corr / 3600;
    let dst_m_corr = (dst_corr % 3600) / 60;

    rt.y = two.y.wrapping_sub(one.y);
    rt.m = two.m.wrapping_sub(one.m);
    rt.d = two.d.wrapping_sub(one.d);
    rt.h = two.h.wrapping_sub(one.h);
    rt.i = two.i.wrapping_sub(one.i);
    rt.s = two.s.wrapping_sub(one.s);
    rt.us = two.us.wrapping_sub(one.us);

    rt.days = i64::from(diff_days(one, two));

    // Fall Back: Cater for transition period, where rt->invert is 0, but there are negative numbers
    if two.sse < one.sse {
        let flipped =
            rt.i.wrapping_mul(60)
                .wrapping_add(rt.s)
                .wrapping_sub(dst_corr)
                .wrapping_abs();
        rt.h = flipped / SECS_PER_HOUR;
        rt.i = (flipped - rt.h * SECS_PER_HOUR) / 60;
        rt.s = flipped % 60;

        rt.invert = 1 - rt.invert;
    }

    if rt.invert != 0 {
        do_rel_normalize(one, &mut rt);
    } else {
        do_rel_normalize(two, &mut rt);
    }

    if one.dst == 1 && two.dst == 0 {
        // Fall Back
        if two.tz_info.is_some()
            && (two.sse.wrapping_sub(one.sse).wrapping_add(dst_corr)) < SECS_PER_DAY
        {
            rt.h -= dst_h_corr;
            rt.i -= dst_m_corr;
        }
    } else if one.dst == 0 && two.dst == 1 {
        // Spring Forward
        if let Some(tz) = two.tz_info.as_deref() {
            let trans = tz.get_time_zone_offset_info(two.sse);
            if let Some((_, trans_transition_time, _)) = trans
                && !((one.sse.wrapping_add(SECS_PER_DAY) > trans_transition_time)
                    && (one.sse.wrapping_add(SECS_PER_DAY)
                        <= trans_transition_time.wrapping_add(dst_corr)))
                && two.sse >= trans_transition_time
                && (two.sse.wrapping_sub(one.sse).wrapping_add(dst_corr) % SECS_PER_DAY)
                    > two.sse.wrapping_sub(trans_transition_time)
            {
                rt.h -= dst_h_corr;
                rt.i -= dst_m_corr;
            }
        }
    } else if two.sse.wrapping_sub(one.sse) >= SECS_PER_DAY {
        // Check whether we're in the period to the next transition time
        if let Some((trans_offset, trans_transition_time, _)) = two
            .tz_info
            .as_deref()
            .and_then(|tz| tz.get_time_zone_offset_info(two.sse.wrapping_sub(i64::from(two.z))))
        {
            dst_corr = i64::from(one.z) - i64::from(trans_offset);

            if two.sse >= trans_transition_time.wrapping_sub(dst_corr)
                && two.sse < trans_transition_time
            {
                rt.d -= 1;
                rt.h = 24;
            }
        }
    }

    rt
}

/// Port of `timelib_diff()` (PHPSRC/ext/date/lib/interval.c): the relative time that takes `one` to `two`
/// (`DateTime::diff()`).
///
/// timelib normalises the month of the base time in place; the inputs here are cloned first, which is
/// indistinguishable for the normalised times PHP passes.
pub fn diff(one: &Time, two: &Time) -> RelTime {
    let mut one = one.clone();
    let mut two = two.clone();
    if same_tzid(&one, &two) {
        return diff_with_tzid(&mut one, &mut two);
    }

    let mut rt = RelTime {
        invert: 0,
        ..RelTime::default()
    };
    let (one, two) = if needs_swap(&one, &two) {
        rt.invert = 1;
        (&mut two, &mut one)
    } else {
        (&mut one, &mut two)
    };

    rt.y = two.y.wrapping_sub(one.y);
    rt.m = two.m.wrapping_sub(one.m);
    rt.d = two.d.wrapping_sub(one.d);
    rt.h = two.h.wrapping_sub(one.h);
    if one.zone_type != ZONETYPE_ID {
        rt.h = rt.h.wrapping_add(i64::from(one.dst));
    }
    if two.zone_type != ZONETYPE_ID {
        rt.h = rt.h.wrapping_sub(i64::from(two.dst));
    }
    rt.i = two.i.wrapping_sub(one.i);
    rt.s = two
        .s
        .wrapping_sub(one.s)
        .wrapping_sub(i64::from(two.z))
        .wrapping_add(i64::from(one.z));
    rt.us = two.us.wrapping_sub(one.us);

    rt.days = i64::from(diff_days(one, two));

    if rt.invert != 0 {
        do_rel_normalize(one, &mut rt);
    } else {
        do_rel_normalize(two, &mut rt);
    }
    rt
}

/// Port of `timelib_same_timezone()` (PHPSRC/ext/date/lib/parse_tz.c).
pub fn same_timezone(one: &Time, two: &Time) -> bool {
    if one.zone_type != two.zone_type {
        return false;
    }
    if one.zone_type == ZONETYPE_ABBR || one.zone_type == ZONETYPE_OFFSET {
        // `int` arithmetic in C.
        return one.z.wrapping_add(one.dst.wrapping_mul(3600))
            == two.z.wrapping_add(two.dst.wrapping_mul(3600));
    }
    same_tzid(one, two)
}

/// Port of `timelib_diff_days()` (PHPSRC/ext/date/lib/interval.c): full days between the two times.
pub fn diff_days(one: &Time, two: &Time) -> i32 {
    if same_timezone(one, two) {
        let (earliest, latest) = if one.compare(two) < 0 {
            (one, two)
        } else {
            (two, one)
        };
        let earliest_time = hmsf_to_decimal_hour(
            earliest.h as i32,
            earliest.i as i32,
            earliest.s as i32,
            earliest.us as i32,
        );
        let latest_time = hmsf_to_decimal_hour(
            latest.h as i32,
            latest.i as i32,
            latest.s as i32,
            latest.us as i32,
        );

        // `days = llabs(...)` assigns a 64-bit value to an `int`.
        let mut d = one
            .epoch_days_from_time()
            .wrapping_sub(two.epoch_days_from_time())
            .wrapping_abs() as i32;
        if latest_time < earliest_time && d > 0 {
            d -= 1;
        }
        d
    } else {
        // `days = fabs(floor(one->sse - two->sse) / 86400)`: a double converted to `int` (x86 yields INT_MIN for
        // values out of range).
        let v = ((one.sse.wrapping_sub(two.sse)) as f64).floor() / 86400.0;
        let v = v.abs();
        if v < 2_147_483_648.0 {
            v as i32
        } else {
            i32::MIN
        }
    }
}

/// Port of `timelib_add()` (PHPSRC/ext/date/lib/interval.c): `DateTime::add()` for "civil" intervals.
pub fn add(old_time: &Time, interval: &RelTime) -> Time {
    let mut t = old_time.clone();

    if interval.have_weekday_relative != 0 || interval.have_special_relative != 0 {
        t.relative = *interval;
    } else {
        let bias = if interval.invert != 0 { -1 } else { 1 };
        t.relative = RelTime::default();
        t.relative.y = interval.y.wrapping_mul(bias);
        t.relative.m = interval.m.wrapping_mul(bias);
        t.relative.d = interval.d.wrapping_mul(bias);
        t.relative.h = interval.h.wrapping_mul(bias);
        t.relative.i = interval.i.wrapping_mul(bias);
        t.relative.s = interval.s.wrapping_mul(bias);
        t.relative.us = interval.us.wrapping_mul(bias);
    }
    t.have_relative = 1;
    t.sse_uptodate = 0;

    t.update_ts(None);

    t.update_from_sse();
    t.have_relative = 0;

    t
}

/// Port of `timelib_sub()` (PHPSRC/ext/date/lib/interval.c): `DateTime::sub()` for "civil" intervals.
pub fn sub(old_time: &Time, interval: &RelTime) -> Time {
    let mut t = old_time.clone();
    let bias = if interval.invert != 0 { -1 } else { 1 };

    t.relative = RelTime::default();
    t.relative.y = 0i64.wrapping_sub(interval.y.wrapping_mul(bias));
    t.relative.m = 0i64.wrapping_sub(interval.m.wrapping_mul(bias));
    t.relative.d = 0i64.wrapping_sub(interval.d.wrapping_mul(bias));
    t.relative.h = 0i64.wrapping_sub(interval.h.wrapping_mul(bias));
    t.relative.i = 0i64.wrapping_sub(interval.i.wrapping_mul(bias));
    t.relative.s = 0i64.wrapping_sub(interval.s.wrapping_mul(bias));
    t.relative.us = 0i64.wrapping_sub(interval.us.wrapping_mul(bias));
    t.have_relative = 1;
    t.sse_uptodate = 0;

    t.update_ts(None);

    t.update_from_sse();

    t.have_relative = 0;

    t
}

/// `do_range_limit()` as defined locally in PHPSRC/ext/date/lib/interval.c (differs from tm2unixtime.c's).
fn do_range_limit(start: i64, end: i64, adj: i64, a: &mut i64, b: &mut i64) {
    if *a < start {
        *b = b.wrapping_sub(start.wrapping_sub(*a).wrapping_sub(1) / adj + 1);
        *a = a.wrapping_add(adj.wrapping_mul(start.wrapping_sub(*a).wrapping_sub(1) / adj + 1));
    }
    if *a >= end {
        *b = b.wrapping_add(*a / adj);
        *a = a.wrapping_sub(adj.wrapping_mul(*a / adj));
    }
}

/// Port of `timelib_add_wall()` (PHPSRC/ext/date/lib/interval.c): `DateTime::add()` for "wall clock"
/// intervals (the time part is added to the timestamp).
pub fn add_wall(old_time: &Time, interval: &RelTime) -> Time {
    let mut t = old_time.clone();

    t.have_relative = 1;
    t.sse_uptodate = 0;

    if interval.have_weekday_relative != 0 || interval.have_special_relative != 0 {
        t.relative = *interval;

        t.update_ts(None);

        t.update_from_sse();
    } else {
        let bias: i64 = if interval.invert != 0 { -1 } else { 1 };
        t.relative = RelTime::default();
        t.relative.y = interval.y.wrapping_mul(bias);
        t.relative.m = interval.m.wrapping_mul(bias);
        t.relative.d = interval.d.wrapping_mul(bias);

        if t.relative.y != 0 || t.relative.m != 0 || t.relative.d != 0 {
            t.update_ts(None);
        }

        if interval.us == 0 {
            t.sse = t.sse.wrapping_add(
                bias.wrapping_mul(hms_to_seconds(interval.h, interval.i, interval.s)),
            );
            t.update_from_sse();
        } else {
            let mut temp_interval = *interval;

            do_range_limit(
                0,
                1_000_000,
                1_000_000,
                &mut temp_interval.us,
                &mut temp_interval.s,
            );
            t.sse = t.sse.wrapping_add(bias.wrapping_mul(hms_to_seconds(
                temp_interval.h,
                temp_interval.i,
                temp_interval.s,
            )));
            t.update_from_sse();
            t.us = t.us.wrapping_add(temp_interval.us.wrapping_mul(bias));

            t.do_normalize();
            t.update_ts(None);
        }
        t.do_normalize();
    }

    if t.zone_type == ZONETYPE_ID
        && let Some(tz) = t.tz_info.clone()
    {
        t.set_timezone(&tz);
    }
    t.have_relative = 0;

    t
}

/// Port of `timelib_sub_wall()` (PHPSRC/ext/date/lib/interval.c): `DateTime::sub()` for "wall clock"
/// intervals.
pub fn sub_wall(old_time: &Time, interval: &RelTime) -> Time {
    let mut t = old_time.clone();

    t.have_relative = 1;
    t.sse_uptodate = 0;

    if interval.have_weekday_relative != 0 || interval.have_special_relative != 0 {
        t.relative = *interval;

        t.update_ts(None);
        t.update_from_sse();
    } else {
        let bias: i64 = if interval.invert != 0 { -1 } else { 1 };
        t.relative = RelTime::default();
        t.relative.y = 0i64.wrapping_sub(interval.y.wrapping_mul(bias));
        t.relative.m = 0i64.wrapping_sub(interval.m.wrapping_mul(bias));
        t.relative.d = 0i64.wrapping_sub(interval.d.wrapping_mul(bias));

        if t.relative.y != 0 || t.relative.m != 0 || t.relative.d != 0 {
            t.update_ts(None);
        }

        if interval.us == 0 {
            t.sse = t.sse.wrapping_sub(
                bias.wrapping_mul(hms_to_seconds(interval.h, interval.i, interval.s)),
            );
            t.update_from_sse();
        } else {
            let mut temp_interval = *interval;

            do_range_limit(
                0,
                1_000_000,
                1_000_000,
                &mut temp_interval.us,
                &mut temp_interval.s,
            );
            t.sse = t.sse.wrapping_sub(bias.wrapping_mul(hms_to_seconds(
                temp_interval.h,
                temp_interval.i,
                temp_interval.s,
            )));
            t.update_from_sse();
            t.us = t.us.wrapping_sub(temp_interval.us.wrapping_mul(bias));

            t.do_normalize();
            t.update_ts(None);
        }
        t.do_normalize();
    }

    if t.zone_type == ZONETYPE_ID
        && let Some(tz) = t.tz_info.clone()
    {
        t.set_timezone(&tz);
    }
    t.have_relative = 0;

    t
}

/*
 * Differential harness for the twig-timelib port: links PHP 8.5.13's timelib copy (PHPSRC/ext/date/lib) and prints a
 * canonical dump of what timelib computes, one line per input line.  The Rust side prints the same format
 * (crates/twig-timelib/examples/timelib_dump.rs), so outputs can be diffed byte for byte.
 *
 * Input lines: fields separated by one space; strings are hex encoded so any byte, NUL included, can be tested.
 * <zone> is a hex encoded DateTimeZone name, parsed like timezone_initialize() (php_date.c) does; <tz> is a plain
 * zone identifier.  Times "at <ts> in <zone>" are built like (new DateTime('@<ts>'))->setTimezone(<zone>).
 *
 *   parse   <hex input>                          timelib_strtotime() only
 *   init    <tz> <now_ts> <hex input>            php_date_initialize() with a fixed "now" (usec 0)
 *   modify  <tz> <base_ts> <hex input>           DateTime('@base') in zone <tz>, then php_date_modify()
 *   format  <hex format> <hex input>             timelib_parse_from_format()
 *   local   <tz> <ts>                            timelib_unixtime2local()
 *   fmtinit <tz> <now_ts> <now_us> <hex format> <hex input>
 *                                                php_date_initialize() with a format (createFromFormat())
 *   zone    <hex name>                           timezone_initialize(): timelib_parse_zone() + PHP's checks
 *   diff    <zone> <ts> <us> <zone> <ts> <us>    timelib_diff_days() and timelib_diff()
 *   add     <zone> <ts> <us> <op> rel <hex>      interval = relative part of timelib_strtotime(<hex>)
 *   add     <zone> <ts> <us> <op> f <y> <m> <d> <h> <i> <s> <us> <invert>
 *                                                <op>: add, sub, addwall, subwall (timelib_add() ...)
 *   mk      <zone> <ts> <op> <a> <b> <c> <d>     <op>: date (setDate), time (setTime), iso (setISODate),
 *                                                ts (setTimestamp); then timelib_get_current_offset()
 *   info    <tz> <ts>                            timelib_get_time_zone_info(), ..._offset_info(),
 *                                                timelib_timestamp_is_in_dst(), POSIX transitions around <ts>
 *   tzfile  <hex name>                           timelib_parse_tzfile() (every field), timelib_timezone_id_is_valid()
 *   dow     <y> <m> <d>                          dow.c, timelib_epoch_days_from_time(), unixtime2date(), ...
 *   abbr    <hex abbr> <offset> <isdst>          timelib_timezone_id_from_abbr()
 *   ival    <hex input>                          timelib_strtointerval()
 *   raw     <hex input>                          timelib_strtotime(), then update_ts(NULL), update_from_sse() and
 *                                                unixtime2local() on the raw result (no fill_holes)
 *   posix   <hex string> <year>                  timelib_parse_posix_str() + timelib_get_transitions_for_year()
 *   hms     <hex double bits> <h> <i> <s> <us>   decimal-hour helpers, timelib_hms_to_seconds(),
 *                                                timelib_get_error_message(<h>)
 *   formatx <prefix 0|1> <hex format> <hex input>
 *                                                timelib_parse_from_format_with_map() with a map that also has the
 *                                                ISO week/year and offset-in-minutes specifiers (V N o Z, W w
 *                                                unhandled) and, for prefix 1, prefix character '%'
 *
 * Build: see run.sh (gcc -O2 -fwrapv, like PHP's ext/date/config0.m4 builds timelib).
 */
#include "timelib.h"
#include "timelib_private.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#define MAXLINE 65536
#define MAXF 16

static timelib_tzinfo *cache_tz[1024];
static char *cache_name[1024];
static int cache_n = 0;

/* Mirrors php_date_parse_tzfile(): a per-name cache (case-sensitive key) in front of timelib_parse_tzfile(). */
static timelib_tzinfo *tz_get(const char *name, const timelib_tzdb *db, int *err)
{
	int i, dummy;
	timelib_tzinfo *tzi;
	(void) err;
	for (i = 0; i < cache_n; i++) {
		if (strcmp(cache_name[i], name) == 0) {
			return cache_tz[i];
		}
	}
	tzi = timelib_parse_tzfile(name, db, &dummy);
	if (tzi && cache_n < 1024) {
		cache_name[cache_n] = strdup(name);
		cache_tz[cache_n] = tzi;
		cache_n++;
	}
	return tzi;
}

static int hexval(int c)
{
	if (c >= '0' && c <= '9') return c - '0';
	if (c >= 'a' && c <= 'f') return c - 'a' + 10;
	if (c >= 'A' && c <= 'F') return c - 'A' + 10;
	return -1;
}

static size_t unhex(const char *in, char *out)
{
	size_t n = 0;
	if (!in) {
		out[0] = 0;
		return 0;
	}
	while (in[0] && in[1] && hexval(in[0]) >= 0 && hexval(in[1]) >= 0) {
		out[n++] = (char) (hexval(in[0]) * 16 + hexval(in[1]));
		in += 2;
	}
	out[n] = 0;
	return n;
}

static void put_str(const char *s)
{
	if (!s) {
		printf("-");
		return;
	}
	for (; *s; s++) {
		unsigned char c = (unsigned char) *s;
		if (c > 32 && c < 127 && c != '\\') {
			putchar(c);
		} else {
			printf("\\x%02x", c);
		}
	}
}

/* with_code = 0: parse_iso_intervals.re's add_error() leaves error_code uninitialised, print "?" instead. */
static void dump_errors_ex(timelib_error_container *e, int with_code)
{
	int i;
	printf(" | E%d", e ? e->error_count : 0);
	for (i = 0; e && i < e->error_count; i++) {
		if (with_code) {
			printf(" %d", e->error_messages[i].error_code);
		} else {
			printf(" ?");
		}
		printf("@%d:%02x:", e->error_messages[i].position, (unsigned char) e->error_messages[i].character);
		put_str(e->error_messages[i].message);
	}
	printf(" | W%d", e ? e->warning_count : 0);
	for (i = 0; e && i < e->warning_count; i++) {
		printf(" %d@%d:%02x:", e->warning_messages[i].error_code, e->warning_messages[i].position, (unsigned char) e->warning_messages[i].character);
		put_str(e->warning_messages[i].message);
	}
}

static void dump_errors(timelib_error_container *e)
{
	dump_errors_ex(e, 1);
}

static void dump_rel(timelib_rel_time *r)
{
	printf("ry=%lld rm=%lld rd=%lld rh=%lld ri=%lld rs=%lld rus=%lld wd=%d wb=%d fl=%d inv=%d days=%lld st=%u sa=%lld hwr=%u hsr=%u",
		r->y, r->m, r->d, r->h, r->i, r->s, r->us,
		r->weekday, r->weekday_behavior, r->first_last_day_of, r->invert,
		r->days, r->special.type, r->special.amount,
		r->have_weekday_relative, r->have_special_relative);
}

static void dump_time(timelib_time *t)
{
	printf("y=%lld m=%lld d=%lld h=%lld i=%lld s=%lld us=%lld z=%d dst=%d abbr=", t->y, t->m, t->d, t->h, t->i, t->s, t->us, t->z, t->dst);
	put_str(t->tz_abbr);
	printf(" tz=");
	put_str(t->tz_info ? t->tz_info->name : NULL);
	printf(" sse=%lld lt=%u zt=%u ht=%u hd=%u hz=%u hr=%u hw=%u",
		t->sse, t->is_localtime, t->zone_type, t->have_time, t->have_date, t->have_zone, t->have_relative, t->have_weeknr_day);
	printf(" | ");
	dump_rel(&t->relative);
}

/* php_date_initialize() for the non-format path, with a fixed current time (usec = 0). */
static void do_init(const char *tzname, long long now_ts, const char *input, size_t len)
{
	timelib_error_container *err = NULL;
	timelib_tzinfo *tzi;
	timelib_time *t, *now;
	int dummy;

	if (len == 0) {
		input = "now";
		len = 3;
	}
	t = timelib_strtotime(input, len, &err, timelib_builtin_db(), tz_get);
	if (err && err->error_count) {
		printf("ERR");
		dump_errors(err);
		printf("\n");
		timelib_time_dtor(t);
		timelib_error_container_dtor(err);
		return;
	}
	if (t->tz_info) {
		tzi = t->tz_info;
	} else {
		tzi = tz_get(tzname, timelib_builtin_db(), &dummy);
	}
	now = timelib_time_ctor();
	now->zone_type = TIMELIB_ZONETYPE_ID;
	now->tz_info = tzi;
	timelib_unixtime2local(now, (timelib_sll) now_ts);
	now->us = 0;
	if (len == 3 && timelib_strncasecmp(input, "now", 3) == 0) {
		timelib_time_dtor(t);
		t = now;
	} else {
		timelib_fill_holes(t, now, TIMELIB_NO_CLONE);
		timelib_update_ts(t, tzi);
		timelib_update_from_sse(t);
		t->have_relative = 0;
		timelib_time_dtor(now);
	}
	dump_time(t);
	dump_errors(err);
	printf("\n");
	timelib_time_dtor(t);
	timelib_error_container_dtor(err);
}

/* php_date_initialize() for createFromFormat(): timelib_parse_from_format() + fill_holes(OVERRIDE_TIME). */
static void do_fmtinit(const char *tzname, long long now_ts, long long now_us, const char *format, const char *input, size_t len)
{
	timelib_error_container *err = NULL;
	timelib_tzinfo *tzi;
	timelib_time *t, *now;
	int dummy;

	t = timelib_parse_from_format(format, input, len, &err, timelib_builtin_db(), tz_get);
	if (err && err->error_count) {
		printf("ERR");
		dump_errors(err);
		printf("\n");
		timelib_time_dtor(t);
		timelib_error_container_dtor(err);
		return;
	}
	if (t->tz_info) {
		tzi = t->tz_info;
	} else {
		tzi = tz_get(tzname, timelib_builtin_db(), &dummy);
	}
	now = timelib_time_ctor();
	now->zone_type = TIMELIB_ZONETYPE_ID;
	now->tz_info = tzi;
	timelib_unixtime2local(now, (timelib_sll) now_ts);
	now->us = now_us;
	timelib_fill_holes(t, now, TIMELIB_NO_CLONE | TIMELIB_OVERRIDE_TIME);
	timelib_update_ts(t, tzi);
	timelib_update_from_sse(t);
	t->have_relative = 0;
	timelib_time_dtor(now);
	dump_time(t);
	dump_errors(err);
	printf("\n");
	timelib_time_dtor(t);
	timelib_error_container_dtor(err);
}

/* new DateTime('@base') + setTimezone(tz) + php_date_modify(). */
static void do_modify(const char *tzname, long long base_ts, const char *input, size_t len)
{
	timelib_error_container *err = NULL;
	timelib_time *t, *tmp;
	int dummy;

	t = timelib_time_ctor();
	t->zone_type = TIMELIB_ZONETYPE_ID;
	t->tz_info = tz_get(tzname, timelib_builtin_db(), &dummy);
	timelib_unixtime2local(t, (timelib_sll) base_ts);
	t->us = 0;

	tmp = timelib_strtotime(input, len, &err, timelib_builtin_db(), tz_get);
	if (err && err->error_count) {
		printf("ERR");
		dump_errors(err);
		printf("\n");
		timelib_time_dtor(tmp);
		timelib_time_dtor(t);
		timelib_error_container_dtor(err);
		return;
	}
	memcpy(&t->relative, &tmp->relative, sizeof(timelib_rel_time));
	t->have_relative = tmp->have_relative;
	t->sse_uptodate = 0;
	if (tmp->y != TIMELIB_UNSET) t->y = tmp->y;
	if (tmp->m != TIMELIB_UNSET) t->m = tmp->m;
	if (tmp->d != TIMELIB_UNSET) t->d = tmp->d;
	if (tmp->h != TIMELIB_UNSET) {
		t->h = tmp->h;
		if (tmp->i != TIMELIB_UNSET) {
			t->i = tmp->i;
			if (tmp->s != TIMELIB_UNSET) {
				t->s = tmp->s;
			} else {
				t->s = 0;
			}
		} else {
			t->i = 0;
			t->s = 0;
		}
	}
	if (tmp->us != TIMELIB_UNSET) t->us = tmp->us;
	if (tmp->y == 1970 && tmp->m == 1 && tmp->d == 1 && tmp->h == 0 && tmp->i == 0 && tmp->s == 0 && tmp->us == 0 &&
		tmp->have_zone && tmp->zone_type == TIMELIB_ZONETYPE_OFFSET && tmp->z == 0 && tmp->dst == 0) {
		timelib_set_timezone_from_offset(t, 0);
	}
	timelib_time_dtor(tmp);
	timelib_update_ts(t, NULL);
	timelib_update_from_sse(t);
	t->have_relative = 0;
	memset(&t->relative, 0, sizeof(t->relative));
	dump_time(t);
	dump_errors(err);
	printf("\n");
	timelib_time_dtor(t);
	timelib_error_container_dtor(err);
}

/* A DateTimeZone object (php_timezone_obj). */
typedef struct {
	int type;
	timelib_tzinfo *tz;
	timelib_sll utc_offset;
	timelib_abbr_info abbr;
} zspec;

/* timezone_initialize() (php_date.c): 0 = ok, 1 = offset out of range, 2 = trailing data, 3 = not found. */
static int zone_init(const char *name, zspec *z, int verbose)
{
	timelib_time *dummy_t = timelib_time_ctor();
	int dst = 0, not_found = 0, status;
	const char *tz = name;

	memset(z, 0, sizeof(*z));
	dummy_t->z = timelib_parse_zone(&tz, &dst, dummy_t, &not_found, timelib_builtin_db(), tz_get);
	if ((dummy_t->z >= (100 * 60 * 60)) || (dummy_t->z <= (-100 * 60 * 60))) {
		status = 1;
	} else {
		dummy_t->dst = dst;
		if (!not_found && (*tz != '\0')) {
			status = 2;
		} else if (not_found) {
			status = 3;
		} else {
			status = 0;
			z->type = dummy_t->zone_type;
			switch (dummy_t->zone_type) {
				case TIMELIB_ZONETYPE_ID:
					z->tz = dummy_t->tz_info;
					break;
				case TIMELIB_ZONETYPE_OFFSET:
					z->utc_offset = dummy_t->z;
					break;
				case TIMELIB_ZONETYPE_ABBR:
					z->abbr.utc_offset = dummy_t->z;
					z->abbr.dst = dummy_t->dst;
					z->abbr.abbr = timelib_strdup(dummy_t->tz_abbr);
					break;
			}
		}
	}
	if (verbose) {
		printf("st=%d used=%d nf=%d dst=%d | ", status, (int) (tz - name), not_found, dst);
		dump_time(dummy_t);
	}
	timelib_time_dtor(dummy_t);
	return status;
}

static void zone_free(zspec *z)
{
	if (z->type == TIMELIB_ZONETYPE_ABBR && z->abbr.abbr) {
		timelib_free(z->abbr.abbr);
	}
}

/* (new DateTime('@ts'))->setTimezone(zone) with microseconds. */
static timelib_time *mk_time(zspec *z, long long ts, long long us)
{
	timelib_time *t = timelib_time_ctor();
	timelib_unixtime2gmt(t, (timelib_sll) ts);
	t->zone_type = TIMELIB_ZONETYPE_OFFSET;
	t->is_localtime = 1;
	t->have_zone = 1;
	t->us = us;
	switch (z->type) {
		case TIMELIB_ZONETYPE_OFFSET:
			timelib_set_timezone_from_offset(t, z->utc_offset);
			break;
		case TIMELIB_ZONETYPE_ABBR:
			timelib_set_timezone_from_abbr(t, z->abbr);
			break;
		case TIMELIB_ZONETYPE_ID:
			timelib_set_timezone(t, z->tz);
			break;
	}
	timelib_unixtime2local(t, t->sse);
	return t;
}

static void do_diff(char **f)
{
	static char n1[MAXLINE], n2[MAXLINE];
	zspec z1, z2;
	int s1, s2;

	unhex(f[1], n1);
	unhex(f[4], n2);
	s1 = zone_init(n1, &z1, 0);
	s2 = zone_init(n2, &z2, 0);
	if (s1 || s2) {
		printf("BADZONE");
	} else {
		timelib_time *t1 = mk_time(&z1, atoll(f[2]), atoll(f[3]));
		timelib_time *t2 = mk_time(&z2, atoll(f[5]), atoll(f[6]));
		int dd = timelib_diff_days(t1, t2);
		timelib_rel_time *rt = timelib_diff(t1, t2);
		printf("dd=%d | ", dd);
		dump_rel(rt);
		printf(" || ");
		dump_time(t1);
		printf(" || ");
		dump_time(t2);
		timelib_rel_time_dtor(rt);
		timelib_time_dtor(t1);
		timelib_time_dtor(t2);
	}
	zone_free(&z1);
	zone_free(&z2);
}

static void do_add(char **f, int n)
{
	static char name[MAXLINE], s[MAXLINE];
	zspec z;
	timelib_rel_time *iv = NULL;

	unhex(f[1], name);
	if (zone_init(name, &z, 0)) {
		printf("BADZONE");
		zone_free(&z);
		return;
	}
	if (strcmp(f[5], "rel") == 0) {
		timelib_error_container *err = NULL;
		size_t len = unhex(n > 6 ? f[6] : "", s);
		timelib_time *p = timelib_strtotime(s, len, &err, timelib_builtin_db(), tz_get);
		if (err->error_count) {
			printf("ERR");
			dump_errors(err);
		} else {
			iv = timelib_rel_time_clone(&p->relative);
		}
		timelib_time_dtor(p);
		timelib_error_container_dtor(err);
	} else if (strcmp(f[5], "f") == 0 && n >= 14) {
		iv = timelib_rel_time_ctor();
		iv->y = atoll(f[6]);
		iv->m = atoll(f[7]);
		iv->d = atoll(f[8]);
		iv->h = atoll(f[9]);
		iv->i = atoll(f[10]);
		iv->s = atoll(f[11]);
		iv->us = atoll(f[12]);
		iv->invert = atoi(f[13]);
		iv->days = TIMELIB_UNSET;
	} else {
		printf("BADLINE");
	}
	if (iv) {
		timelib_time *t = mk_time(&z, atoll(f[2]), atoll(f[3]));
		timelib_time *r = NULL;
		if (strcmp(f[4], "add") == 0) {
			r = timelib_add(t, iv);
		} else if (strcmp(f[4], "sub") == 0) {
			r = timelib_sub(t, iv);
		} else if (strcmp(f[4], "addwall") == 0) {
			r = timelib_add_wall(t, iv);
		} else if (strcmp(f[4], "subwall") == 0) {
			r = timelib_sub_wall(t, iv);
		}
		if (r) {
			dump_time(r);
			timelib_time_dtor(r);
		} else {
			printf("BADOP");
		}
		timelib_time_dtor(t);
		timelib_rel_time_dtor(iv);
	}
	zone_free(&z);
}

static void do_mk(char **f)
{
	static char name[MAXLINE];
	zspec z;
	timelib_time *t;
	long long a, b, c, d;

	unhex(f[1], name);
	if (zone_init(name, &z, 0)) {
		printf("BADZONE");
		zone_free(&z);
		return;
	}
	t = mk_time(&z, atoll(f[2]), 0);
	a = atoll(f[4]);
	b = atoll(f[5]);
	c = atoll(f[6]);
	d = atoll(f[7]);
	if (strcmp(f[3], "date") == 0) {
		t->y = a;
		t->m = b;
		t->d = c;
		timelib_update_ts(t, NULL);
	} else if (strcmp(f[3], "time") == 0) {
		t->h = a;
		t->i = b;
		t->s = c;
		t->us = d;
		timelib_update_ts(t, NULL);
		timelib_update_from_sse(t);
	} else if (strcmp(f[3], "iso") == 0) {
		t->y = a;
		t->m = 1;
		t->d = 1;
		memset(&t->relative, 0, sizeof(t->relative));
		t->relative.d = timelib_daynr_from_weeknr(a, b, c);
		t->have_relative = 1;
		timelib_update_ts(t, NULL);
	} else if (strcmp(f[3], "ts") == 0) {
		timelib_unixtime2local(t, a);
		timelib_update_ts(t, NULL);
		t->us = 0;
	}
	dump_time(t);
	printf(" | off=%lld", timelib_get_current_offset(t));
	timelib_time_dtor(t);
	zone_free(&z);
}

static void do_info(char **f)
{
	int dummy;
	timelib_tzinfo *tz = tz_get(f[1], timelib_builtin_db(), &dummy);
	timelib_sll ts = atoll(f[2]);
	timelib_time_offset *o;
	int32_t off = 0;
	timelib_sll tt = 0;
	unsigned int isdst = 0;
	int ok;

	if (!tz) {
		printf("NOTZ");
		return;
	}
	o = timelib_get_time_zone_info(ts, tz);
	printf("o=%d ls=%u dst=%u abbr=", o->offset, o->leap_secs, o->is_dst);
	put_str(o->abbr);
	printf(" tt=%lld", o->transition_time);
	timelib_time_offset_dtor(o);
	ok = timelib_get_time_zone_offset_info(ts, tz, &off, &tt, &isdst);
	printf(" | ok=%d off=%d tt=%lld dst=%u | in=%d", ok, off, tt, isdst, timelib_timestamp_is_in_dst(ts, tz));
	if (tz->posix_info && tz->posix_info->dst_end) {
		timelib_sll y, m, d;
		timelib_posix_transitions tr;
		size_t i;

		memset(&tr, 0, sizeof(tr));
		timelib_unixtime2date(ts, &y, &m, &d);
		timelib_get_transitions_for_year(tz, y - 1, &tr);
		timelib_get_transitions_for_year(tz, y, &tr);
		timelib_get_transitions_for_year(tz, y + 1, &tr);
		printf(" | n=%zu", tr.count);
		for (i = 0; i < tr.count; i++) {
			printf(" %lld:%lld", tr.times[i], tr.types[i]);
		}
	}
}

static void put_counts(const char *label, timelib_ull a, timelib_ull b, timelib_ull c, timelib_ull d, timelib_ull e, timelib_ull g)
{
	printf(" %s=%llu,%llu,%llu,%llu,%llu,%llu", label, a, b, c, d, e, g);
}

static void put_trans_info(timelib_posix_trans_info *p)
{
	if (!p) {
		printf("-");
		return;
	}
	if (p->type == TIMELIB_POSIX_TRANS_TYPE_MWD) {
		printf("%d:%d.%d.%d/%d", p->type, p->mwd.month, p->mwd.week, p->mwd.dow, p->hour);
	} else {
		printf("%d:%d/%d", p->type, p->days, p->hour);
	}
}

static void do_tzfile(char **f)
{
	static char name[MAXLINE];
	int err = 0;
	timelib_tzinfo *tz;
	timelib_ull i;
	double lat, lon;
	unsigned long long lat_bits, lon_bits;

	unhex(f[1], name);
	tz = timelib_parse_tzfile(name, timelib_builtin_db(), &err);
	printf("err=%d valid=%d", err, timelib_timezone_id_is_valid(name, timelib_builtin_db()));
	if (!tz) {
		return;
	}
	printf(" name=");
	put_str(tz->name);
	lat = tz->location.latitude;
	lon = tz->location.longitude;
	memcpy(&lat_bits, &lat, 8);
	memcpy(&lon_bits, &lon, 8);
	printf(" bc=%u cc=", tz->bc);
	put_str(tz->location.country_code);
	printf(" lat=%016llx lon=%016llx com=", lat_bits, lon_bits);
	put_str(tz->location.comments);
	put_counts("b32", tz->_bit32.ttisgmtcnt, tz->_bit32.ttisstdcnt, tz->_bit32.leapcnt, tz->_bit32.timecnt, tz->_bit32.typecnt, tz->_bit32.charcnt);
	put_counts("b64", tz->bit64.ttisgmtcnt, tz->bit64.ttisstdcnt, tz->bit64.leapcnt, tz->bit64.timecnt, tz->bit64.typecnt, tz->bit64.charcnt);
	printf(" trans=");
	for (i = 0; i < tz->bit64.timecnt; i++) {
		printf("%lld:%u,", (long long) tz->trans[i], tz->trans_idx[i]);
	}
	printf(" types=");
	for (i = 0; i < tz->bit64.typecnt; i++) {
		printf("%d/%d/%u/%u/%u,", tz->type[i].offset, tz->type[i].isdst, tz->type[i].abbr_idx, tz->type[i].isstdcnt, tz->type[i].isgmtcnt);
	}
	printf(" abbrs=");
	for (i = 0; i < tz->bit64.charcnt; i++) {
		printf("%02x", (unsigned char) tz->timezone_abbr[i]);
	}
	printf(" posix=");
	put_str(tz->posix_string);
	if (tz->posix_info) {
		timelib_posix_str *p = tz->posix_info;
		printf(" std=");
		put_str(p->std);
		printf(" so=%lld dst=", p->std_offset);
		put_str(p->dst);
		printf(" do=%lld b=", p->dst_offset);
		put_trans_info(p->dst_begin);
		printf(" e=");
		put_trans_info(p->dst_end);
		printf(" ti=%d,%d", p->type_index_std_type, p->type_index_dst_type);
	}
	timelib_tzinfo_dtor(tz);
}

static void do_dow(char **f)
{
	timelib_sll y = atoll(f[1]), m = atoll(f[2]), d = atoll(f[3]);
	timelib_sll iw, iy, a, b, c;
	timelib_time tmp;

	printf("vd=%d vt=%d leap=%d dn=%lld", timelib_valid_date(y, m, d), timelib_valid_time(y, m, d), timelib_is_leap(y) ? 1 : 0,
		timelib_daynr_from_weeknr(y, m, d));
	memset(&tmp, 0, sizeof(tmp));
	tmp.y = y;
	tmp.m = m;
	tmp.d = d;
	printf(" ed=%lld", timelib_epoch_days_from_time(&tmp));
	timelib_date_from_epoch_days(y, &a, &b, &c);
	printf(" fe=%lld-%lld-%lld", a, b, c);
	timelib_unixtime2date(y, &a, &b, &c);
	printf(" ud=%lld-%lld-%lld", a, b, c);
	if (m >= 0 && m <= 12) {
		printf(" dow=%lld idow=%lld doy=%lld dim=%lld", timelib_day_of_week(y, m, d), timelib_iso_day_of_week(y, m, d),
			timelib_day_of_year(y, m, d), timelib_days_in_month(y, m));
		timelib_isoweek_from_date(y, m, d, &iw, &iy);
		printf(" iw=%lld iy=%lld", iw, iy);
		timelib_isodate_from_date(y, m, d, &a, &b, &c);
		printf(" isod=%lld-%lld-%lld", a, b, c);
	}
	if (m >= -60 && m <= 60 && d >= -60 && d <= 60) {
		timelib_date_from_isodate(y, m, d, &a, &b, &c);
		printf(" fromiso=%lld-%lld-%lld", a, b, c);
	}
}

static void do_abbr(char **f)
{
	static char s[MAXLINE];
	const char *r;

	unhex(f[1], s);
	r = timelib_timezone_id_from_abbr(s, atoll(f[2]), atoi(f[3]));
	put_str(r);
}

static void do_ival(char **f, int n)
{
	static char s[MAXLINE];
	timelib_time *b = NULL, *e = NULL;
	timelib_rel_time *p = NULL;
	int r = 0;
	timelib_error_container *err = NULL;
	size_t len = unhex(n > 1 ? f[1] : "", s);

	timelib_strtointerval(s, len, &b, &e, &p, &r, &err);
	printf("r=%d | b=", r);
	if (b) {
		dump_time(b);
	} else {
		printf("-");
	}
	printf(" | e=");
	if (e) {
		dump_time(e);
	} else {
		printf("-");
	}
	printf(" | p=");
	if (p) {
		dump_rel(p);
	} else {
		printf("-");
	}
	dump_errors_ex(err, 0);
	if (b) timelib_time_dtor(b);
	if (e) timelib_time_dtor(e);
	if (p) timelib_rel_time_dtor(p);
	timelib_error_container_dtor(err);
}

static void do_raw(char **f, int n)
{
	static char s[MAXLINE];
	timelib_error_container *err = NULL;
	size_t len = unhex(n > 1 ? f[1] : "", s);
	timelib_time *t = timelib_strtotime(s, len, &err, timelib_builtin_db(), tz_get);

	timelib_update_ts(t, NULL);
	dump_time(t);
	printf(" || ");
	timelib_update_from_sse(t);
	dump_time(t);
	printf(" || ");
	if (t->zone_type != TIMELIB_ZONETYPE_ID || t->tz_info) {
		timelib_unixtime2local(t, t->sse);
		dump_time(t);
	}
	dump_errors(err);
	timelib_time_dtor(t);
	timelib_error_container_dtor(err);
}

static void do_posix(char **f)
{
	static char s[MAXLINE];
	timelib_posix_str *p;

	unhex(f[1], s);
	p = timelib_parse_posix_str(s);
	if (!p) {
		printf("NULL");
		return;
	}
	printf("std=");
	put_str(p->std);
	printf(" so=%lld dst=", p->std_offset);
	put_str(p->dst);
	printf(" do=%lld b=", p->dst_offset);
	put_trans_info(p->dst_begin);
	printf(" e=");
	put_trans_info(p->dst_end);
	if (p->dst_end) {
		timelib_tzinfo tz;
		timelib_posix_transitions tr;
		size_t i;

		memset(&tz, 0, sizeof(tz));
		memset(&tr, 0, sizeof(tr));
		p->type_index_std_type = 0;
		p->type_index_dst_type = 1;
		tz.posix_info = p;
		timelib_get_transitions_for_year(&tz, atoll(f[2]), &tr);
		printf(" | n=%zu", tr.count);
		for (i = 0; i < tr.count; i++) {
			printf(" %lld:%lld", tr.times[i], tr.types[i]);
		}
	}
	timelib_posix_str_dtor(p);
}

static void do_hms(char **f)
{
	unsigned long long bits = strtoull(f[1], NULL, 16), b1, b2;
	double h, d1, d2;
	int hh, mm, ss;

	memcpy(&h, &bits, 8);
	timelib_decimal_hour_to_hms(h, &hh, &mm, &ss);
	timelib_hms_to_decimal_hour(atoi(f[2]), atoi(f[3]), atoi(f[4]), &d1);
	timelib_hmsf_to_decimal_hour(atoi(f[2]), atoi(f[3]), atoi(f[4]), atoi(f[5]), &d2);
	memcpy(&b1, &d1, 8);
	memcpy(&b2, &d2, 8);
	printf("%d %d %d | %016llx %016llx | %lld | ", hh, mm, ss, b1, b2, timelib_hms_to_seconds(atoll(f[2]), atoll(f[3]), atoll(f[4])));
	put_str(timelib_get_error_message(atoi(f[2])));
}

static const timelib_format_specifier custom_format_map[] = {
	{'+', TIMELIB_FORMAT_ALLOW_EXTRA_CHARACTERS}, {'#', TIMELIB_FORMAT_ANY_SEPARATOR},
	{'j', TIMELIB_FORMAT_DAY_TWO_DIGIT}, {'d', TIMELIB_FORMAT_DAY_TWO_DIGIT_PADDED},
	{'z', TIMELIB_FORMAT_DAY_OF_YEAR}, {'S', TIMELIB_FORMAT_DAY_SUFFIX},
	{'U', TIMELIB_FORMAT_EPOCH_SECONDS}, {'\\', TIMELIB_FORMAT_ESCAPE},
	{'h', TIMELIB_FORMAT_HOUR_TWO_DIGIT_12_MAX}, {'g', TIMELIB_FORMAT_HOUR_TWO_DIGIT_12_MAX_PADDED},
	{'H', TIMELIB_FORMAT_HOUR_TWO_DIGIT_24_MAX}, {'G', TIMELIB_FORMAT_HOUR_TWO_DIGIT_24_MAX_PADDED},
	{'a', TIMELIB_FORMAT_MERIDIAN}, {'A', TIMELIB_FORMAT_MERIDIAN},
	{'u', TIMELIB_FORMAT_MICROSECOND_SIX_DIGIT}, {'v', TIMELIB_FORMAT_MILLISECOND_THREE_DIGIT},
	{'i', TIMELIB_FORMAT_MINUTE_TWO_DIGIT}, {'n', TIMELIB_FORMAT_MONTH_TWO_DIGIT},
	{'m', TIMELIB_FORMAT_MONTH_TWO_DIGIT_PADDED}, {'?', TIMELIB_FORMAT_RANDOM_CHAR},
	{'!', TIMELIB_FORMAT_RESET_ALL}, {'|', TIMELIB_FORMAT_RESET_ALL_WHEN_NOT_SET},
	{'s', TIMELIB_FORMAT_SECOND_TWO_DIGIT}, {';', TIMELIB_FORMAT_SEPARATOR}, {':', TIMELIB_FORMAT_SEPARATOR},
	{'/', TIMELIB_FORMAT_SEPARATOR}, {'.', TIMELIB_FORMAT_SEPARATOR}, {',', TIMELIB_FORMAT_SEPARATOR},
	{'-', TIMELIB_FORMAT_SEPARATOR}, {'(', TIMELIB_FORMAT_SEPARATOR}, {')', TIMELIB_FORMAT_SEPARATOR},
	{'*', TIMELIB_FORMAT_SKIP_TO_SEPARATOR}, {'D', TIMELIB_FORMAT_TEXTUAL_DAY_3_LETTER},
	{'l', TIMELIB_FORMAT_TEXTUAL_DAY_FULL}, {'M', TIMELIB_FORMAT_TEXTUAL_MONTH_3_LETTER},
	{'F', TIMELIB_FORMAT_TEXTUAL_MONTH_FULL}, {'e', TIMELIB_FORMAT_TIMEZONE_OFFSET},
	{'Z', TIMELIB_FORMAT_TIMEZONE_OFFSET_MINUTES}, {'V', TIMELIB_FORMAT_WEEK_OF_YEAR_ISO},
	{'W', TIMELIB_FORMAT_WEEK_OF_YEAR}, {'N', TIMELIB_FORMAT_DAY_OF_WEEK_ISO}, {'w', TIMELIB_FORMAT_DAY_OF_WEEK},
	{'o', TIMELIB_FORMAT_YEAR_ISO}, {' ', TIMELIB_FORMAT_WHITESPACE}, {'y', TIMELIB_FORMAT_YEAR_TWO_DIGIT},
	{'Y', TIMELIB_FORMAT_YEAR_FOUR_DIGIT}, {'x', TIMELIB_FORMAT_YEAR_EXPANDED}, {'X', TIMELIB_FORMAT_YEAR_EXPANDED},
	{'\0', TIMELIB_FORMAT_END}
};

static void do_formatx(char **f, int n)
{
	static char fmt[MAXLINE], s[MAXLINE];
	timelib_format_config cfg = { custom_format_map, atoi(f[1]) ? '%' : '\0' };
	timelib_error_container *err = NULL;
	size_t len;
	timelib_time *t;

	unhex(f[2], fmt);
	len = unhex(n > 3 ? f[3] : "", s);
	t = timelib_parse_from_format_with_map(fmt, s, len, &err, timelib_builtin_db(), tz_get, &cfg);
	dump_time(t);
	dump_errors(err);
	timelib_time_dtor(t);
	timelib_error_container_dtor(err);
}

static char buf[MAXLINE], a1[MAXLINE], a2[MAXLINE];

int main(void)
{
	while (fgets(buf, sizeof(buf), stdin)) {
		char *p = buf, *f[MAXF + 1] = {0};
		int n = 0;
		size_t l = strlen(buf);
		while (l && (buf[l - 1] == '\n' || buf[l - 1] == '\r')) buf[--l] = 0;
		while (n < MAXF) {
			f[n++] = p;
			p = strchr(p, ' ');
			if (!p) break;
			*p++ = 0;
		}
		if (strcmp(f[0], "parse") == 0 && n >= 1) {
			timelib_error_container *err = NULL;
			size_t len = unhex(f[1] ? f[1] : "", a1);
			timelib_time *t = timelib_strtotime(a1, len, &err, timelib_builtin_db(), tz_get);
			dump_time(t);
			dump_errors(err);
			printf("\n");
			timelib_time_dtor(t);
			timelib_error_container_dtor(err);
		} else if (strcmp(f[0], "init") == 0 && n >= 3) {
			size_t len = unhex(f[3] ? f[3] : "", a1);
			do_init(f[1], atoll(f[2]), a1, len);
		} else if (strcmp(f[0], "modify") == 0 && n >= 3) {
			size_t len = unhex(f[3] ? f[3] : "", a1);
			do_modify(f[1], atoll(f[2]), a1, len);
		} else if (strcmp(f[0], "format") == 0 && n >= 2) {
			timelib_error_container *err = NULL;
			size_t len;
			timelib_time *t;
			unhex(f[1], a2);
			len = unhex(f[2] ? f[2] : "", a1);
			t = timelib_parse_from_format(a2, a1, len, &err, timelib_builtin_db(), tz_get);
			dump_time(t);
			dump_errors(err);
			printf("\n");
			timelib_time_dtor(t);
			timelib_error_container_dtor(err);
		} else if (strcmp(f[0], "local") == 0 && n >= 3) {
			int dummy;
			timelib_time *t = timelib_time_ctor();
			t->zone_type = TIMELIB_ZONETYPE_ID;
			t->tz_info = tz_get(f[1], timelib_builtin_db(), &dummy);
			if (!t->tz_info) {
				printf("NOTZ\n");
			} else {
				timelib_unixtime2local(t, (timelib_sll) atoll(f[2]));
				dump_time(t);
				printf("\n");
			}
			timelib_time_dtor(t);
		} else if (strcmp(f[0], "fmtinit") == 0 && n >= 5) {
			size_t len;
			unhex(f[4], a2);
			len = unhex(f[5] ? f[5] : "", a1);
			do_fmtinit(f[1], atoll(f[2]), atoll(f[3]), a2, a1, len);
		} else if (strcmp(f[0], "zone") == 0 && n >= 1) {
			zspec z;
			unhex(f[1] ? f[1] : "", a1);
			zone_init(a1, &z, 1);
			zone_free(&z);
			printf("\n");
		} else if (strcmp(f[0], "diff") == 0 && n >= 7) {
			do_diff(f);
			printf("\n");
		} else if (strcmp(f[0], "add") == 0 && n >= 6) {
			do_add(f, n);
			printf("\n");
		} else if (strcmp(f[0], "mk") == 0 && n >= 8) {
			do_mk(f);
			printf("\n");
		} else if (strcmp(f[0], "info") == 0 && n >= 3) {
			do_info(f);
			printf("\n");
		} else if (strcmp(f[0], "tzfile") == 0 && n >= 1) {
			char *args[2] = { f[0], f[1] ? f[1] : "" };
			do_tzfile(args);
			printf("\n");
		} else if (strcmp(f[0], "dow") == 0 && n >= 4) {
			do_dow(f);
			printf("\n");
		} else if (strcmp(f[0], "abbr") == 0 && n >= 4) {
			do_abbr(f);
			printf("\n");
		} else if (strcmp(f[0], "ival") == 0 && n >= 1) {
			do_ival(f, n);
			printf("\n");
		} else if (strcmp(f[0], "raw") == 0 && n >= 1) {
			do_raw(f, n);
			printf("\n");
		} else if (strcmp(f[0], "posix") == 0 && n >= 3) {
			do_posix(f);
			printf("\n");
		} else if (strcmp(f[0], "hms") == 0 && n >= 6) {
			do_hms(f);
			printf("\n");
		} else if (strcmp(f[0], "formatx") == 0 && n >= 3) {
			do_formatx(f, n);
			printf("\n");
		} else {
			printf("BADLINE\n");
		}
		fflush(stdout);
	}
	return 0;
}

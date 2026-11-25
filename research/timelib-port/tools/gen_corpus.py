#!/usr/bin/env python3
"""Generates differential-test inputs for the timelib harness (research/timelib-port/harness).

Usage: python -I gen_corpus.py <php-src>/ext/date/tests|- <mode> <count> <seed> > corpus.txt

<mode> is one of: parse, init, modify, format, local, fmtinit, zone, diff, add, mk, info, tzfile, dow, abbr, ival,
raw, posix, hms, formatx, trans, extreme, special.
Each output line is a harness command (see harness.c) with strings hex encoded.  For parse/init/modify the corpus
mixes
  * every quoted literal found in php-src's ext/date/tests/*.phpt (the strings PHP's own tests feed to
    strtotime(), date_parse(), DateTime::__construct() and modify()), unless the directory is "-",
  * random sequences of date/time tokens (numbers, month/day names, relative words, zones, separators),
  * random short byte strings over a small alphabet (to hit odd scanner paths: NUL, NBSP, signs, dots).
"format" pairs createFromFormat() formats with inputs built from matching and mismatching pieces; "local"
converts timestamps (dense around DST changes of any year, plus the whole 64-bit range) in every zone of the
database (zone names are read from crates/twig-timelib/src/tzdb/index.rs).  The other modes exercise the rest of
the API: DateTimeZone names (zone), DateTime::diff() (diff), add()/sub() with civil and wall intervals (add),
setDate()/setTime()/setISODate()/setTimestamp() (mk), zone lookups and POSIX transitions (info), the parsed tzfile
of every zone (tzfile), dow.c and epoch-day arithmetic (dow), timezone_name_from_abbr() (abbr), ISO 8601 interval
strings (ival; php-src test literals are included when a test directory is given) and createFromFormat() with
fill_holes() (fmtinit); "raw" runs update_ts()/update_from_sse() on unfilled parse results, "posix" parses POSIX TZ
strings, "hms" covers the decimal-hour helpers, "formatx" uses a custom format map with the ISO and minutes
specifiers and a prefix character, "trans" places diff/add/mk/init/modify within hours of the real transitions
listed in crates/twig-timelib/tests/vectors/zones.json, "extreme" feeds amounts, dates and base times near the
64-bit limits to parse/init/modify/local, and "special" emits fixed edge cases (more than 1000 errors,
overflowing relative amounts, expanded years equal to TIMELIB_UNSET, NNBSP in log dates, ISO weeks past year end).
"""
import os
import random
import re
import sys

TOKENS = [
    "now", "today", "midnight", "noon", "tomorrow", "yesterday", "ago", "next", "last", "previous", "this",
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "eight", "ninth", "tenth",
    "eleventh", "twelfth", "day", "days", "week", "weeks", "fortnight", "forthnight", "month", "months", "year",
    "years", "hour", "hours", "min", "mins", "minute", "minutes", "sec", "secs", "second", "seconds", "ms", "msec",
    "millisecond", "milliseconds", "usec", "µs", "µsec", "microsecond", "microseconds", "weekday",
    "weekdays", "monday", "mon", "tuesday", "tue", "wednesday", "wed", "thursday", "thu", "friday", "fri",
    "saturday", "sat", "sunday", "sun", "mondays", "sundays", "january", "jan", "february", "feb", "march",
    "mar", "april", "apr", "may", "june", "jun", "july", "jul", "august", "aug", "september", "sep", "sept",
    "october", "oct", "november", "nov", "december", "dec", "I", "II", "III", "IV", "V", "VI", "VII", "VIII",
    "IX", "X", "XI", "XII", "first day of", "last day of", "back of", "front of", "of", "am", "pm", "a.m.",
    "p.m.", "AM", "PM", "T", "t", "W", "Z", "UTC", "GMT", "utc", "gmt", "EST", "EDT", "CET", "CEST", "PST", "PDT",
    "MSK", "IST", "BST", "Europe/Paris", "America/New_York", "Asia/Tokyo", "Europe/London", "Australia/Sydney",
    "Pacific/Chatham", "Not/AZone", "Unknown", "+00:00", "-05:00", "+0530", "+01", "-1", "+14:00", "-12",
    "GMT+2", "GMT-0330", "+1:30", "-01:02:03", "+010203", "st", "nd", "rd", "th", "@", "@-", "+", "-", "--",
    "+1", "-1", "+2", "-3", "+10", "1", "2", "3", "5", "7", "9", "10", "11", "12", "13", "15", "20", "24",
    "25", "29", "30", "31", "32", "59", "60", "61", "99", "100", "123", "365", "366", "999", "1000", "1970",
    "1999", "2000", "2004", "2021", "2024", "2038", "9999", "10000", "12345", "0", "00", "000", "0000", "01",
    "001", "1234567890", "1700000000", "9223372036854775807", "99999999999999999999", "2024-01-31",
    "2021-02-29", "2024-W05", "2024W053", "2024-W53-7", "2024.123", "2024123", "20240131", "20240131T101112",
    "2024-01-31T10:11:12", "2024-01-31T10:11:12.123456Z", "2024-01-31T10:11:12+02:00", "10/Oct/2000:13:55:36 -0700",
    "2024:01:31 10:11:12", "31.01.2024", "31.01.24", "1/31/2024", "1/31", "2024/01/31", "31-jan-2024", "jan-31-2024",
    "2024-jan-31", "10:11", "10:11:12", "10.11.12", "10:11:12.5", "101112", "1011", "t1011", "T10:11", "10am",
    "10 pm", "12:30pm", "12:00:00 a.m.", "3:04:05.678 PM", "11:59:59.999999 pm", "\t", " ", ",", ".", "/", ":",
    ";", "(", ")", " ", " ", "\x00", "\n",
]

ALPHABET = "0123456789 :./-+,@TWtamp\tZ\x00( )jfmasondJFMASOND"

FORMAT_PIECES = [
    ("Y", ["2024", "1999", "0001", "99", "12345", "-200"]), ("y", ["24", "99", "7", "123"]),
    ("x", ["+2024", "-0044", "12345", "+", "2024"]), ("X", ["+10000", "-1", "2024"]),
    ("m", ["01", "12", "7", "13", "00"]), ("n", ["1", "12", "07", "13"]), ("d", ["01", "31", "9", "32", "00"]),
    ("j", ["1", "31", "09", "40"]), ("D", ["Mon", "tue", "Sun", "xyz"]), ("l", ["Monday", "friday", "Funday"]),
    ("M", ["Jan", "feb", "DEC", "Foo"]), ("F", ["January", "march", "Smarch"]), ("S", ["st", "nd", "th", "xx"]),
    ("z", ["0", "59", "365", "400"]), ("H", ["00", "23", "24", "7"]), ("G", ["0", "23", "99"]),
    ("h", ["12", "01", "13", "7"]), ("g", ["1", "12", "13"]), ("A", ["AM", "PM", "am", "p.m.", "x"]),
    ("a", ["am", "pm", "A.M.", "b"]), ("i", ["00", "59", "60", "5"]), ("s", ["00", "59", "61", "7"]),
    ("u", ["123456", "1", "1234567", "x"]), ("v", ["123", "1", "1234"]), ("U", ["1700000000", "-1", "+5", "x"]),
    ("e", ["UTC", "Europe/Paris", "+02:00", "EST", "Not/Zone"]), ("T", ["CEST", "GMT+1", "Z", "-0330"]),
    ("O", ["+0200", "-05", "+14:00"]), ("P", ["+02:00", "-12:00", "+1"]), ("p", ["Z", "+00:00"]),
    ("!", [""]), ("|", [""]), ("+", ["", "trailing"]), ("*", ["abc", "x1"]), ("?", ["x", "1"]),
    ("#", [";", ":", "/", ".", ",", "-", "(", ")", "x"]), (" ", [" ", "  ", "\t", ""]), (":", [":", "x"]),
    ("/", ["/", "-"]), ("-", ["-", "/"]), (".", [".", ","]), ("\\T", ["T", "t"]), ("\\", [""]), ("Q", ["Q", "q"]),
]

ZONES = ["UTC", "Europe/Paris", "America/New_York", "Asia/Kolkata", "Australia/Lord_Howe", "Pacific/Chatham",
         "America/Sao_Paulo", "Africa/Casablanca", "Europe/London", "America/St_Johns", "Asia/Tehran", "Etc/GMT+5"]
BASES = [0, 1700000000, 1711846800, 1730000000, 951782400, -1000000000, 2147483647, 4102444800, 1616893200,
         1636246800]

ZONE_IDX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "crates", "twig-timelib",
                        "src", "tzdb", "index.rs")


def literals(test_dir):
    out = set()
    pat = re.compile(r"'((?:[^'\\\n]|\\.){0,60})'|\"((?:[^\"\\\n$]|\\.){0,60})\"")
    for name in sorted(os.listdir(test_dir)):
        if not name.endswith(".phpt"):
            continue
        with open(os.path.join(test_dir, name), encoding="latin-1") as fh:
            text = fh.read()
        for m in pat.finditer(text):
            s = m.group(1) if m.group(1) is not None else m.group(2)
            s = s.replace("\\'", "'").replace('\\"', '"').replace("\\\\", "\\")
            out.add(s)
    return sorted(out)


def rand_tokens(rng):
    n = rng.randint(1, 6)
    parts = []
    for _ in range(n):
        parts.append(rng.choice(TOKENS))
        parts.append(rng.choice(["", " ", " ", " ", "  ", "-", "/", ".", ",", ":", "\t"]))
    return "".join(parts).strip(" ") if rng.random() < 0.7 else "".join(parts)


def rand_bytes(rng):
    n = rng.randint(0, 14)
    return "".join(rng.choice(ALPHABET) for _ in range(n))


def zone_names():
    with open(ZONE_IDX, encoding="utf-8") as fh:
        return re.findall(r'^    \("([^"]+)", 0x', fh.read(), re.M)


def format_case(rng):
    n = rng.randint(1, 7)
    fmt, inp = [], []
    for _ in range(n):
        spec, values = rng.choice(FORMAT_PIECES)
        fmt.append(spec)
        inp.append(rng.choice(values))
    if rng.random() < 0.1:
        inp.append(rng.choice(["x", "1", " "]))
    if rng.random() < 0.1 and inp:
        inp.pop()
    return "".join(fmt), "".join(inp)


def local_cases(rng, count):
    zones = zone_names()
    out = []
    per_zone = max(count // len(zones), 1)
    for z in zones:
        for _ in range(per_zone):
            r = rng.random()
            if r < 0.6:
                # around a likely DST change (late March, early April, late October, early November), any year
                year = rng.randint(1900, 2100)
                base = int((year - 1970) * 365.2425 * 86400)
                month_off = rng.choice([80, 85, 88, 95, 100, 280, 295, 300, 305, 310]) * 86400
                ts = base + month_off + rng.randint(-3 * 86400, 3 * 86400) // 900 * 900 + rng.choice([-1, 0, 1, 0])
            elif r < 0.9:
                ts = rng.randint(-2 ** 35, 2 ** 35)
            else:
                ts = rng.randint(-2 ** 62, 2 ** 62)
            out.append((z, ts))
    return out


ABBR_DATA = os.path.join(os.path.dirname(ZONE_IDX), "..", "abbr_data.rs")

OFFSETS = ["+00:00", "-05:00", "+05:30", "+0530", "-0330", "+1", "-1", "+01", "+14:00", "-12", "+123", "+1234",
           "+12345", "+123456", "-123456", "+12:34:56", "+99:99", "+99:59:59", "+100:00", "-100:00", "+99", "+999999",
           "GMT+2", "GMT-0330", "GMT+", "+", "-", "+:", "+1:", "+1:3", "+01:3", "+01:30:", "+0:0:0", "+00000000",
           "+12:34:5", "+1:23:45", "+99:99:99", "-99:59", "+9:59:59"]
WEIRD_ZONES = ["", " ", "(", "()", "(UTC)", "((CEST))", " (EST", "UTC)", "Z", "z", "utc", "gmt", "GMT", "Utc",
               "Not/AZone", "Europe/Paris x", "Europe/Paris ", "UTC+1", "CEST+1", "EST5EDT", "est5edt", "Etc/GMT+5",
               "etc/gmt-14", "America/Argentina/Buenos_Aires", "AMERICA/NEW_YORK", "europe/paris", "a", "j", "Ab",
               "abcdef", "abcdefg", "_", "/", "-", "+-1", "--1", "UTC\x00junk", "\tUTC", "MSK", "msk", "IST", "BST"]
CALC_ZONES = ["UTC", "Europe/Paris", "America/New_York", "Australia/Lord_Howe", "Pacific/Chatham", "Asia/Kolkata",
              "America/Sao_Paulo", "Europe/London", "America/St_Johns", "Africa/Casablanca", "Asia/Tehran",
              "Pacific/Apia", "Antarctica/Troll", "Europe/Dublin", "America/Santiago", "Asia/Gaza", "Europe/Moscow",
              "Pacific/Kiritimati", "America/Havana", "Etc/GMT+5", "+05:30", "-03:00", "+00:00", "-12:00", "+14:00",
              "+01:02:03", "CEST", "EST", "PDT", "ACDT", "BST", "IST", "NZDT", "utc", "Z"]
RELS = ["+1 day", "+1 month", "-1 month", "+1 year", "+24 hours", "next monday", "last day of next month",
        "first day of this month", "+3 weekdays", "-2 weekdays", "1 week ago", "+90 minutes", "+1 day +1 second",
        "+1 month -1 day", "last friday of next month", "+1 hour 30 minutes", "yesterday", "tomorrow", "+500 ms",
        "-1500 msec", "+1 week 2 days 4 hours 2 seconds", "first monday of", "sunday this week", "monday next week",
        "+5 weekdays", "0 weekdays", "-0 weekdays", "this week", "saturday", "+1 fortnight", "1 year ago",
        "-1 year -1 month -1 day", "+100000 years", "noon", "+2147483648 seconds", "+1.5 hours", "last day of",
        "first day of next year", "third wednesday of", "+1 week", "-7 weekdays", "+250 usec", "6 months ago"]
ISO_FIXED = ["", " ", "  ", "P", "PT", "P1Y", "P1Y2M3DT4H5M6S", "PT36H", "P2W", "P1W2D", "P0D", "PT0S",
             "R4/2012-07-01T00:00:00Z/P7D", "2008-03-01T13:00:00Z/P1Y2M10DT2H30M",
             "R5/2008-03-01T13:00:00Z/P1Y2M10DT2H30M", "2008-03-01T13:00:00Z/2009-05-11T15:30:00Z",
             "P0001-02-03T04:05:06", "P1Y2M3DT4H5M6", "P-1D", "p1d", "PT1M", "P1M", "P1DT", "P 1D", "P1D ", " P1D",
             "R-1/2012-07-01T00:00:00Z/P7D", "R/2012-07-01T00:00:00Z/P7D", "20120701T000000Z", "20120701T000000Z/P1D",
             "P1234567890123D", "P123456789012D", "R1234567890", "R4294967296", "P1Y1Y", "PT1H1H", "P1D/P2D",
             "2012-07-01T00:00:00Z/2012-07-01T00:00:00Z/2012-07-01T00:00:00Z", "P9999-12-31T24:59:59",
             "P0000-00-00T00:00:00", "P2024-13-32T25:60:60", "xP1D", "P1X", "P1D\x00junk", "P1D\njunk", "\tP1D\t",
             "P1Y,P2Y", "P.5Y", "P1.5Y", "+P1D", "R5", "R5/", "/P1D", "P1W1D", "P1MT1M"]


def abbr_names():
    with open(ABBR_DATA, encoding="utf-8") as fh:
        return sorted(set(re.findall(r'TzLookup \{ name: "([^"]*)"', fh.read())))


def mangle_case(rng, s):
    r = rng.random()
    if r < 0.6:
        return s
    if r < 0.75:
        return s.lower()
    if r < 0.9:
        return s.upper()
    return "".join(c.upper() if rng.random() < 0.5 else c.lower() for c in s)


def zone_strings(rng, count):
    zones, abbrs = zone_names(), abbr_names()
    out = list(zones) + [a for a in abbrs] + [a.upper() for a in abbrs] + OFFSETS + WEIRD_ZONES
    while len(out) < count:
        r = rng.random()
        if r < 0.3:
            out.append(mangle_case(rng, rng.choice(zones)))
        elif r < 0.5:
            out.append(mangle_case(rng, rng.choice(abbrs)))
        elif r < 0.7:
            digits = "".join(rng.choice("0123456789:") for _ in range(rng.randint(0, 9)))
            out.append(rng.choice(["", "GMT", "(", " ", "\t"]) + rng.choice("+-") + digits + rng.choice(["", ")", " ", "x"]))
        elif r < 0.85:
            out.append(rng.choice(["", "(", " ", "\t", "(("]) + rng.choice(zones + abbrs + OFFSETS) +
                       rng.choice(["", ")", "))", " ", "x", "/x"]))
        else:
            out.append("".join(rng.choice("abcXYZ/_-+0123:() \t") for _ in range(rng.randint(0, 10))))
    return out


def rand_ts(rng):
    r = rng.random()
    if r < 0.5:
        # around a likely DST change, 1970-2040
        year = rng.randint(1970, 2040)
        base = int((year - 1970) * 365.2425 * 86400)
        month_off = rng.choice([80, 85, 88, 95, 100, 280, 295, 300, 305, 310]) * 86400
        return base + month_off + rng.randint(-3 * 86400, 3 * 86400) // 900 * 900 + \
            rng.choice([-1, 0, 1, 0, 1800, 3600, -3600])
    if r < 0.8:
        return rng.choice(BASES) + rng.randint(-200000000, 200000000)
    if r < 0.95:
        return rng.randint(-2 ** 35, 2 ** 35)
    return rng.randint(-2 ** 40, 2 ** 40)


def rand_zone(rng, zones):
    return rng.choice(zones) if rng.random() < 0.85 else rng.choice(CALC_ZONES)


def hexs(s):
    return s.encode("utf-8").hex()


def rand_field(rng, small, big):
    r = rng.random()
    if r < 0.7:
        return rng.randint(0, small)
    if r < 0.85:
        return rng.randint(-small, small)
    if r < 0.97:
        return rng.randint(-big, big)
    return rng.choice([2 ** 31, -2 ** 31, 2 ** 40, -2 ** 40])


def diff_lines(rng, count):
    zones = zone_names()
    for _ in range(count):
        z1 = rand_zone(rng, CALC_ZONES if rng.random() < 0.8 else zones)
        z2 = z1 if rng.random() < 0.6 else rand_zone(rng, CALC_ZONES)
        t1 = rand_ts(rng)
        if rng.random() < 0.4:
            t2 = t1 + rng.choice([3600, -3600, 86400, -86400, 82800, 90000, 1, -1, 0, 1800, 7200, 86400 * 31,
                                  86400 * 365, -86400 * 366])
        else:
            t2 = rand_ts(rng) if rng.random() < 0.5 else t1 + rng.randint(-10 ** 8, 10 ** 8)
        u1 = 0 if rng.random() < 0.5 else rng.randint(0, 999999)
        u2 = 0 if rng.random() < 0.5 else rng.randint(0, 999999)
        yield "diff %s %d %d %s %d %d" % (hexs(z1), t1, u1, hexs(z2), t2, u2)


def add_lines(rng, count):
    for _ in range(count):
        z = rand_zone(rng, CALC_ZONES)
        ts = rand_ts(rng)
        us = 0 if rng.random() < 0.6 else rng.randint(0, 999999)
        op = rng.choice(["add", "sub", "addwall", "subwall"])
        if rng.random() < 0.5:
            s = rng.choice(RELS) if rng.random() < 0.7 else rand_tokens(rng)
            yield "add %s %d %d %s rel %s" % (hexs(z), ts, us, op, hexs(s))
        else:
            f = [rand_field(rng, 3, 10 ** 4), rand_field(rng, 14, 10 ** 4), rand_field(rng, 40, 10 ** 6),
                 rand_field(rng, 30, 10 ** 6), rand_field(rng, 100, 10 ** 7), rand_field(rng, 100, 10 ** 8),
                 0 if rng.random() < 0.5 else rand_field(rng, 999999, 10 ** 9), rng.choice([0, 0, 1])]
            yield "add %s %d %d %s f %s" % (hexs(z), ts, us, op, " ".join(str(v) for v in f))


def mk_lines(rng, count):
    for _ in range(count):
        z = rand_zone(rng, CALC_ZONES)
        ts = rand_ts(rng)
        op = rng.choice(["date", "time", "iso", "ts"])
        if op == "date":
            v = [rng.randint(-3000, 5000) if rng.random() < 0.9 else rand_field(rng, 10 ** 6, 10 ** 15),
                 rng.randint(-30, 30) if rng.random() < 0.9 else rand_field(rng, 10 ** 4, 10 ** 12),
                 rng.randint(-400, 400) if rng.random() < 0.9 else rand_field(rng, 10 ** 5, 10 ** 12), 0]
        elif op == "time":
            v = [rng.choice([0, 1, 2, 3, 23, 24, 25, -1]) if rng.random() < 0.5 else rand_field(rng, 30, 10 ** 6),
                 rng.choice([0, 30, 59, 60, -1]) if rng.random() < 0.5 else rand_field(rng, 100, 10 ** 8),
                 rng.choice([0, 59, 60]) if rng.random() < 0.5 else rand_field(rng, 100, 10 ** 9),
                 0 if rng.random() < 0.5 else rand_field(rng, 999999, 10 ** 10)]
        elif op == "iso":
            v = [rng.randint(-3000, 5000) if rng.random() < 0.9 else rand_field(rng, 10 ** 6, 10 ** 12),
                 rng.randint(-60, 60) if rng.random() < 0.9 else rand_field(rng, 100, 10 ** 10),
                 rng.randint(-10, 10) if rng.random() < 0.9 else rand_field(rng, 100, 10 ** 10), 0]
        else:
            v = [rand_ts(rng), 0, 0, 0]
        yield "mk %s %d %s %s" % (hexs(z), ts, op, " ".join(str(x) for x in v))


ZONES_JSON = os.path.join(os.path.dirname(ZONE_IDX), "..", "..", "tests", "vectors", "zones.json")


def transitions():
    """Real transition instants (zone, ts, offset before, offset after) from the oracle's zones.json."""
    import json
    with open(ZONES_JSON, encoding="utf-8") as fh:
        data = json.load(fh)
    out = []
    for name, _begin, _end, entries in data["transitions"]:
        for prev, cur in zip(entries, entries[1:]):
            if prev[1] != cur[1]:
                out.append((name, cur[0], prev[1], cur[1]))
    return out


def gm(ts):
    import datetime
    d = datetime.datetime(1970, 1, 1) + datetime.timedelta(seconds=ts)
    return d.strftime("%Y-%m-%d %H:%M:%S")


def trans_lines(rng, count):
    """diff/add/mk/init/modify commands within hours of real DST and offset transitions."""
    trs = transitions()
    mods = ["+1 day", "-1 day", "+1 hour", "-1 hour", "+30 minutes", "+24 hours", "-24 hours", "midnight", "noon",
            "tomorrow", "yesterday", "+1 week", "today 02:30", "02:30", "03:30", "01:30", "+0 day", "+1 month",
            "first day of next month", "last day of this month", "next sunday", "+2 hours", "-90 minutes"]
    for _ in range(count):
        zone, t, before, after = rng.choice(trs)
        kind = rng.random()
        if kind < 0.25:
            t1 = t + rng.randint(-7200, 7200)
            t2 = t + rng.randint(-7200, 7200) if rng.random() < 0.6 else \
                t1 + rng.choice([86400, -86400, 82800, 90000, 3600, -3600, 86400 * 2]) + rng.randint(-120, 120)
            yield "diff %s %d %d %s %d %d" % (hexs(zone), t1, rng.choice([0, 0, 500000]), hexs(zone), t2,
                                               rng.choice([0, 0, 250000]))
        elif kind < 0.45:
            ts = t + rng.randint(-7200, 7200) if rng.random() < 0.7 else t - 86400 + rng.randint(-7200, 7200)
            op = rng.choice(["add", "sub", "addwall", "subwall"])
            if rng.random() < 0.5:
                yield "add %s %d 0 %s rel %s" % (hexs(zone), ts, op, hexs(rng.choice(mods)))
            else:
                f = [0, rng.choice([0, 0, 1]), rng.choice([0, 1, 1, 2]), rng.choice([0, 1, 2, 3, 23, 24, 25]),
                     rng.choice([0, 0, 30, 59, 90]), rng.choice([0, 0, 1, 59]), rng.choice([0, 0, 0, 999999]),
                     rng.choice([0, 0, 1])]
                yield "add %s %d 0 %s f %s" % (hexs(zone), ts, op, " ".join(str(v) for v in f))
        elif kind < 0.65:
            ts = t + rng.choice([-86400, 0, 86400]) + rng.randint(-14400, 14400)
            if rng.random() < 0.6:
                yield "mk %s %d time %d %d %d %d" % (hexs(zone), ts, rng.randint(0, 4), rng.choice([0, 15, 30, 45, 59]),
                                                     rng.choice([0, 30, 59]), rng.choice([0, 0, 1]))
            else:
                y, m, d = (int(x) for x in gm(t + after).split(" ")[0].split("-"))
                yield "mk %s %d date %d %d %d 0" % (hexs(zone), ts - 86400, y, m, d + rng.choice([-1, 0, 0, 1]))
        elif kind < 0.85:
            wall = gm(t + rng.choice([before, after]) + rng.choice([-3601, -1800, -1, 0, 1, 1800, 3599]))
            if rng.random() < 0.3:
                wall += rng.choice([" CET", " CEST", " EST", " EDT", " +01:00", " Z", " UTC"])
            yield "init %s %d %s" % (zone, rng.choice(BASES), hexs(wall))
        else:
            yield "modify %s %d %s" % (zone, t + rng.randint(-10800, 10800), hexs(rng.choice(mods)))


FORMATX_PIECES = [
    ("%Y", ["2024", "1999", "99"]), ("%V", ["01", "53", "54", "00", "5", "x"]), ("%N", ["1", "7", "0", "8", "x"]),
    ("%o", ["2024", "1999", "0001", "x", "12"]), ("%Z", ["+60", "-330", "+0", "-", "x", "+99999"]),
    ("%d", ["01", "31", "x"]), ("%m", ["02", "13"]), ("%H", ["10", "24"]), ("%i", ["30"]), ("%s", ["59"]),
    ("%%", ["%", "x"]), ("-", ["-", "x"]), ("Y", ["Y", "2"]), ("%w", ["w", "3"]), ("%W", ["W", "1"]),
    ("%e", ["UTC", "+01:00", "Europe/Paris"]), ("%!", [""]), ("%|", [""]), ("%+", ["", "junk"]), (" ", [" ", ""]),
    ("%", [""]), ("V", ["V", "1"]), ("%z", ["0", "365"]), ("%D", ["Mon"]), ("%a", ["am", "a.x", "a.m", "p.mx"]),
    ("%X", ["-9999999", "+2024"]), ("%U", ["1700000000"]), ("%?", ["x"]), ("%*", ["abc"]), ("%\\", ["", "x"]),
]

EXTREME_NUMS = ["9223372036854775807", "9223372036854775808", "-9223372036854775808", "-9223372036854775809",
                "99999999999999999999999", "4294967296", "-4294967296", "2147483647", "2147483648", "-2147483649",
                "292277026596", "-292277026596", "292277022657", "999999999", "9223372036854", "1000000000000", "0",
                "-0", "1", "00000000000000000001"]
EXTREME_UNITS = ["days", "day", "months", "years", "weeks", "fortnights", "hours", "minutes", "sec", "seconds", "ms",
                 "usec", "monday", "weekdays", "msec", "µs"]
EXTREME_DATES = ["-292277026596-13-32", "-0001-12-31", "0000-01-01", "9999-12-31 23:59:59", "+100000-00-31",
                 "-9999999999999999999-13-01", "-9999999999999999999-01-00", "2024-02-29", "-292277026596-13-01",
                 "+292277026596-12-31", "-292277022657-01-27 08:29:52", "292277026596-12-04 15:30:07"]
EXTREME_REL = ["first day of", "last day of", "next month", "last day of next month", "next sunday of",
               "this monday of next month", "previous sunday of", "+1 day", "-1 sec", "+1 week", "next month of",
               "last month of next month", "first monday of next year", "+5 weekdays", "-5 weekdays"]
EXTREME_BASES = [-9223372036854775808, -9223372036854775807, -9223372036854775000, 9223372036854775807,
                 9223372036854775000, -67768040609740800, 67767976233532799, 253402300799, -62135596800, 0, -1]
EXTREME_ZONES = ["UTC", "Europe/Paris", "America/New_York", "Australia/Lord_Howe", "Pacific/Kiritimati"]


def extreme_input(rng):
    parts = []
    for _ in range(rng.randint(1, 3)):
        r = rng.random()
        if r < 0.45:
            parts.append(rng.choice(["+", "-", "+-", "--", "+--", ""]) + rng.choice(EXTREME_NUMS) + " " +
                         rng.choice(EXTREME_UNITS))
        elif r < 0.6:
            parts.append(rng.choice(EXTREME_DATES))
        elif r < 0.8:
            parts.append(rng.choice(EXTREME_REL))
        else:
            parts.append("@" + rng.choice(["", "-", "--"]) + rng.choice(EXTREME_NUMS) +
                         rng.choice(["", ".5", ".123456", ".9999999"]))
    return " ".join(parts)


POSIX_PIECES = ["CET", "CEST", "EST", "EDT", "<+0330>", "<-03>", "<+14>", "<>", "<abc", "A", "-1", "1", "+1", "5",
                "-3:30", "3:30:45", "-12:59:59", "24", "167", ",", "M3.5.0", "M10.5.0/3", "M3.2.0/2:00:00",
                "M11.1.0", "M4.1.0/-1", "M9.5.0/25", "J60", "J1", "J365", "0", "59", "365", "/2", "/-2", "/26",
                "M13.1.0", "M3.6.0", "M3.5.7", "M0.1.0", "M3.0.0", "M", "M3", "M3.", "M3.5", "M3.5.", "J", ".", ":",
                "x", "", " "]


def main():
    test_dir, mode, count, seed = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
    rng = random.Random(seed)
    out = sys.stdout
    if mode == "format":
        for _ in range(count):
            f, s = format_case(rng)
            out.write("format %s %s\n" % (f.encode("utf-8").hex(), s.encode("utf-8").hex()))
        return
    if mode == "local":
        for z, ts in local_cases(rng, count):
            out.write("local %s %d\n" % (z, ts))
        return
    if mode == "fmtinit":
        for _ in range(count):
            f, s = format_case(rng)
            out.write("fmtinit %s %d %d %s %s\n" % (rng.choice(ZONES), rand_ts(rng), rng.choice([0, 0, 1, 999999,
                      rng.randint(0, 999999)]), hexs(f), hexs(s)))
        return
    if mode == "zone":
        for s in zone_strings(rng, count):
            out.write("zone %s\n" % hexs(s))
        return
    if mode in ("diff", "add", "mk"):
        gen = {"diff": diff_lines, "add": add_lines, "mk": mk_lines}[mode]
        for line in gen(rng, count):
            out.write(line + "\n")
        return
    if mode == "info":
        zones = zone_names()
        per_zone = max(count // len(zones), 1)
        for z in zones:
            for _ in range(per_zone):
                ts = rand_ts(rng) if rng.random() < 0.8 else rng.randint(-2 ** 62, 2 ** 62)
                out.write("info %s %d\n" % (z, ts))
        return
    if mode == "tzfile":
        zones = zone_names()
        names = list(zones) + ["", "UTC\x00", "Nowhere/City", "Europe", "Europe/", "europe/paris", "EUROPE/PARIS"]
        while len(names) < count:
            names.append(mangle_case(rng, rng.choice(zones)) if rng.random() < 0.8 else
                         "".join(rng.choice("aeEuUrop/_TC") for _ in range(rng.randint(0, 8))))
        for n in names:
            out.write("tzfile %s\n" % hexs(n))
        return
    if mode == "trans":
        for line in trans_lines(rng, count):
            out.write(line + "\n")
        return
    if mode == "extreme":
        for _ in range(count):
            r = rng.random()
            s = hexs(extreme_input(rng))
            if r < 0.33:
                out.write("parse %s\n" % s)
            elif r < 0.66:
                out.write("init %s %d %s\n" % (rng.choice(EXTREME_ZONES), rng.choice(EXTREME_BASES), s))
            elif r < 0.99:
                out.write("modify %s %d %s\n" % (rng.choice(EXTREME_ZONES), rng.choice(EXTREME_BASES), s))
            else:
                out.write("local %s %d\n" % (rng.choice(EXTREME_ZONES), rng.choice(EXTREME_BASES)))
        return
    if mode == "raw":
        inputs = literals(test_dir) if test_dir != "-" else []
        while len(inputs) < count:
            inputs.append(rand_tokens(rng) if rng.random() < 0.75 else rand_bytes(rng))
        for s in inputs[:count]:
            out.write("raw %s\n" % hexs(s))
        return
    if mode == "posix":
        for _ in range(count):
            s = "".join(rng.choice(POSIX_PIECES) for _ in range(rng.randint(1, 7)))
            if rng.random() < 0.4:
                s = rng.choice(["CET-1CEST,", "EST5EDT,", "<+0330>-3:30<+0430>,", "NZST-12NZDT,", "AEST-10AEDT,",
                                "IST-1GMT0,", "<-03>3<-02>,", "WET0WEST,"]) + \
                    rng.choice(["M3.5.0,M10.5.0/3", "M3.2.0,M11.1.0", "J60/0,J300/1", "59,300", "J1/24,J365/-1",
                                "M10.1.0,M4.1.0/3", "M9.5.0/2,M4.1.0/3", "0/0,J365/25", "M3.5.0/-2,M10.5.0/26",
                                "M2.5.6/167,M10.5.0"])
            out.write("posix %s %d\n" % (hexs(s), rng.choice([1970, 2024, 2025, 2100, 1600, -5, 0, 9999,
                                                                rng.randint(-10000, 10000)])))
        return
    if mode == "hms":
        import struct
        for _ in range(count):
            r = rng.random()
            if r < 0.6:
                h = rng.uniform(-30, 30)
            elif r < 0.8:
                h = rng.choice([0.0, -0.0, 1.5, -1.5, 23.999999, 24.0, 2147483647.5, 2147483648.0, -2147483649.0,
                                1e300, -1e300, float("inf"), float("-inf"), float("nan"), 5e-324, 0.1, 1 / 3])
            else:
                h = rng.uniform(-1e10, 1e10)
            bits = struct.unpack("<Q", struct.pack("<d", h))[0]
            vals = [rng.randint(-100, 100), rng.randint(-100, 100), rng.randint(-10000, 10000),
                    rng.randint(-10 ** 7, 10 ** 7)]
            if rng.random() < 0.1:
                vals[0] = rng.choice([-1, 0, 5, 9, 10, 11, 2 ** 31 - 1, -2 ** 31])
            out.write("hms %016x %s\n" % (bits, " ".join(str(v) for v in vals)))
        return
    if mode == "formatx":
        for _ in range(count):
            n = rng.randint(1, 6)
            fmt, inp = [], []
            for _ in range(n):
                spec, values = rng.choice(FORMATX_PIECES)
                fmt.append(spec)
                inp.append(rng.choice(values))
            if rng.random() < 0.1:
                inp.append(rng.choice(["x", "1", " "]))
            out.write("formatx %d %s %s\n" % (rng.choice([0, 1, 1]), hexs("".join(fmt)), hexs("".join(inp))))
        return
    if mode == "special":
        lines = []
        # More than TIMELIB_MAX_ERRORS (1000) errors / warnings.
        lines.append("parse " + hexs("!" * 1500))
        lines.append("parse " + hexs("x" * 1200 + " UTC UTC UTC"))
        lines.append("parse " + hexs("UTC " * 1100))
        lines.append("format " + hexs("Y" * 1100) + " " + hexs("x" * 1100))
        lines.append("format " + hexs("-" * 1500) + " " + hexs("x" * 1500))
        lines.append("format " + hexs("+" + "d" * 600) + " " + hexs("01" * 600 + "junk"))
        lines.append("ival " + hexs("x" * 1500))
        lines.append("ival " + hexs("P1Dx" * 400))
        # Relative amounts that overflow int64 (add_with_overflow): 13-digit amounts of milliseconds.
        for unit in ["ms", "msec", "usec", "sec", "min", "hour", "day", "fortnight", "month", "year", "weeks"]:
            for sign in ["+", "-"]:
                lines.append("parse " + hexs(("%s9999999999999 %s " % (sign, unit)) * 1000))
        lines.append("parse " + hexs("+9999999999999 ms " * 922 + "-9999999999999 ms " * 5))
        # Expanded years equal to TIMELIB_UNSET, meridian edge cases, NNBSP in the common log format.
        for f, s in [("X", "-9999999"), ("x", "-9999999"), ("X", "-9999998"), ("Y-m-d", "-999-01-01"),
                     ("h A", "10 a.x"), ("h A", "10 a.m"), ("h a", "10 p.mx"), ("h a", "10 p.m."), ("h a", "10 P.M."),
                     ("g a", "1 a"), ("g A", "12 pm"), ("h A", "13 am"), ("A h", "am 10"), ("h a", "10 x"),
                     ("u", "1234567"), ("v", "12"), ("U", "-9223372036854775808"), ("U", "9223372036854775808"),
                     ("e", "Europe/Paris"), ("e", "(CEST)"), ("T", "GMT+99:99"), ("O", "+100:00"), ("P", "-99:59:59"),
                     # Both strings are read as C strings: an embedded NUL ends them.
                     ("Y\x00m", "2024\x00junk"), ("Y-m", "2024\x00-01"), ("Y", "2024\x00"), ("\x00", "x"),
                     ("Y\x00", "2024"), ("+Y", "2024\x00\x00x")]:
            lines.append("format %s %s" % (hexs(f), hexs(s)))
        for s in ["10/Oct/2000:13:55:36 -0700", "10/Oct/2000:13:55:36 -0700", "10/Oct/2000:13:55:36\t+0100",
                  "10/Oct/2000:13:55:36 GMT+0100", "1/Jan/2000:00:00:00 -12:00:00", "10/oct/2000:13:55:36 +14:00"]:
            lines.append("parse " + hexs(s))
            lines.append("init UTC 0 " + hexs(s))
        # Broken POSIX TZ strings (each failure exit of timelib_parse_posix_str()).
        for p in ["CET-1CEST,M.5.0,M10.5.0", "CET-1CEST,M3.5.x,M10.5.0", "CET-1CEST,M3.5.0/x,M10.5.0",
                  "CET-1CEST,M3.5.0,Mx", "CET-1CEST,M3.5.0,M10.5.0x", "CET-1CEST,M3.5.0", "CET-1CEST,M3.5.0,",
                  "CET-1CEST,M3x5.0,M10.5.0", "CET-1CEST,M3.5x0,M10.5.0", "CET-1CEST,M3.5.0,M10.5.0/", "CET-1CEST,",
                  "CET-1CEST", "CET-1CEST+", "CET-1CEST1:", "CET-1:", "CET-1::", "CET", "<+03>-3<+04>,J60,J300",
                  "<+03>-3<+04>,x,J300", "<+03>-3<+04>,J60,Jx", "<+03>-3<+04>,60/1:2:3,300/-1:30"]:
            lines.append("posix %s 2024" % hexs(p))
        # ISO dates whose day number runs past the end of the ISO year (timelib_date_from_isodate loops).
        for y in [2004, 2009, 2015, 2020, 2026, 2032, 1999, 1, 0, -1, 9999]:
            for w in [52, 53, 54, 60]:
                for d in [0, 1, 7, 8, 30]:
                    lines.append("dow %d %d %d" % (y, w, d))
                    lines.append("mk %s 0 iso %d %d %d 0" % (hexs("UTC"), y, w, d))
        for line in lines:
            out.write(line + "\n")
        return
    if mode == "dow":
        for _ in range(count):
            r = rng.random()
            if r < 0.1:
                y, m, d = rng.randint(-2000, 3000), rng.randint(40, 60), rng.randint(-10, 10)
            elif r < 0.8:
                y, m, d = rng.randint(-2000, 3000), rng.randint(-2, 14), rng.randint(-40, 40)
            elif r < 0.95:
                y, m, d = rand_field(rng, 10 ** 6, 10 ** 15), rng.randint(0, 12), rand_field(rng, 40, 10 ** 12)
            else:
                y, m, d = rng.randint(-2 ** 63, 2 ** 63 - 1), rng.randint(-2 ** 63, 2 ** 63 - 1), \
                    rng.randint(-2 ** 63, 2 ** 63 - 1)
            out.write("dow %d %d %d\n" % (y, m, d))
        return
    if mode == "abbr":
        abbrs = abbr_names()
        offsets = [-1, 0, 1, 3600, -3600, -18000, 7200, 37800, 19800, -14400, 50400, -43200, 12600, 20700, -12600,
                   34200, 2 ** 32 + 3600]
        for _ in range(count):
            a = mangle_case(rng, rng.choice(abbrs)) if rng.random() < 0.85 else rng.choice(["", "x", "utc", "GMT",
                                                                                        "zz", "a", "nope"])
            off = rng.choice(offsets) if rng.random() < 0.8 else rng.randint(-50400, 50400) // 900 * 900
            out.write("abbr %s %d %d\n" % (hexs(a), off, rng.choice([-1, 0, 1, 1, 0, 2])))
        return
    if mode == "ival":
        inputs = list(ISO_FIXED) + (literals(test_dir) if test_dir != "-" else [])
        pieces = ["P", "T", "Y", "M", "W", "D", "H", "S", "R", "/", "-", ":", "Z", " ", ".", ",", "\t", "\x00", "\n",
                  "x", "+", "p", "t"]
        while len(inputs) < count:
            r = rng.random()
            if r < 0.4:
                s = "P"
                for unit in "YMWD":
                    if rng.random() < 0.4:
                        s += str(rng.choice([0, 1, 2, 10, 99, rng.randint(0, 10 ** 6), 123456789012, 1234567890123])) + unit
                if rng.random() < 0.5:
                    s += "T"
                    for unit in "HMS":
                        if rng.random() < 0.5:
                            s += str(rng.choice([0, 1, 30, 59, 60, rng.randint(0, 10 ** 7)])) + unit
                inputs.append(s)
            elif r < 0.55:
                inputs.append("P%04d-%02d-%02dT%02d:%02d:%02d" % (rng.randint(0, 9999), rng.randint(0, 13),
                              rng.randint(0, 32), rng.randint(0, 25), rng.randint(0, 60), rng.randint(0, 60)))
            elif r < 0.8:
                parts = []
                if rng.random() < 0.5:
                    parts.append("R" + str(rng.choice([0, 1, 5, rng.randint(0, 10 ** 10)])))
                for _ in range(rng.randint(1, 3)):
                    k = rng.random()
                    if k < 0.35:
                        parts.append("%04d-%02d-%02dT%02d:%02d:%02dZ" % (rng.randint(1900, 2100), rng.randint(1, 12),
                                     rng.randint(1, 31), rng.randint(0, 24), rng.randint(0, 59), rng.randint(0, 59)))
                    elif k < 0.6:
                        parts.append("%04d%02d%02dT%02d%02d%02dZ" % (rng.randint(1900, 2100), rng.randint(1, 12),
                                     rng.randint(1, 31), rng.randint(0, 24), rng.randint(0, 59), rng.randint(0, 59)))
                    else:
                        parts.append(rng.choice(["P1D", "P7D", "PT1H", "P1Y2M10DT2H30M", "P1W", "PT36H"]))
                inputs.append(rng.choice(["/", "/", " ", ","]).join(parts))
            else:
                inputs.append("".join(rng.choice(pieces + list("0123456789")) for _ in range(rng.randint(0, 16))))
        for s in inputs[:count]:
            out.write("ival %s\n" % hexs(s))
        return
    inputs = literals(test_dir) if test_dir != "-" else []
    while len(inputs) < count:
        r = rng.random()
        inputs.append(rand_tokens(rng) if r < 0.75 else rand_bytes(rng))
    for s in inputs[:max(count, 0) or len(inputs)]:
        h = s.encode("utf-8").hex()
        if mode == "parse":
            out.write("parse %s\n" % h)
        elif mode == "init":
            out.write("init %s %d %s\n" % (rng.choice(ZONES), rng.choice(BASES), h))
        elif mode == "modify":
            out.write("modify %s %d %s\n" % (rng.choice(ZONES), rng.choice(BASES), h))
        else:
            raise SystemExit("unknown mode " + mode)


if __name__ == "__main__":
    main()

<?php
/*
 * Oracle vectors for timelib_strtotime() and timelib_parse_from_format(), via date_parse() and
 * date_parse_from_format() (PHP 8.5.13, timelib 2022.17). Regenerate from crates/twig-timelib with
 *   tests/vectors/regen.sh
 * Output: tests/vectors/parse.json. Inputs: tests/vectors/inputs.json (string literals from php-src's
 * ext/date/tests) plus deterministic random token strings (mt_srand fixed seed).
 *
 * Each case is a compact form of the date_parse() array: keys whose value is false or empty are omitted;
 * warnings/errors are [position, message] lists in PHP's array order (date_parse() keys them by position, so a
 * later message at the same position replaces an earlier one); "f" is the fraction as big-endian double hex.
 */
error_reporting(E_ALL);
mt_srand(20261125);

const TOKENS = [
    "now", "today", "midnight", "noon", "tomorrow", "yesterday", "ago", "next", "last", "previous", "this",
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "eight", "ninth", "tenth",
    "eleventh", "twelfth", "day", "days", "week", "weeks", "fortnight", "forthnight", "month", "months", "year",
    "years", "hour", "hours", "min", "mins", "minute", "minutes", "sec", "secs", "second", "seconds", "ms", "msec",
    "millisecond", "milliseconds", "usec", "\u{b5}s", "\u{b5}sec", "microsecond", "microseconds", "weekday",
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
    ";", "(", ")", "\u{a0}", "\u{202f}", "\0", "\n", "@1700000000.5", "@-1.25", "10:20:30.1234567890123",
    "+9223372036854775807 days +1 day",
];
const SEPS = ["", " ", " ", " ", "  ", "-", "/", ".", ",", ":", "\t"];
const ALPHABET = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", " ", ":", ".", "/", "-", "+", ",", "@", "T", "W",
    "t", "a", "m", "p", "\t", "Z", "\0", "(", "\u{a0}", ")", "j", "f", "s", "o", "n", "d", "J", "F", "M", "A", "S",
    "O", "N", "D"];

function pick(array $a) { return $a[mt_rand(0, count($a) - 1)]; }

function rand_input(): string
{
    if (mt_rand(0, 3) > 0) {
        $s = '';
        $n = mt_rand(1, 6);
        for ($k = 0; $k < $n; $k++) {
            $s .= pick(TOKENS) . pick(SEPS);
        }
        return mt_rand(0, 9) < 7 ? trim($s, ' ') : $s;
    }
    $s = '';
    $n = mt_rand(0, 14);
    for ($k = 0; $k < $n; $k++) {
        $s .= pick(ALPHABET);
    }
    return $s;
}

function compact_parse(string $in, array $r): array
{
    $c = ['in' => $in];
    foreach (['year' => 'y', 'month' => 'm', 'day' => 'd', 'hour' => 'h', 'minute' => 'i', 'second' => 's'] as $k => $short) {
        if ($r[$k] !== false) {
            $c[$short] = $r[$k];
        }
    }
    if ($r['fraction'] !== false) {
        $c['f'] = bin2hex(pack('E', $r['fraction']));
    }
    if ($r['warning_count']) {
        $c['wc'] = $r['warning_count'];
        $c['w'] = array_map(null, array_keys($r['warnings']), array_values($r['warnings']));
    }
    if ($r['error_count']) {
        $c['ec'] = $r['error_count'];
        $c['e'] = array_map(null, array_keys($r['errors']), array_values($r['errors']));
    }
    if ($r['is_localtime']) {
        $c['lt'] = true;
        foreach (['zone_type' => 'zt', 'zone' => 'z', 'is_dst' => 'dst', 'tz_abbr' => 'abbr', 'tz_id' => 'tzid'] as $k => $short) {
            if (array_key_exists($k, $r)) {
                $c[$short] = $r[$k];
            }
        }
    }
    if (isset($r['relative'])) {
        $rel = $r['relative'];
        $o = [$rel['year'], $rel['month'], $rel['day'], $rel['hour'], $rel['minute'], $rel['second']];
        $c['rel'] = $o;
        if (array_key_exists('weekday', $rel)) {
            $c['wd'] = $rel['weekday'];
        }
        if (array_key_exists('weekdays', $rel)) {
            $c['wds'] = $rel['weekdays'];
        }
        if (array_key_exists('first_day_of_month', $rel)) {
            $c['fdom'] = true;
        }
        if (array_key_exists('last_day_of_month', $rel)) {
            $c['ldom'] = true;
        }
    }
    return $c;
}

const FORMAT_PIECES = [
    ["Y", ["2024", "1999", "0001", "99", "12345", "-200"]], ["y", ["24", "99", "7", "123"]],
    ["x", ["+2024", "-0044", "12345", "+", "2024"]], ["X", ["+10000", "-1", "2024"]],
    ["m", ["01", "12", "7", "13", "00"]], ["n", ["1", "12", "07", "13"]], ["d", ["01", "31", "9", "32", "00"]],
    ["j", ["1", "31", "09", "40"]], ["D", ["Mon", "tue", "Sun", "xyz"]], ["l", ["Monday", "friday", "Funday"]],
    ["M", ["Jan", "feb", "DEC", "Foo"]], ["F", ["January", "march", "Smarch"]], ["S", ["st", "nd", "th", "xx"]],
    ["z", ["0", "59", "365", "400"]], ["H", ["00", "23", "24", "7"]], ["G", ["0", "23", "99"]],
    ["h", ["12", "01", "13", "7"]], ["g", ["1", "12", "13"]], ["A", ["AM", "PM", "am", "p.m.", "x"]],
    ["a", ["am", "pm", "A.M.", "b"]], ["i", ["00", "59", "60", "5"]], ["s", ["00", "59", "61", "7"]],
    ["u", ["123456", "1", "1234567", "x"]], ["v", ["123", "1", "1234"]], ["U", ["1700000000", "-1", "+5", "x"]],
    ["e", ["UTC", "Europe/Paris", "+02:00", "EST", "Not/Zone"]], ["T", ["CEST", "GMT+1", "Z", "-0330"]],
    ["O", ["+0200", "-05", "+14:00"]], ["P", ["+02:00", "-12:00", "+1"]], ["p", ["Z", "+00:00"]],
    ["!", [""]], ["|", [""]], ["+", ["", "trailing"]], ["*", ["abc", "x1"]], ["?", ["x", "1"]],
    ["#", [";", ":", "/", ".", ",", "-", "(", ")", "x"]], [" ", [" ", "  ", "\t", ""]], [":", [":", "x"]],
    ["/", ["/", "-"]], ["-", ["-", "/"]], [".", [".", ","]], ["\\T", ["T", "t"]], ["\\", [""]], ["Q", ["Q", "q"]],
];

$inputs = json_decode(file_get_contents(__DIR__ . '/inputs.json'), true, 512, JSON_THROW_ON_ERROR);
while (count($inputs) < 9000) {
    $inputs[] = rand_input();
}

$parse = [];
foreach ($inputs as $in) {
    $parse[] = compact_parse($in, date_parse($in));
}

$format = [];
for ($k = 0; $k < 6000; $k++) {
    $f = '';
    $s = '';
    $n = mt_rand(1, 7);
    for ($j = 0; $j < $n; $j++) {
        [$spec, $values] = pick(FORMAT_PIECES);
        $f .= $spec;
        $s .= pick($values);
    }
    if (mt_rand(0, 9) === 0) {
        $s .= pick(["x", "1", " "]);
    }
    $c = compact_parse($s, date_parse_from_format($f, $s));
    $format[] = ['fmt' => $f] + $c;
}

echo json_encode([
    'generator' => 'tests/vectors/parse.gen.php',
    'php' => PHP_VERSION,
    'timezone_version' => timezone_version_get(),
    'parse' => $parse,
    'format' => $format,
], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";

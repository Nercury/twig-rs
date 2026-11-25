<?php
/*
 * Oracle vectors for the full parse pipeline (PHP 8.5.13, timelib 2022.17). Regenerate from crates/twig-timelib
 * with tests/vectors/regen.sh. Output: tests/vectors/strtotime.json.
 *
 *  "strtotime": [zone, base, input, result]       strtotime(input, base) with date_default_timezone_set(zone);
 *                                                 result is the timestamp or false
 *                                                 (timelib_strtotime + fill_holes + update_ts)
 *  "modify":    [zone, base, input, result]       $d = new DateTime('@'.base); $d->setTimezone(new DateTimeZone(zone));
 *                                                 $d->modify(input); result is $d->format('Y-m-d H:i:s.u T P U')
 *                                                 or the exception message
 *  "construct": [zone, input, result]             new DateTime(input, new DateTimeZone(zone)) for inputs that need
 *                                                 no "now" (full date and time given); result as for modify
 * Inputs: tests/vectors/inputs.json plus deterministic random token strings and hand-picked cases.
 * Results that are not valid UTF-8 (an error message quoting a single byte of a multi-byte character) are
 * written as "hex:" followed by the bytes in hex.
 */
error_reporting(E_ALL);
mt_srand(20261126);

const TOKENS = [
    "now", "today", "midnight", "noon", "tomorrow", "yesterday", "ago", "next", "last", "previous", "this",
    "first", "second", "third", "fourth", "fifth", "sixth", "eighth", "twelfth", "day", "days", "week", "weeks",
    "fortnight", "month", "months", "year", "years", "hour", "hours", "min", "minutes", "sec", "seconds", "ms",
    "usec", "\u{b5}s", "weekday", "weekdays", "monday", "mon", "tuesday", "wed", "thursday", "fri", "saturday",
    "sun", "sundays", "january", "jan", "feb", "march", "apr", "may", "june", "jul", "aug", "sept", "oct", "nov",
    "december", "I", "IV", "XII", "first day of", "last day of", "back of", "front of", "of", "am", "pm", "a.m.",
    "T", "UTC", "GMT", "EST", "EDT", "CET", "CEST", "PST", "MSK", "IST", "Europe/Paris", "America/New_York",
    "Asia/Tokyo", "Australia/Lord_Howe", "Pacific/Chatham", "+00:00", "-05:00", "+0530", "+01", "-1", "+14:00",
    "GMT+2", "+1", "-1", "+2", "-3", "+10", "1", "2", "3", "7", "10", "12", "13", "24", "29", "30", "31", "59",
    "60", "100", "365", "1970", "2000", "2021", "2024", "2038", "0", "00", "2024-01-31", "2021-02-28",
    "2024-02-29", "2024-W05", "2024-W53-7", "2024.123", "20240131", "2024-01-31T10:11:12", "2024-03-31 02:30",
    "2024-10-27 02:30", "2024-03-10 02:30", "2024-11-03 01:30", "31.01.2024", "1/31/2024", "1/31", "10:11",
    "10:11:12", "10:11:12.5", "1011", "10am", "12:30pm", "@1700000000", "@-1.5",
];
const SEPS = ["", " ", " ", " ", "  ", "-", "/", ".", ",", ":"];
const ZONES = ["UTC", "Europe/Paris", "America/New_York", "Asia/Kolkata", "Australia/Lord_Howe", "Pacific/Chatham",
    "America/Sao_Paulo", "Africa/Casablanca", "Europe/London", "America/St_Johns", "Asia/Tehran", "Etc/GMT+5",
    "Europe/Dublin", "Pacific/Apia", "America/Santiago", "Asia/Gaza", "Antarctica/Troll", "Europe/Moscow"];
const BASES = [0, 1700000000, 1711846800, 1711846799, 1730000000, 951782400, -1000000000, 2147483647, 4102444800,
    1616893200, 1636246800, 1710054000, 1730613600, 1704067199, -62135596800, 253402300799];
const FIXED = [
    "2021-01-31 +1 month", "2024-01-31 +1 month", "last day of next month", "first day of last month",
    "+1 month", "-1 month", "last day of february", "first monday of january 2024", "last sunday of march",
    "next monday", "last friday", "this sunday", "sunday this week", "monday next week", "saturday last week",
    "+3 weekdays", "-3 weekdays", "+5 weekdays", "0 weekdays", "1 weekday ago", "tomorrow noon", "yesterday 14:00",
    "midnight", "noon", "now", "today", "+1 day +2 hours", "+90 minutes", "-1 year", "+2 weeks",
    "2024-03-31 02:30:00", "2024-10-27 02:30:00", "2024-10-27 02:30:00 CEST", "2024-10-27 02:30:00 CET",
    "2024-03-10 02:30:00", "2024-11-03 01:30:00", "2024-11-03 01:30:00 EDT", "2024-11-03 01:30:00 EST",
    "@1700000000", "@1700000000.123456", "@-1700000000.5", "@0", "1700000000", "back of 7pm", "front of 0",
    "+1 week 2 days 4 hours 2 seconds", "next year", "last month", "this month", "2024W101", "2024-W53",
    "10 September 2000 +1 week", "1 Jan", "Jan 2024", "2024 Jan", "5 days ago", "3 months ago 2 days",
    "+1000 years", "-10000 days", "+2147483647 seconds", "1 fortnight", "first sat of July 2008",
    "last wed of next month", "+1.5 hours", "12/22/78", "1978-12-22T09:30:00Z", "2008-07-01T22:35:17.02",
    "2008-07-01T22:35:17.03+08:00", "Sat, 18 Aug 2007 12:00:00 +0200", "Tue, 14 Oct 2008 12:00:00 GMT",
];

function pick(array $a) { return $a[mt_rand(0, count($a) - 1)]; }

function rand_input(): string
{
    $s = '';
    $n = mt_rand(1, 5);
    for ($k = 0; $k < $n; $k++) {
        $s .= pick(TOKENS) . pick(SEPS);
    }
    return trim($s, ' ');
}

function enc(string $s): string
{
    return preg_match('//u', $s) ? $s : 'hex:' . bin2hex($s);
}

function show(DateTime $d): string
{
    return $d->format('Y-m-d H:i:s.u T P U');
}

$inputs = array_merge(FIXED, json_decode(file_get_contents(__DIR__ . '/inputs.json'), true, 512, JSON_THROW_ON_ERROR));
while (count($inputs) < 6000) {
    $inputs[] = rand_input();
}

$strtotime = [];
$modify = [];
foreach ($inputs as $idx => $in) {
    foreach ([0, 1] as $rep) {
        $zone = $idx < count(FIXED) ? ZONES[($idx + $rep * 7) % count(ZONES)] : pick(ZONES);
        $base = $idx < count(FIXED) ? BASES[($idx + $rep * 5) % count(BASES)] : pick(BASES);
        date_default_timezone_set($zone);
        $r = @strtotime($in, $base);
        $strtotime[] = [$zone, $base, $in, $r];

        $d = new DateTime('@' . $base);
        $d->setTimezone(new DateTimeZone($zone));
        try {
            $d->modify($in);
            $modify[] = [$zone, $base, $in, show($d)];
        } catch (Throwable $e) {
            $modify[] = [$zone, $base, $in, enc(get_class($e) . ': ' . $e->getMessage())];
        }
    }
}
date_default_timezone_set('UTC');

// Wall-clock construction around DST transitions in many zones (no "now" involved: full date and time given).
$construct = [];
$zones = ZONES;
foreach (["Europe/Berlin", "America/Chicago", "Australia/Sydney", "America/Havana", "Asia/Jerusalem",
          "Europe/Lisbon", "America/Godthab", "Pacific/Auckland", "Asia/Amman", "Africa/Cairo"] as $z) {
    $zones[] = $z;
}
foreach ($zones as $z) {
    $tz = new DateTimeZone($z);
    $tr = $tz->getTransitions(0, 2240524800);
    $count = 0;
    for ($k = 1; $k < count($tr) && $count < 12; $k += max(1, intdiv(count($tr), 12))) {
        $count++;
        $t = $tr[$k]['ts'];
        $before = $tr[$k - 1]['offset'];
        $after = $tr[$k]['offset'];
        foreach ([-3601, -1800, -1, 0, 1, 1800, 3599] as $delta) {
            foreach ([$before, $after] as $off) {
                $wall = gmdate('Y-m-d H:i:s', $t + $off + $delta);
                try {
                    $construct[] = [$z, $wall, show(new DateTime($wall, $tz))];
                } catch (Throwable $e) {
                    $construct[] = [$z, $wall, enc(get_class($e) . ': ' . $e->getMessage())];
                }
            }
        }
    }
}

echo json_encode([
    'generator' => 'tests/vectors/strtotime.gen.php',
    'php' => PHP_VERSION,
    'timezone_version' => timezone_version_get(),
    'strtotime' => $strtotime,
    'modify' => $modify,
    'construct' => $construct,
], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";

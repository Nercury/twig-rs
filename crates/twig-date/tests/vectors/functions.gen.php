<?php
/*
 * Oracle vectors for the procedural date functions with a faked clock: mktime(), gmmktime(), checkdate(),
 * strtotime(), idate(), getdate(), localtime(), time(), date_parse(), date_parse_from_format() and
 * date_default_timezone_set()/get(). Regenerate with tests/vectors/regen.sh functions.
 *
 *  "mktime":     [gmt, default_tz, now_sec, args (null = omitted), result]
 *  "checkdate":  [m, d, y, result]
 *  "strtotime":  [default_tz, now_sec, input, base|null, result]
 *  "idate":      [default_tz, ts, format, result, diags]
 *  "getdate":    [default_tz, ts, getdate(ts), localtime(ts), localtime(ts, true)]
 *  "parse":      [input, date_parse(input)]
 *  "parse_from_format": [format, input, date_parse_from_format(format, input) | "Class: message"]
 *  "default_tz": [zone, set result, get after, diags]
 *  "now":        [now_sec, now_usec, time(), date('c'), mktime(0), strtotime('now'), idate('U')]
 */
require __DIR__ . '/common.inc.php';
mt_srand(20261129);

$zones = ['UTC', 'Europe/Vilnius', 'America/New_York', 'Australia/Lord_Howe', 'Asia/Kathmandu', 'America/Sao_Paulo'];
$nows = [1700000000, 0, 1711846799, 1730613600, -1000000000, 2000000000, 951825600];

$mktime = [];
$argsets = [
    [0], [12, 30], [12, 30, 45], [12, 30, 45, 6], [12, 30, 45, 6, 15], [12, 30, 45, 6, 15, 2024],
    [0, 0, 0, 1, 1, 1970], [23, 59, 59, 12, 31, 1969], [25, 61, 61, 13, 32, 2023], [-1, -1, -1, 0, 0, 2000],
    [0, 0, 0, 2, 30, 24], [0, 0, 0, 1, 1, 69], [0, 0, 0, 1, 1, 70], [0, 0, 0, 1, 1, 100], [0, 0, 0, 1, 1, 101],
    [0, 0, 0, 1, 1, -1], [0, 0, 0, 1, 1, 0], [2, 30, 0, 3, 31, 2024], [3, 30, 0, 10, 27, 2024], [1, 30, 0, 11, 3, 2024],
    [12, null, null, null, null, 2030], [12, null, 0, null, 1], [0, 0, 0, 1, 1, 99999], [0, 0, 0, 1, 1, -5000],
    [1000000, 0, 0, 1, 1, 2000],
];
foreach ([false, true] as $gmt) {
    foreach ($argsets as $args) {
        $z = pick($zones);
        $now = pick($nows);
        date_default_timezone_set($z);
        now($now);
        $r = $gmt ? gmmktime(...$args) : mktime(...$args);
        $mktime[] = [$gmt, $z, $now, $args, $r];
    }
}

$checkdate = [];
foreach ([[2, 29, 2024], [2, 29, 2023], [2, 29, 1900], [2, 29, 2000], [12, 31, 32767], [1, 1, 32768], [1, 1, 0],
    [1, 1, 1], [0, 1, 2024], [13, 1, 2024], [4, 31, 2024], [4, 30, 2024], [1, 0, 2024], [-1, 1, 2024], [1, 32, 2024]] as [$m, $d, $y]) {
    $checkdate[] = [$m, $d, $y, checkdate($m, $d, $y)];
}

$strtotime = [];
$inputs = ['now', '', 'today', 'tomorrow', '+1 day', '-1 week', 'next monday', 'last day of next month', '2024-02-29',
    '2024-02-29 12:00:00', '@1700000000', '10:00', 'noon', 'midnight', 'garbage', '1 January 2024', 'Jan 1',
    '2024-01-01 Europe/Paris', '2024-01-01T00:00:00Z', '+1 month', 'first monday of next month', '2024-03-31 02:30',
    'yesterday noon', '00:00:60', '24:00', 'Saturday', '-1 year', '+5 weekdays', '1e3', 'NOW', ' now ', "now\0"];
foreach ($inputs as $in) {
    for ($k = 0; $k < 3; $k++) {
        $z = pick($zones);
        $now = pick($nows);
        $base = $k === 2 ? pick([0, 86400 * 365, -86400, 1709251199]) : null;
        date_default_timezone_set($z);
        now($now);
        $strtotime[] = [$z, $now, enc($in), $base, $base === null ? strtotime($in) : strtotime($in, $base)];
    }
}

$idate = [];
foreach (['d', 'j', 'N', 'w', 'z', 'W', 'm', 'n', 't', 'L', 'y', 'Y', 'o', 'B', 'g', 'h', 'H', 'G', 'i', 's', 'I', 'Z', 'U',
    'x', 'D', '', 'dd', 'e'] as $f) {
    foreach ([[1700000000, 'Europe/Vilnius'], [-62198755200, 'UTC'], [1711846799, 'Europe/Vilnius'], [4102444800, 'America/New_York'],
        [-1, 'Australia/Lord_Howe'], [1000000000, 'Asia/Kathmandu']] as [$ts, $z]) {
        date_default_timezone_set($z);
        $r = idate($f, $ts);
        $idate[] = [$z, $ts, $f, $r, take_diags()];
    }
}

$getdate = [];
foreach ([[1700000000, 'Europe/Vilnius'], [0, 'UTC'], [-62198755200, 'America/New_York'], [1711846799, 'Europe/Vilnius'],
    [1730613600, 'America/New_York'], [253402300800, 'Asia/Kathmandu']] as [$ts, $z]) {
    date_default_timezone_set($z);
    $getdate[] = [$z, $ts, getdate($ts), localtime($ts), localtime($ts, true)];
}

$parse = [];
foreach (['2024-02-29 13:14:15.5', 'now', '', 'garbage', '+1 day', 'next monday', 'last day of next month', '10:00 CEST',
    '2024-01-01 Europe/Paris', '2024-01-01T00:00:00+05:30', '@1700000000', '2024-13-45', 'Feb 30 2024 25:61', '+3 weekdays',
    'first day of next month', '2024-01-01 EST EDT', 'Mon, 15 Aug 2005 15:52:01 +0000', '1 year ago', 'tomorrow noon'] as $in) {
    $parse[] = [enc($in), date_parse($in)];
}

$pff = [];
foreach ([['Y-m-d', '2024-02-29'], ['Y-m-d H:i:s', '2024-02-29 13:14:15'], ['!Y-m-d', '2024-02-30'], ['d/m/Y', '31/02/2024'],
    ['Y-m-d', 'x'], ['H:i', '25:00'], ['U', '1700000000'], ['Y-m-d e', '2024-01-01 Europe/Paris'], ['Y-m-d T', '2024-01-01 CEST'],
    ['Y-m-d P', '2024-01-01 +05:30'], ['D', 'Mon'], ['Y-m-d+', '2024-01-01 xyz'], ['Y-m-d', "2024-01-01\0"], ['', ''], ['Y', '']] as [$f, $in]) {
    try {
        $pff[] = [enc($f), enc($in), date_parse_from_format($f, $in)];
    } catch (Throwable $e) {
        $pff[] = [enc($f), enc($in), exc($e)];
    }
}

$default_tz = [];
date_default_timezone_set('UTC');
foreach (['Europe/Vilnius', 'europe/vilnius', 'Nowhere/City', '', 'UTC', 'utc', 'EST', 'CEST', '+01:00', "Asia/Tokyo\0x", 'US/Eastern'] as $z) {
    $r = date_default_timezone_set($z);
    $default_tz[] = [enc($z), $r, date_default_timezone_get(), take_diags()];
}

$nowv = [];
date_default_timezone_set('Europe/Vilnius');
foreach ([[1700000000, 123456], [0, 0], [-1, 999999], [2147483648, 1]] as [$s, $u]) {
    now($s, $u);
    $nowv[] = [$s, $u, time(), date('c'), mktime(0), strtotime('now'), idate('U'), date('Y-m-d', null), (new DateTime())->format('U.u')];
}

emit('functions.gen.php', ['mktime' => $mktime, 'checkdate' => $checkdate, 'strtotime' => $strtotime, 'idate' => $idate,
    'getdate' => $getdate, 'parse' => $parse, 'parse_from_format' => $pff, 'default_tz' => $default_tz, 'now' => $nowv,
    'version' => timezone_version_get()]);

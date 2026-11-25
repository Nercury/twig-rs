<?php
/*
 * Oracle vectors for DateTime::format(), date() and gmdate() (date_format() in php_date.c): every letter over
 * many instants (random, around DST transitions, negative and five-digit years, microseconds) and zones (ids,
 * offsets, abbreviations). Regenerate with tests/vectors/regen.sh format.
 *
 *  "letters": [zone, ts, us, out]   $d = new DateTime('@ts'); $d->setTimezone(new DateTimeZone(zone));
 *                                   $d->setMicrosecond(us); out = $d->format(FMT)
 *  "date":    [zone, ts, date(FMT), gmdate(FMT)] after date_default_timezone_set(zone)
 *  "formats": [zone, ts, us, format, out] special format strings (escapes, trailing backslash, unknown letters)
 */
require __DIR__ . '/common.inc.php';
mt_srand(20261125);

const FMT = 'd|D|j|l|N|S|w|z|W|F|m|M|n|t|L|o|X|x|Y|y|a|A|B|g|G|h|H|i|s|u|v|e|I|O|P|p|T|Z|c|r|U';

$zones = ['UTC', 'Europe/Vilnius', 'America/New_York', 'Australia/Lord_Howe', 'Asia/Kolkata', 'Pacific/Chatham',
    'America/St_Johns', 'Africa/Casablanca', 'Europe/London', 'Asia/Kathmandu', 'Antarctica/Troll', 'Pacific/Apia',
    'America/Sao_Paulo', 'Europe/Dublin', 'Pacific/Kiritimati', 'America/Adak', 'europe/paris', 'Africa/Monrovia',
    '+05:30', '-03:00', '+00:00', '-00:00', '+14:00', '-11:30', '+99:59', '-0930', '+1', 'GMT+02:00',
    'CEST', 'EST', 'PST', 'utc', 'GMT', 'z', 'BST', 'IST', 'NZDT', 'a', 'HST'];

$special = [0, -1, 1, 86399, 86400, -86400, -62167219200, -62167219201, -62198755200, -62135596800,
    253402300799, 253402300800, 100000000000, -100000000000, 1 << 40, -(1 << 40), 1 << 45, -(1 << 45),
    951782400, 951868800, 1709208000, 4107542400, -2208988800, -5000000000, 2147483647, 2147483648,
    -2147483648, -2147483649, 1000000000, 1234567890];

$transitions = [];
foreach ($zones as $z) {
    if (!str_contains($z, '/')) {
        continue;
    }
    foreach ((new DateTimeZone($z))->getTransitions(-2208988800, 2208988800) as $k => $t) {
        if ($k > 0) {
            $transitions[] = $t['ts'];
        }
    }
}

$letters = [];
$date = [];
foreach ($zones as $zone) {
    try {
        $tz = new DateTimeZone($zone);
    } catch (Throwable $e) {
        fwrite(STDERR, "skip zone $zone: " . $e->getMessage() . "\n");
        continue;
    }
    $stamps = $special;
    for ($k = 0; $k < 60; $k++) {
        $stamps[] = mt_rand(-50000000000, 50000000000);
    }
    for ($k = 0; $k < 40; $k++) {
        $stamps[] = pick($transitions) + mt_rand(-3, 3) * pick([1, 1800, 3600]);
    }
    foreach ($stamps as $ts) {
        $us = pick([0, 0, 999999, 500000, mt_rand(0, 999999), mt_rand(0, 999)]);
        $d = new DateTime('@' . $ts);
        $d->setTimezone($tz);
        $d->setMicrosecond($us);
        $letters[] = [$zone, $ts, $us, enc($d->format(FMT))];
    }
    if (str_contains($zone, '/') || $zone === 'UTC') {
        date_default_timezone_set($zone);
        foreach (array_slice($stamps, 0, 40) as $ts) {
            $date[] = [$zone, $ts, enc(date(FMT, $ts)), enc(gmdate(FMT, $ts))];
        }
        date_default_timezone_set('UTC');
    }
}

$formats = [];
$fmts = ['', '\\', 'Y\\', '\\\\', '\\Y-\\m-\\d', 'Y-m-d\\TH:i:sP', '\\\\Y', 'ÄY', 'Y年m月d日', '%Y %m',
    'b f k q C E G J K Q R V', 'x X Y y', 'DATE_ATOM', DATE_COOKIE, DATE_RFC7231, DATE_RFC3339_EXTENDED,
    DATE_ISO8601_EXPANDED, "Y\0m", "\\\0Y", '\\a\\b\\c', 'jS F Y', 'jS', 'l \\t\\h\\e jS', 'U.u', 'v', 'B'];
foreach (['UTC', 'Europe/Vilnius', '+05:30', 'EST', 'Pacific/Chatham'] as $zone) {
    foreach ([0, 1709528767, -62198755200, 253402300800, 1700000000] as $ts) {
        foreach ($fmts as $f) {
            $us = 123456;
            $d = new DateTime('@' . $ts);
            $d->setTimezone(new DateTimeZone($zone));
            $d->setMicrosecond($us);
            $formats[] = [$zone, $ts, $us, enc($f), enc($d->format($f))];
        }
    }
}
// Day suffixes and 12-hour clock over a month of days and every hour.
for ($day = 1; $day <= 31; $day++) {
    $d = new DateTime(sprintf('2024-01-%02d %02d:30:00', $day, $day % 24), new DateTimeZone('UTC'));
    $formats[] = ['UTC', $d->getTimestamp(), 0, 'jS g h a A G H', $d->format('jS g h a A G H')];
}

emit('format.gen.php', ['fmt' => FMT, 'letters' => $letters, 'date' => $date, 'formats' => $formats]);

<?php
/*
 * Oracle vectors for DateTimeZone (timezone_initialize(), getName(), getOffset(), getLocation(),
 * getTransitions(), listIdentifiers(), listAbbreviations(), comparisons) and timezone_open() /
 * timezone_name_from_abbr(). Regenerate with tests/vectors/regen.sh timezone.
 *
 *  "construct": [name, getName() | "Class: message", (array) $tz | null, timezone_open() name | false, diags]
 *  "offset":    [zone, ts, getOffset(new DateTime('@ts'))]
 *  "location":  [zone, getLocation()]
 *  "transitions": [zone, begin, end, getTransitions(begin, end)]   (begin/end null = default arguments)
 *  "identifiers": [group, country|null, listIdentifiers() | "Class: message"]
 *  "abbreviations": listAbbreviations()
 *  "from_abbr": [abbr, offset, isdst, timezone_name_from_abbr()]
 *  "compare":   [a, b, a == b | "Class: message", a < b | "Class: message"]
 */
require __DIR__ . '/common.inc.php';

$names = ['UTC', 'utc', 'Utc', 'Europe/Vilnius', 'europe/vilnius', 'EUROPE/VILNIUS', 'America/Argentina/Buenos_Aires',
    'America/Buenos_Aires', 'US/Eastern', 'Etc/GMT+5', 'GMT', 'Z', 'z', 'CEST', 'cest', 'EST', 'PST', 'A', 'J',
    'Europe/Nowhere', 'Nowhere', '', ' ', 'UTC ', ' UTC', '(UTC)', 'Europe/Paris Asia/Tokyo', '+00:00', '-00:00',
    '+05:30', '-0930', '+5', '+05', '+0530', '+99:59', '+100:00', '-100:00', '+99:60', '+9999', '+05:30:15',
    '+053015', 'GMT+02:00', 'GMT-3', 'UTC+2', '+', '-', '+a', '++01:00', '1', 'Europe/Vilnius1', 'EST5EDT', 'CST6CDT',
    'localtime', 'Factory', 'ÄÖ', "UTC\0", 'Antarctica/Troll', 'Pacific/Chatham', 'Asia/Kathmandu', 'Africa/Casablanca'];
$construct = [];
foreach ($names as $n) {
    try {
        $tz = new DateTimeZone($n);
        $row = [enc($n), enc($tz->getName()), (array) $tz];
    } catch (Throwable $e) {
        $row = [enc($n), exc($e), null];
    }
    try {
        $o = timezone_open($n);
        $row[] = $o === false ? false : enc($o->getName());
    } catch (Throwable $e) {
        $row[] = exc($e);
    }
    $row[] = take_diags();
    $construct[] = $row;
}

$zones = ['UTC', 'Europe/Vilnius', 'America/New_York', 'Australia/Lord_Howe', '+05:30', '-03:00', 'EST', 'CEST',
    'Asia/Kathmandu', 'Africa/Casablanca'];
$offset = [];
foreach ($zones as $z) {
    foreach ([0, 1700000000, 1711846800, 1711846799, 1730613599, 1730613600, -5000000000, 4000000000] as $ts) {
        $offset[] = [$z, $ts, (new DateTimeZone($z))->getOffset(new DateTime('@' . $ts))];
    }
}

$location = [];
foreach (['UTC', 'Europe/Vilnius', 'America/New_York', 'Antarctica/Troll', 'US/Eastern', 'Etc/GMT+5', 'europe/paris',
    '+05:30', 'EST', 'Factory'] as $z) {
    $location[] = [$z, (new DateTimeZone($z))->getLocation()];
}

$transitions = [];
foreach ([['Europe/Vilnius', null, null], ['Europe/Vilnius', 1700000000, 1800000000], ['America/New_York', 1600000000, 1700000000],
    ['UTC', null, null], ['UTC', 0, 100], ['Asia/Kolkata', null, null], ['Australia/Lord_Howe', 2000000000, 2100000000],
    ['America/Sao_Paulo', 1500000000, 1800000000], ['Europe/London', -5000000000, -4000000000], ['Africa/Casablanca', 1900000000, 2000000000],
    ['Europe/Vilnius', 2147483647, 2200000000], ['America/New_York', PHP_INT_MIN, -2700000000], ['Pacific/Apia', 1200000000, 1400000000],
    ['+05:30', null, null], ['EST', 0, 1]] as [$z, $b, $e]) {
    $tz = new DateTimeZone($z);
    $t = $b === null ? $tz->getTransitions() : $tz->getTransitions($b, $e);
    $transitions[] = [$z, $b, $e, $t];
}

$identifiers = [];
foreach ([[DateTimeZone::ALL, null], [DateTimeZone::ALL_WITH_BC, null], [DateTimeZone::EUROPE, null], [DateTimeZone::UTC, null],
    [DateTimeZone::AFRICA | DateTimeZone::INDIAN, null], [0, null], [DateTimeZone::PER_COUNTRY, 'LT'], [DateTimeZone::PER_COUNTRY, 'US'],
    [DateTimeZone::PER_COUNTRY, 'us'], [DateTimeZone::PER_COUNTRY, 'XX'], [DateTimeZone::PER_COUNTRY, 'USA'],
    [DateTimeZone::PER_COUNTRY, null], [DateTimeZone::ANTARCTICA, 'LT'], [-1, null], [DateTimeZone::ARCTIC | DateTimeZone::ATLANTIC, null]] as [$g, $cc]) {
    try {
        $identifiers[] = [$g, $cc, DateTimeZone::listIdentifiers($g, $cc)];
    } catch (Throwable $e) {
        $identifiers[] = [$g, $cc, exc($e)];
    }
}

$from_abbr = [];
foreach ([['EST', -1, -1], ['est', -1, -1], ['CEST', -1, -1], ['', 3600, 0], ['', 3600, 1], ['', -18000, 0], ['', 12345, 0],
    ['XYZ', -1, -1], ['XYZ', 7200, 0], ['UTC', -1, -1], ['GMT', -1, -1], ['A', -1, -1], ['EST', 3600, -1], ['EDT', -1, -1],
    ['IST', 19800, 0], ['IST', 3600, 1], ['', 19800, 0], ["EST\0x", -1, -1]] as [$a, $o, $d]) {
    $from_abbr[] = [enc($a), $o, $d, timezone_name_from_abbr($a, $o, $d)];
}

$compare = [];
foreach ([['UTC', 'UTC'], ['UTC', 'utc'], ['UTC', '+00:00'], ['Europe/Vilnius', 'europe/vilnius'], ['+01:00', '+01:00'],
    ['+01:00', '+02:00'], ['CEST', 'CEST'], ['CEST', 'CET'], ['EST', 'est'], ['Europe/Vilnius', 'Europe/Riga']] as [$a, $b]) {
    $ta = new DateTimeZone($a);
    $tb = new DateTimeZone($b);
    $row = [$a, $b];
    foreach (['eq', 'lt'] as $op) {
        try {
            $row[] = $op === 'eq' ? $ta == $tb : $ta < $tb;
        } catch (Throwable $e) {
            $row[] = exc($e);
        }
    }
    $compare[] = $row;
}

emit('timezone.gen.php', ['construct' => $construct, 'offset' => $offset, 'location' => $location,
    'transitions' => $transitions, 'identifiers' => $identifiers, 'abbreviations' => DateTimeZone::listAbbreviations(),
    'from_abbr' => $from_abbr, 'compare' => $compare]);

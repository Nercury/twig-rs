<?php
/*
 * Oracle vectors for the embedded timezone database and parse_tz.c / parse_posix.c (PHP 8.5.13, timelib 2022.17,
 * timezonedb 2026.6). Regenerate from crates/twig-timelib with tests/vectors/regen.sh.
 * Output: tests/vectors/zones.json.
 *
 *  "zones":     [name, in_default_list, country_code, latitude, longitude, comments]   all 598 entries
 *               (timezone_identifiers_list(ALL_WITH_BC), DateTimeZone::getLocation()); the location fields are
 *               null when new DateTimeZone(name) is not an identifier zone (names such as "EST" or "GMT" are
 *               parsed as abbreviations first)
 *  "local":     [name, ts, [Y, n, j, G, i, s, Z, I, T, N, z, W, o, t, L]]
 *               (new DateTime('@ts'))->setTimezone(zone)->format(...) for every zone: transitions (from
 *               getTransitions() between 1900 and 2100, +-1 s), timestamps around DST changes far in the
 *               future/past and random 64-bit timestamps
 *  "transitions": [name, begin, end, [[ts, offset, isdst, abbr], ...]] getTransitions(begin, end) for a sample
 *  "abbreviations": timezone_abbreviations_list() flattened: [abbr, dst, offset, timezone_id]
 *  "from_abbr": [abbr, offset, isdst, result]   timezone_name_from_abbr()
 */
error_reporting(E_ALL);
mt_srand(20261128);

$all = timezone_identifiers_list(DateTimeZone::ALL_WITH_BC);
$default = array_flip(timezone_identifiers_list());

$zones = [];
foreach ($all as $name) {
    $loc = (new DateTimeZone($name))->getLocation();
    if ($loc === false) {
        $zones[] = [$name, isset($default[$name]), null, null, null, null];
    } else {
        $zones[] = [$name, isset($default[$name]), $loc['country_code'], $loc['latitude'], $loc['longitude'], $loc['comments']];
    }
}

$fmt = function (DateTime $d): array {
    return [(int) $d->format('Y'), (int) $d->format('n'), (int) $d->format('j'), (int) $d->format('G'),
        (int) $d->format('i'), (int) $d->format('s'), (int) $d->format('Z'), (int) $d->format('I'),
        $d->format('T'), (int) $d->format('N'), (int) $d->format('z'), (int) $d->format('W'),
        (int) $d->format('o'), (int) $d->format('t'), (int) $d->format('L')];
};

$local = [];
foreach ($all as $name) {
    $tz = new DateTimeZone($name);
    $tr = $tz->getTransitions(-2208988800, 4102444800) ?: [];
    $list = [];
    $n = count($tr);
    $step = max(1, intdiv($n, 8));
    for ($k = 1; $k < $n; $k += $step) {
        foreach ([-1, 0, 1] as $delta) {
            $list[] = $tr[$k]['ts'] + $delta;
        }
    }
    foreach ([2051222400, 2067033600, 4133894400, 4149705600, 7258118400, -2524521600, -1000000000, 0] as $base) {
        $list[] = $base + mt_rand(0, 300 * 86400);
    }
    for ($k = 0; $k < 4; $k++) {
        $list[] = mt_rand(-62135596800, 253402300799);
    }
    $list[] = mt_rand(-PHP_INT_MAX >> 8, PHP_INT_MAX >> 8);
    foreach ($list as $ts) {
        $d = new DateTime('@' . $ts);
        $d->setTimezone($tz);
        $local[] = [$name, $ts, $fmt($d)];
    }
}

$transitions = [];
foreach (["Europe/Paris", "America/New_York", "Australia/Lord_Howe", "Pacific/Chatham", "Africa/Casablanca",
          "Asia/Tehran", "America/Santiago", "Europe/Dublin", "Antarctica/Troll", "UTC", "Asia/Kolkata",
          "America/Sao_Paulo", "Pacific/Apia", "Asia/Gaza", "Europe/Moscow", "America/St_Johns"] as $name) {
    foreach ([[PHP_INT_MIN, 2147483647], [946684800, 2524608000], [2000000000, 2100000000], [1700000000, 1700000000]] as [$b, $e]) {
        $out = [];
        foreach ((new DateTimeZone($name))->getTransitions($b, $e) as $t) {
            $out[] = [$t['ts'], $t['offset'], $t['isdst'], $t['abbr']];
        }
        $transitions[] = [$name, $b, $e, $out];
    }
}

$abbreviations = [];
foreach (timezone_abbreviations_list() as $abbr => $entries) {
    foreach ($entries as $e) {
        $abbreviations[] = [$abbr, $e['dst'], $e['offset'], $e['timezone_id']];
    }
}

$from_abbr = [];
foreach (["CEST", "cet", "EST", "est", "UTC", "gmt", "Z", "a", "BST", "IST", "xyz", "", "PDT", "ACDT", "nzdt"] as $abbr) {
    foreach ([-1, 0, 3600, -18000, 7200, 37800, 19800, -14400, 1] as $off) {
        foreach ([-1, 0, 1] as $dst) {
            $from_abbr[] = [$abbr, $off, $dst, timezone_name_from_abbr($abbr, $off, $dst)];
        }
    }
}

echo json_encode([
    'generator' => 'tests/vectors/zones.gen.php',
    'php' => PHP_VERSION,
    'timezone_version' => timezone_version_get(),
    'zones' => $zones,
    'local' => $local,
    'transitions' => $transitions,
    'abbreviations' => $abbreviations,
    'from_abbr' => $from_abbr,
], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR | JSON_PRESERVE_ZERO_FRACTION), "\n";

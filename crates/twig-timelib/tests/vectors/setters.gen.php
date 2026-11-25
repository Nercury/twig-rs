<?php
/*
 * Oracle vectors for DateTime::setDate(), setTime(), setISODate() and setTimestamp() (PHP 8.5.13, timelib 2022.17):
 * local wall-clock fields to timestamp (timelib_update_ts(), incl. DST gaps and overlaps) and back. Regenerate
 * from crates/twig-timelib with tests/vectors/regen.sh. Output: tests/vectors/setters.json.
 *
 *  "set": [zone, ts, op, [a, b, c, d], result]
 *         $d = new DateTime('@ts'); $d->setTimezone(new DateTimeZone(zone)); then
 *         op "date": setDate(a, b, c); "time": setTime(a, b, c, d); "iso": setISODate(a, b, c); "ts": setTimestamp(a)
 *         result: $d->format('Y-m-d H:i:s.u T P U')
 * Half of the cases sit within hours of real transitions of the zone (getTransitions()), the rest use wide random
 * field values (out-of-range months, days, hours, ... that timelib_do_normalize() carries over).
 */
error_reporting(E_ALL);
mt_srand(20261130);

const ZONES = ["UTC", "Europe/Paris", "America/New_York", "Australia/Lord_Howe", "Pacific/Chatham", "Asia/Kolkata",
    "America/Sao_Paulo", "Europe/London", "America/St_Johns", "Africa/Casablanca", "Pacific/Apia", "Antarctica/Troll",
    "America/Havana", "Asia/Tehran", "Europe/Dublin", "America/Santiago", "+05:30", "-03:00", "CEST", "EST"];

function pick(array $a) { return $a[mt_rand(0, count($a) - 1)]; }

$transitions = [];
foreach (ZONES as $z) {
    if (!str_contains($z, '/')) {
        continue;
    }
    foreach ((new DateTimeZone($z))->getTransitions(946684800, 1893456000) as $k => $t) {
        if ($k > 0) {
            $transitions[] = [$z, $t['ts']];
        }
    }
}

$set = [];
for ($k = 0; $k < 6000; $k++) {
    $near = mt_rand(0, 1) === 1;
    if ($near) {
        [$zone, $t] = pick($transitions);
        $ts = $t + pick([-86400, 0, 0, 86400]) + mt_rand(-14400, 14400);
    } else {
        $zone = pick(ZONES);
        $ts = mt_rand(-5000000000, 5000000000);
    }
    $op = pick(['date', 'time', 'time', 'iso', 'ts']);
    if ($op === 'date') {
        if ($near) {
            $local = (new DateTime('@' . $t))->setTimezone(new DateTimeZone($zone));
            $v = [(int) $local->format('Y'), (int) $local->format('n'), (int) $local->format('j') + mt_rand(-1, 1), 0];
        } else {
            $v = [mt_rand(-3000, 5000), mt_rand(-30, 30), mt_rand(-400, 400), 0];
        }
    } elseif ($op === 'time') {
        $v = $near ? [mt_rand(0, 4), pick([0, 15, 30, 45, 59]), pick([0, 30, 59]), pick([0, 0, 123456])]
            : [mt_rand(-100, 100), mt_rand(-10000, 10000), mt_rand(-100000, 100000), mt_rand(-3000000, 3000000)];
    } elseif ($op === 'iso') {
        $v = $near ? [(int) gmdate('o', $t), (int) gmdate('W', $t), mt_rand(1, 7), 0]
            : [mt_rand(-3000, 5000), mt_rand(-60, 60), mt_rand(-10, 10), 0];
    } else {
        $v = [$near ? $t + mt_rand(-7200, 7200) : mt_rand(-PHP_INT_MAX >> 20, PHP_INT_MAX >> 20), 0, 0, 0];
    }
    $d = new DateTime('@' . $ts);
    $d->setTimezone(new DateTimeZone($zone));
    switch ($op) {
        case 'date': $d->setDate($v[0], $v[1], $v[2]); break;
        case 'time': $d->setTime($v[0], $v[1], $v[2], $v[3]); break;
        case 'iso': $d->setISODate($v[0], $v[1], $v[2]); break;
        case 'ts': $d->setTimestamp($v[0]); break;
    }
    $set[] = [$zone, $ts, $op, $v, $d->format('Y-m-d H:i:s.u T P U')];
}

echo json_encode([
    'generator' => 'tests/vectors/setters.gen.php',
    'php' => PHP_VERSION,
    'timezone_version' => timezone_version_get(),
    'set' => $set,
], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";

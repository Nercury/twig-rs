<?php
/*
 * Oracle vectors for DateTime::diff() (timelib_diff()), DateInterval::format() of the result and
 * DateTime::add() / sub() with diff results (civil), ISO 8601 intervals (wall clock) and
 * createFromDateString() intervals. Regenerate with tests/vectors/regen.sh diff.
 *
 *  "diff":   [zone_a, ts_a, us_a, zone_b, ts_b, us_b, diff(FMT), diff(absolute)(FMT)]
 *  "addsub": [zone, ts, us, interval_kind, interval_spec, add_result, sub_result]
 *            kind "iso": new DateInterval(spec); "str": DateInterval::createFromDateString(spec);
 *            "diff": spec = [zone_b, ts_b] and the interval is $base->diff(that date)
 */
require __DIR__ . '/common.inc.php';
mt_srand(20261128);

const FMT = '%Y|%y|%M|%m|%D|%d|%H|%h|%I|%i|%S|%s|%F|%f|%a|%R|%r|%%';

$zones = ['UTC', 'Europe/Vilnius', 'America/New_York', 'Australia/Lord_Howe', 'Asia/Kathmandu', '+05:30',
    '-03:00', 'EST', 'CEST', 'America/Sao_Paulo', 'Europe/London', 'Pacific/Chatham'];

function date_at(string $zone, int $ts, int $us): DateTime
{
    $d = new DateTime('@' . $ts);
    $d->setTimezone(new DateTimeZone($zone));
    $d->setMicrosecond($us);
    return $d;
}

$transitions = [];
foreach (['Europe/Vilnius', 'America/New_York', 'Australia/Lord_Howe', 'Europe/London'] as $z) {
    foreach ((new DateTimeZone($z))->getTransitions(1400000000, 1900000000) as $k => $t) {
        if ($k > 0) {
            $transitions[] = [$z, $t['ts']];
        }
    }
}

$pairs = [];
for ($k = 0; $k < 700; $k++) {
    $kind = mt_rand(0, 4);
    if ($kind === 0) {
        // same zone, random
        $z = pick($zones);
        $pairs[] = [$z, mt_rand(-3000000000, 4000000000), mt_rand(0, 999999), $z, mt_rand(-3000000000, 4000000000), mt_rand(0, 999999)];
    } elseif ($kind === 1) {
        // different zones
        $pairs[] = [pick($zones), mt_rand(0, 2000000000), pick([0, mt_rand(0, 999999)]), pick($zones), mt_rand(0, 2000000000), pick([0, mt_rand(0, 999999)])];
    } elseif ($kind === 2) {
        // around a DST transition, same zone
        [$z, $t] = pick($transitions);
        $pairs[] = [$z, $t + mt_rand(-90000, 90000), 0, $z, $t + mt_rand(-90000, 90000), 0];
    } elseif ($kind === 3) {
        // close instants
        $z = pick($zones);
        $a = mt_rand(0, 2000000000);
        $pairs[] = [$z, $a, mt_rand(0, 999999), $z, $a + mt_rand(-200000, 200000), mt_rand(0, 999999)];
    } else {
        // month ends and leap days at midnight (UTC)
        $a = (new DateTime(sprintf('%04d-%02d-%02d', mt_rand(1999, 2030), mt_rand(1, 12), pick([1, 28, 29, 30, 31])), new DateTimeZone('UTC')))->getTimestamp();
        $b = (new DateTime(sprintf('%04d-%02d-%02d', mt_rand(1999, 2030), mt_rand(1, 12), pick([1, 28, 29, 30, 31])), new DateTimeZone('UTC')))->getTimestamp();
        $z = pick(['UTC', 'Europe/Vilnius', 'America/New_York']);
        $pairs[] = [$z, $a, 0, $z, $b, 0];
    }
}

$diff = [];
foreach ($pairs as [$za, $ta, $ua, $zb, $tb, $ub]) {
    $a = date_at($za, $ta, $ua);
    $b = date_at($zb, $tb, $ub);
    $diff[] = [$za, $ta, $ua, $zb, $tb, $ub, $a->diff($b)->format(FMT), $a->diff($b, true)->format(FMT)];
}

$isos = ['P1D', 'PT1H', 'P1M', 'P1Y', 'P1Y2M3DT4H5M6S', 'PT36H', 'P2W', 'PT0S', 'P0D', 'PT90M', 'P1M1D',
    'P0001-02-03T04:05:06', 'PT1S', 'P10Y', 'P400D', 'PT8760H'];
$strs = ['+1 day', '-1 day', '+1 month', 'next monday', 'last day of next month', '+1 weekday', '-2 weekdays',
    '+90 minutes', '1 year ago', 'first day of next month', '+1 week 2 days 3 hours', '+0 day', 'tomorrow',
    'last sunday of next month', 'saturday'];
$addsub = [];
for ($k = 0; $k < 600; $k++) {
    $z = pick($zones);
    if (mt_rand(0, 2) === 0 && $z !== '+05:30') {
        [$tz, $t] = pick($transitions);
        $z = $tz;
        $ts = $t + mt_rand(-90000, 90000);
    } else {
        $ts = mt_rand(-1000000000, 3000000000);
    }
    $us = pick([0, mt_rand(0, 999999)]);
    $kind = pick(['iso', 'iso', 'str', 'diff']);
    if ($kind === 'iso') {
        $spec = pick($isos);
        $iv = new DateInterval($spec);
        if (mt_rand(0, 3) === 0) {
            $iv->invert = 1;
            $spec .= '/invert';
        }
    } elseif ($kind === 'str') {
        $spec = pick($strs);
        $iv = DateInterval::createFromDateString($spec);
    } else {
        $spec = [pick($zones), $ts + mt_rand(-100000000, 100000000)];
        $iv = date_at($z, $ts, $us)->diff(date_at($spec[0], $spec[1], 0));
    }
    $add = date_at($z, $ts, $us);
    try {
        $add->add($iv);
        $ra = enc(show($add));
    } catch (Throwable $e) {
        $ra = exc($e);
    }
    $sub = date_at($z, $ts, $us);
    try {
        $sub->sub($iv);
        $rs = enc(show($sub));
    } catch (Throwable $e) {
        $rs = exc($e);
    }
    $addsub[] = [$z, $ts, $us, $kind, $spec, $ra, $rs];
}

emit('diff.gen.php', ['fmt' => FMT, 'diff' => $diff, 'addsub' => $addsub]);

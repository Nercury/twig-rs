<?php
/*
 * Oracle vectors for timelib_diff(), timelib_add()/timelib_sub() (civil) and timelib_add_wall()/sub_wall()
 * (PHP 8.5.13, timelib 2022.17). Regenerate from crates/twig-timelib with tests/vectors/regen.sh.
 * Output: tests/vectors/interval.json.
 *
 *  "diff": [zone1, ts1, us1, zone2, ts2, us2, [y, m, d, h, i, s, f(us), invert, days]]
 *          (new DateTime('@ts.us'))->setTimezone(zone) for both, then $a->diff($b). A zone written "+hh:mm" or an
 *          abbreviation gives an offset/abbreviation DateTimeZone.
 *  "add":  [zone, ts, us, kind, spec, op, result]
 *          kind "wall": new DateInterval(spec) (PHP_DATE_WALL); kind "civil": DateInterval::createFromDateString(spec)
 *          (PHP_DATE_CIVIL); op "add" or "sub"; result is format('Y-m-d H:i:s.u T P U') or the warning/exception.
 *          For "wall" the interval fields are also recorded: [y, m, d, h, i, s, f, invert].
 */
error_reporting(E_ALL);
mt_srand(20261127);

const ZONES = ["UTC", "Europe/Paris", "America/New_York", "Australia/Lord_Howe", "Pacific/Chatham", "Asia/Kolkata",
    "America/Sao_Paulo", "Europe/London", "America/St_Johns", "Africa/Casablanca", "+05:30", "-03:00", "CEST", "EST"];
const SPECS = ["P1D", "P1M", "P1Y", "PT1H", "PT1M", "PT1S", "P1W", "P1Y2M3DT4H5M6S", "PT36H", "P31D", "PT24H",
    "P2M", "P0D", "PT0S", "P10Y", "PT90M", "P1DT1S", "PT86399S", "P400Y", "PT1000000S"];
const RELS = ["+1 day", "+1 month", "-1 month", "+1 year", "+24 hours", "next monday", "last day of next month",
    "first day of this month", "+3 weekdays", "-2 weekdays", "1 week ago", "+90 minutes", "+1 day +1 second",
    "+1 month -1 day", "last friday of next month", "+1 hour 30 minutes", "yesterday", "tomorrow", "+500 ms",
    "-1500 msec"];
const BASES = [0, 1700000000, 1711846800, 1711843200, 1711850400, 1729990800, 1729994400, 1729987200, 951782400,
    -1000000000, 1704067199, 1706659200, 1709164800, 1710054000, 1730613600, 1616893200, 1636246800];

function pick(array $a) { return $a[mt_rand(0, count($a) - 1)]; }

function make(string $zone, int $ts, int $us): DateTime
{
    $d = DateTime::createFromFormat('U.u', sprintf('%d.%06d', $ts, $us));
    $d->setTimezone(new DateTimeZone($zone));
    return $d;
}

function show(DateTime $d): string
{
    return $d->format('Y-m-d H:i:s.u T P U');
}

$diff = [];
for ($k = 0; $k < 4000; $k++) {
    $z1 = pick(ZONES);
    $z2 = mt_rand(0, 2) === 0 ? pick(ZONES) : $z1;
    $t1 = pick(BASES) + (mt_rand(0, 3) === 0 ? 0 : mt_rand(-200000000, 200000000));
    $t2 = mt_rand(0, 3) === 0 ? $t1 + pick([3600, -3600, 86400, -86400, 82800, 90000, 1, -1, 0]) : pick(BASES) + mt_rand(-100000000, 100000000);
    $u1 = mt_rand(0, 1) ? 0 : mt_rand(0, 999999);
    $u2 = mt_rand(0, 1) ? 0 : mt_rand(0, 999999);
    $i = make($z1, $t1, $u1)->diff(make($z2, $t2, $u2));
    $diff[] = [$z1, $t1, $u1, $z2, $t2, $u2, [$i->y, $i->m, $i->d, $i->h, $i->i, $i->s, (int) round($i->f * 1000000), $i->invert, $i->days]];
}

$add = [];
for ($k = 0; $k < 4000; $k++) {
    $zone = pick(ZONES);
    $ts = pick(BASES) + (mt_rand(0, 2) === 0 ? 0 : mt_rand(-5000000, 5000000));
    $us = mt_rand(0, 2) ? 0 : mt_rand(0, 999999);
    $op = mt_rand(0, 1) ? 'add' : 'sub';
    if (mt_rand(0, 1)) {
        $spec = pick(SPECS);
        $iv = new DateInterval($spec);
        if (mt_rand(0, 3) === 0) {
            $iv->invert = 1;
        }
        if (mt_rand(0, 3) === 0) {
            $iv->f = mt_rand(0, 999999) / 1000000;
        }
        $kind = 'wall';
        $fields = [$iv->y, $iv->m, $iv->d, $iv->h, $iv->i, $iv->s, (int) round($iv->f * 1000000), $iv->invert];
    } else {
        $spec = pick(RELS);
        $iv = DateInterval::createFromDateString($spec);
        $kind = 'civil';
        $fields = null;
    }
    $d = make($zone, $ts, $us);
    try {
        set_error_handler(function ($no, $str) { throw new ErrorException($str); });
        $d->$op($iv);
        $res = show($d);
    } catch (Throwable $e) {
        $res = get_class($e) . ': ' . $e->getMessage();
    } finally {
        restore_error_handler();
    }
    $add[] = [$zone, $ts, $us, $kind, $spec, $op, $fields, $res];
}

echo json_encode([
    'generator' => 'tests/vectors/interval.gen.php',
    'php' => PHP_VERSION,
    'timezone_version' => timezone_version_get(),
    'diff' => $diff,
    'add' => $add,
], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";

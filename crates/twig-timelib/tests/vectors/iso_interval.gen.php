<?php
/*
 * Oracle vectors for timelib_strtointerval() via new DateInterval($spec) (PHP 8.5.13, timelib 2022.17).
 * Regenerate from crates/twig-timelib with tests/vectors/regen.sh. Output: tests/vectors/iso_interval.json.
 *
 *  "iso": [spec, result]   result is [y, m, d, h, i, s, f(us), invert, days] (days false when unknown) or
 *                          "Class: message" for the exception (date_interval_initialize() in php_date.c:
 *                          "Unknown or bad format (%s)" / "Failed to parse interval (%s)").
 * Inputs: hand-picked ISO 8601 durations, combined representations, start/end forms, recurrences and broken
 * strings, plus deterministic random ones (mt_srand fixed seed). Strings never contain NUL bytes (PHP's message
 * would cut them off).
 */
error_reporting(E_ALL);
mt_srand(20261129);

const FIXED = ["", " ", "  ", "P", "PT", "P1Y", "P1Y2M3DT4H5M6S", "PT36H", "P2W", "P1W2D", "P0D", "PT0S",
    "R4/2012-07-01T00:00:00Z/P7D", "2008-03-01T13:00:00Z/P1Y2M10DT2H30M", "R5/2008-03-01T13:00:00Z/P1Y2M10DT2H30M",
    "2008-03-01T13:00:00Z/2009-05-11T15:30:00Z", "2009-05-11T15:30:00Z/2008-03-01T13:00:00Z",
    "20080301T130000Z/20090511T153000Z", "P0001-02-03T04:05:06", "P1Y2M3DT4H5M6", "P-1D", "p1d", "PT1M", "P1M",
    "P1DT", "P 1D", "P1D ", " P1D", "R-1/2012-07-01T00:00:00Z/P7D", "20120701T000000Z", "20120701T000000Z/P1D",
    "P1234567890123D", "P123456789012D", "P999999999999Y", "R1234567890", "P1Y1Y", "PT1H1H", "P1D/P2D",
    "P9999-12-31T24:59:59", "P0000-00-00T00:00:00", "P2024-13-32T25:60:60", "xP1D", "P1X", "\tP1D\t", "P1Y,P2Y",
    "P.5Y", "P1.5Y", "+P1D", "R5", "R5/", "/P1D", "P1W1D", "P1MT1M", "PT1H2M3S", "P1Y2M", "PT24H", "P366D",
    "2024-02-29T00:00:00Z/2025-02-28T00:00:00Z", "2024-01-31T00:00:00Z/2024-03-01T12:00:00Z",
    "2024-03-31T02:30:00Z/2024-10-27T02:30:00Z", "2024-12-31T23:59:59Z/2024-01-01T00:00:00Z"];
const PIECES = ["P", "T", "Y", "M", "W", "D", "H", "S", "R", "/", "-", ":", "Z", " ", ".", ",", "\t", "\n", "x", "+",
    "p", "t", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

function pick(array $a) { return $a[mt_rand(0, count($a) - 1)]; }

function rand_spec(): string
{
    $r = mt_rand(0, 99);
    if ($r < 40) {
        $s = 'P';
        foreach (['Y', 'M', 'W', 'D'] as $u) {
            if (mt_rand(0, 9) < 4) {
                $s .= pick([0, 1, 2, 10, 99, mt_rand(0, 1000000), 123456789012]) . $u;
            }
        }
        if (mt_rand(0, 1)) {
            $s .= 'T';
            foreach (['H', 'M', 'S'] as $u) {
                if (mt_rand(0, 1)) {
                    $s .= pick([0, 1, 30, 59, 60, mt_rand(0, 10000000)]) . $u;
                }
            }
        }
        return $s;
    }
    if ($r < 55) {
        return sprintf('P%04d-%02d-%02dT%02d:%02d:%02d', mt_rand(0, 9999), mt_rand(0, 13), mt_rand(0, 32),
            mt_rand(0, 25), mt_rand(0, 60), mt_rand(0, 60));
    }
    if ($r < 80) {
        $parts = [];
        if (mt_rand(0, 1)) {
            $parts[] = 'R' . pick([0, 1, 5, mt_rand(0, 2000000000)]);
        }
        $n = mt_rand(1, 3);
        for ($k = 0; $k < $n; $k++) {
            $kind = mt_rand(0, 99);
            if ($kind < 35) {
                $parts[] = sprintf('%04d-%02d-%02dT%02d:%02d:%02dZ', mt_rand(1900, 2100), mt_rand(1, 12),
                    mt_rand(1, 31), mt_rand(0, 24), mt_rand(0, 59), mt_rand(0, 59));
            } elseif ($kind < 60) {
                $parts[] = sprintf('%04d%02d%02dT%02d%02d%02dZ', mt_rand(1900, 2100), mt_rand(1, 12), mt_rand(1, 31),
                    mt_rand(0, 24), mt_rand(0, 59), mt_rand(0, 59));
            } else {
                $parts[] = pick(['P1D', 'P7D', 'PT1H', 'P1Y2M10DT2H30M', 'P1W', 'PT36H']);
            }
        }
        return implode(pick(['/', '/', ' ', ',']), $parts);
    }
    $s = '';
    $n = mt_rand(0, 16);
    for ($k = 0; $k < $n; $k++) {
        $s .= pick(PIECES);
    }
    return $s;
}

$inputs = FIXED;
while (count($inputs) < 5000) {
    $inputs[] = rand_spec();
}

$iso = [];
foreach ($inputs as $spec) {
    try {
        $i = new DateInterval($spec);
        $iso[] = [$spec, [$i->y, $i->m, $i->d, $i->h, $i->i, $i->s, (int) round($i->f * 1000000), $i->invert, $i->days]];
    } catch (Throwable $e) {
        $iso[] = [$spec, get_class($e) . ': ' . $e->getMessage()];
    }
}

echo json_encode([
    'generator' => 'tests/vectors/iso_interval.gen.php',
    'php' => PHP_VERSION,
    'timezone_version' => timezone_version_get(),
    'iso' => $iso,
], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";

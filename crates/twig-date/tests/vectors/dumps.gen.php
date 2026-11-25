<?php
/*
 * Oracle vectors for the property tables of the date objects (date_object_get_properties_for() & co.):
 * var_dump(), var_export(), print_r(), json_encode(), (array), get_object_vars(), foreach and __serialize().
 * Regenerate with tests/vectors/regen.sh dumps. The clock is 1700000000.123456, the default timezone UTC.
 *
 *  "dumps": [kind, arg, props, var_dump, var_export, print_r, json, (array), get_object_vars, foreach, __serialize, diags]
 *    kind: dt / dti (new DateTime[Immutable](arg)), tz (new DateTimeZone(arg)), iv (new DateInterval(arg)),
 *          ivs (DateInterval::createFromDateString(arg)), diff (arg "a|b": (new DateTime(a))->diff(new DateTime(b)))
 *    props: dynamic properties [name, value] written before dumping
 */
require __DIR__ . '/common.inc.php';
now(1700000000, 123456);
date_default_timezone_set('UTC');

function make(string $kind, string $arg)
{
    return match ($kind) {
        'dt' => new DateTime($arg),
        'dti' => new DateTimeImmutable($arg),
        'tz' => new DateTimeZone($arg),
        'iv' => new DateInterval($arg),
        'ivs' => DateInterval::createFromDateString($arg),
        'diff' => (new DateTime(explode('|', $arg)[0]))->diff(new DateTime(explode('|', $arg)[1])),
    };
}

$cases = [
    ['dt', '2024-01-01 10:00:00.5 Europe/Vilnius', []],
    ['dt', '2024-01-01 10:00:00 +05:30', []],
    ['dt', '2024-01-01 10:00:00 -004430', []],
    ['dt', '2024-01-01 10:00:00 CEST', []],
    ['dt', '2024-01-01 10:00:00 +24:59', []],
    ['dt', '@-62198755200', []],
    ['dt', '@253402300800', []],
    ['dt', 'now', []],
    ['dti', '2024-02-29 23:59:59.999999 America/New_York', []],
    ['dt', '2024-01-01', [['foo', 1], ['date', 'mine'], ['timezone', null]]],
    ['dti', '2024-01-01', [['zz', [1, 2]]]],
    ['tz', 'Europe/Vilnius', []],
    ['tz', '+05:30', []],
    ['tz', '-03:00:15', []],
    ['tz', 'CEST', []],
    ['tz', 'utc', [['foo', 'bar']]],
    ['iv', 'P1Y2M3DT4H5M6S', []],
    ['iv', 'PT0S', [['foo', 1]]],
    ['ivs', '+1 day', []],
    ['ivs', 'last day of next month', [['x', 2]]],
    ['diff', '2024-01-01|2023-11-15 12:30:00.25', []],
    ['diff', '2024-01-01|2024-03-05 10:20:30', []],
];

$dumps = [];
foreach ($cases as [$kind, $arg, $props]) {
    $o = make($kind, $arg);
    foreach ($props as [$k, $v]) {
        $o->$k = $v;
    }
    take_diags();
    ob_start();
    var_dump($o);
    $vd = preg_replace('/\)#\d+ \(/', ')#N (', ob_get_clean());
    $ve = var_export($o, true);
    $pr = preg_replace('/ Object\n/', " Object\n", print_r($o, true));
    $js = json_encode($o);
    $arr = (array) $o;
    $gov = get_object_vars($o);
    $fe = [];
    foreach ($o as $k => $v) {
        $fe[$k] = $v;
    }
    $ser = $o->__serialize();
    $dumps[] = [$kind, $arg, $props, enc($vd), enc($ve), enc($pr), $js, $arr, $gov, $fe, $ser, take_diags()];
}

emit('dumps.gen.php', ['dumps' => $dumps]);

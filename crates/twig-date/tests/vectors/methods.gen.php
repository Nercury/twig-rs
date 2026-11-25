<?php
/*
 * Oracle vectors for the methods of DateTime, DateTimeImmutable, DateTimeZone and DateInterval called with
 * PHP values: argument parsing (types, arity, named arguments, null/float deprecations), setters and getters,
 * static methods called through an instance, (un)serialisation, the uninitialised state, clone and
 * comparisons. Regenerate with tests/vectors/regen.sh methods. Clock 1700000000.123456, default timezone
 * Europe/Vilnius.
 *
 *  "calls": [target, method, args, result, same, after, diags]
 *    target: [kind, arg] (dt, dti, tz, iv, ivs as in dumps.gen.php)
 *    args:   [[name|null, value], ...] where value is JSON or {"$tz": s}, {"$dt": s}, {"$dti": s}, {"$iv": s},
 *            {"$std": 1}, {"$float": "NAN"|"INF"|"-INF"}
 *    result: encoded return value ({"$class": ..., ...} for objects) or "Class: message"
 *    same:   whether the result is the target object itself
 *    after:  the target afterwards (encoded)
 *  "compare": [a, b, ==, <, >, <=>, diags]   (a, b: [kind, arg]; results or "Class: message")
 *  "sequences": [name, steps]   multi-step scenarios, each step [description, encoded result]
 */
require __DIR__ . '/common.inc.php';
now(1700000000, 123456);
date_default_timezone_set('Europe/Vilnius');

function make(array $t)
{
    [$kind, $arg] = $t;
    return match ($kind) {
        'dt' => new DateTime($arg),
        'dti' => new DateTimeImmutable($arg),
        'tz' => new DateTimeZone($arg),
        'iv' => new DateInterval($arg),
        'ivs' => DateInterval::createFromDateString($arg),
        'std' => new stdClass(),
        'int' => (int) $arg,
    };
}

function arg_value($v)
{
    if (is_array($v) && count($v) === 1 && is_string(array_key_first($v)) && str_starts_with(array_key_first($v), '$')) {
        $k = array_key_first($v);
        $a = $v[$k];
        return match ($k) {
            '$tz' => new DateTimeZone($a),
            '$dt' => new DateTime($a),
            '$dti' => new DateTimeImmutable($a),
            '$iv' => new DateInterval($a),
            '$std' => new stdClass(),
            '$float' => match ($a) { 'NAN' => NAN, 'INF' => INF, '-INF' => -INF },
        };
    }
    return $v;
}

function encode($v)
{
    if ($v instanceof DateTimeInterface) {
        try {
            return ['$class' => get_class($v), 'show' => enc(show($v))];
        } catch (Throwable $e) {
            return ['$class' => get_class($v), 'show' => exc($e)];
        }
    }
    if ($v instanceof DateTimeZone) {
        return ['$class' => 'DateTimeZone', 'name' => $v->getName(), 'type' => ((array) $v)['timezone_type']];
    }
    if ($v instanceof DateInterval) {
        return ['$class' => 'DateInterval', 'fmt' => $v->format('%y %m %d %h %i %s %f %R %a'), 'props' => (array) $v];
    }
    if (is_object($v)) {
        return ['$class' => get_class($v)];
    }
    if (is_array($v)) {
        return array_map('encode', $v);
    }
    if (is_float($v) && !is_finite($v)) {
        return ['$float' => (string) $v];
    }
    return is_string($v) ? enc($v) : $v;
}

function run(array $target, string $method, array $args): array
{
    $o = make($target);
    take_diags();
    $pos = [];
    $named = [];
    foreach ($args as [$name, $value]) {
        if ($name === null) {
            $pos[] = arg_value($value);
        } else {
            $named[$name] = arg_value($value);
        }
    }
    try {
        $r = $o->$method(...$pos, ...$named);
        $res = encode($r);
        $same = $r === $o;
    } catch (Throwable $e) {
        $res = exc($e);
        $same = false;
    }
    return [$target, $method, $args, $res, $same, encode($o), take_diags()];
}

$D = ['dt', '2024-01-31 10:20:30.5'];
$I = ['dti', '2024-01-31 10:20:30.5'];
$Z = ['tz', 'Europe/Vilnius'];
$V = ['iv', 'P1Y2M3DT4H5M6S'];
$calls = [];
$add = function (array $t, string $m, array $args = []) use (&$calls) {
    $calls[] = run($t, $m, $args);
};

foreach ([$D, $I] as $T) {
    // arity, types, named arguments
    $add($T, 'format', []);
    $add($T, 'format', [[null, 'Y'], [null, 'm']]);
    $add($T, 'format', [[null, ['x']]]);
    $add($T, 'format', [[null, null]]);
    $add($T, 'format', [[null, 12]]);
    $add($T, 'format', [[null, 1.5]]);
    $add($T, 'format', [[null, true]]);
    $add($T, 'format', [['format', 'Y']]);
    $add($T, 'format', [['fmt', 'Y']]);
    $add($T, 'format', [[null, 'Y'], ['format', 'Y']]);
    $add($T, 'setTime', [[null, 1]]);
    $add($T, 'setTime', [[null, 'abc'], [null, 1]]);
    $add($T, 'setTime', [[null, '5'], [null, ' 6']]);
    $add($T, 'setTime', [[null, 1.5], [null, 2]]);
    $add($T, 'setTime', [[null, null], [null, 2]]);
    $add($T, 'setTime', [[null, 1], [null, 2], [null, 3], [null, 4], [null, 5]]);
    $add($T, 'setTime', [['hour', 1], ['minute', 2]]);
    $add($T, 'setTime', [['minute', 2]]);
    $add($T, 'setTime', [[null, 1], [null, 2], ['microsecond', 999999]]);
    $add($T, 'setTime', [[null, 25], [null, 70], [null, 70], [null, 2000000]]);
    $add($T, 'setTime', [[null, -1], [null, -1], [null, -1], [null, -1]]);
    $add($T, 'setTime', [[null, '1e3'], [null, 0]]);
    $add($T, 'setTime', [[null, ['x']], [null, 0]]);
    $add($T, 'setTime', [[null, 1], [null, 0], ['foo', 3]]);
    $add($T, 'setDate', [[null, 2024], [null, 2], [null, 30]]);
    $add($T, 'setDate', [[null, 2024], [null, 14], [null, -1]]);
    $add($T, 'setDate', [[null, -44], [null, 3], [null, 15]]);
    $add($T, 'setDate', [[null, 2024], [null, 2]]);
    $add($T, 'setISODate', [[null, 2024], [null, 1]]);
    $add($T, 'setISODate', [[null, 2020], [null, 53], [null, 7]]);
    $add($T, 'setISODate', [[null, 2021], [null, 0], [null, 0]]);
    $add($T, 'setISODate', [['year', 2024], ['week', 10], ['dayOfWeek', 3]]);
    $add($T, 'setTimestamp', [[null, 0]]);
    $add($T, 'setTimestamp', [[null, -1]]);
    $add($T, 'setTimestamp', [[null, '1700000000']]);
    $add($T, 'setTimestamp', [[null, 1.0e19]]);
    $add($T, 'setTimestamp', [[null, 'x']]);
    $add($T, 'setMicrosecond', [[null, 0]]);
    $add($T, 'setMicrosecond', [[null, 999999]]);
    $add($T, 'setMicrosecond', [[null, 1000000]]);
    $add($T, 'setMicrosecond', [[null, -1]]);
    $add($T, 'getMicrosecond', []);
    $add($T, 'getMicrosecond', [[null, 1]]);
    $add($T, 'getTimestamp', []);
    $add($T, 'getTimestamp', [[null, 1]]);
    $add($T, 'getOffset', []);
    $add($T, 'getTimezone', []);
    $add($T, 'setTimezone', [[null, ['$tz' => 'Asia/Tokyo']]]);
    $add($T, 'setTimezone', [[null, ['$tz' => '-03:30']]]);
    $add($T, 'setTimezone', [[null, ['$tz' => 'PST']]]);
    $add($T, 'setTimezone', [[null, 'UTC']]);
    $add($T, 'setTimezone', [[null, null]]);
    $add($T, 'setTimezone', [[null, ['$std' => 1]]]);
    $add($T, 'modify', [[null, '+1 month']]);
    $add($T, 'modify', [[null, null]]);
    $add($T, 'modify', [[null, 'nonsense']]);
    $add($T, 'modify', [[null, 5]]);
    $add($T, 'add', [[null, ['$iv' => 'P1M']]]);
    $add($T, 'add', [[null, 'P1D']]);
    $add($T, 'sub', [[null, ['$iv' => 'PT36H']]]);
    $add($T, 'diff', [[null, ['$dt' => '2025-03-01']]]);
    $add($T, 'diff', [[null, ['$dti' => '2020-03-01 Asia/Tokyo']], [null, true]]);
    $add($T, 'diff', [[null, ['$dt' => '2025-03-01']], [null, 'yes']]);
    $add($T, 'diff', [[null, 'x']]);
    $add($T, 'diff', [['targetObject', ['$dt' => '2024-01-01']], ['absolute', 1]]);
    // static methods through the instance
    $add($T, 'createFromFormat', [[null, 'Y-m-d H:i'], [null, '2020-05-06 07:08']]);
    $add($T, 'createFromFormat', [[null, 'Y-m-d'], [null, 'nope']]);
    $add($T, 'createFromFormat', [[null, 'Y-m-d'], [null, "2020-01-01\0"]]);
    $add($T, 'createFromFormat', [[null, 'Y-m-d'], [null, '2020-01-01'], [null, ['$tz' => '+01:00']]]);
    $add($T, 'createFromFormat', [[null, 'Y-m-d'], [null, '2020-01-01'], [null, 'UTC']]);
    $add($T, 'createFromTimestamp', [[null, 1700000000]]);
    $add($T, 'createFromTimestamp', [[null, 1700000000.5]]);
    $add($T, 'createFromTimestamp', [[null, -1.5]]);
    $add($T, 'createFromTimestamp', [[null, -0.0000001]]);
    $add($T, 'createFromTimestamp', [[null, 0.9999996]]);
    $add($T, 'createFromTimestamp', [[null, '12.25']]);
    $add($T, 'createFromTimestamp', [[null, 1.0e19]]);
    $add($T, 'createFromTimestamp', [[null, -9.3e18]]);
    $add($T, 'createFromTimestamp', [[null, ['$float' => 'NAN']]]);
    $add($T, 'createFromTimestamp', [[null, ['$float' => '-INF']]]);
    $add($T, 'createFromTimestamp', [[null, 'abc']]);
    $add($T, 'createFromInterface', [[null, ['$dti' => '2001-02-03 04:05:06 +01:00']]]);
    $add($T, 'createFromInterface', [[null, ['$std' => 1]]]);
    $add($T, 'getLastErrors', []);
    $add($T, '__set_state', [[null, ['date' => '2020-01-01 00:00:00.000000', 'timezone_type' => 3, 'timezone' => 'Asia/Tokyo']]]);
    $add($T, '__set_state', [[null, ['date' => '2020-01-01 00:00:00.000000', 'timezone_type' => 1, 'timezone' => '+05:00']]]);
    $add($T, '__set_state', [[null, ['date' => '2020-01-01 00:00:00.000000', 'timezone_type' => 2, 'timezone' => 'CEST']]]);
    $add($T, '__set_state', [[null, ['date' => '2020-01-01', 'timezone_type' => 3, 'timezone' => 'Nowhere']]]);
    $add($T, '__set_state', [[null, ['date' => 'garbage', 'timezone_type' => 3, 'timezone' => 'UTC']]]);
    $add($T, '__set_state', [[null, ['date' => '2020-01-01', 'timezone_type' => '3', 'timezone' => 'UTC']]]);
    $add($T, '__set_state', [[null, []]]);
    $add($T, '__serialize', []);
    $add($T, '__unserialize', [[null, ['date' => '2021-06-07 08:09:10.5', 'timezone_type' => 3, 'timezone' => 'UTC', 'extra' => 5]]]);
    $add($T, '__unserialize', [[null, ['date' => 'garbage', 'timezone_type' => 3, 'timezone' => 'UTC']]]);
    $add($T, '__unserialize', [[null, ['date' => 1]]]);
    $add($T, '__wakeup', []);
    $add($T, '__construct', [[null, '2020-02-02 02:02']]);
    $add($T, '__construct', [[null, 'garbage']]);
    $add($T, '__construct', []);
    $add($T, 'nosuchmethod', []);
}
$add($D, 'createFromImmutable', [[null, ['$dti' => '2001-02-03']]]);
$add($D, 'createFromImmutable', [[null, ['$dt' => '2001-02-03']]]);
$add($I, 'createFromMutable', [[null, ['$dt' => '2001-02-03']]]);
$add($I, 'createFromMutable', [[null, ['$dti' => '2001-02-03']]]);
$add(['dt', '2024-01-01 00:00 CEST'], 'getOffset', []);
$add(['dt', '2024-01-01 00:00 +05:45'], 'getOffset', []);
$add(['dt', '2024-01-01 00:00 CEST'], 'getTimezone', []);
$add(['dt', '@1700000000'], 'getTimezone', []);
$add(['dt', '2024-07-01 America/New_York'], 'getOffset', []);

foreach ([$Z, ['tz', '+05:30'], ['tz', 'CEST']] as $T) {
    $add($T, 'getName', []);
    $add($T, 'getName', [[null, 1]]);
    $add($T, 'getOffset', [[null, ['$dt' => '2024-07-01 12:00']]]);
    $add($T, 'getOffset', [[null, 'now']]);
    $add($T, 'getOffset', []);
    $add($T, 'getLocation', []);
    $add($T, 'getTransitions', [[null, 1700000000], [null, 1720000000]]);
    $add($T, 'getTransitions', [[null, 'x']]);
    $add($T, '__serialize', []);
    $add($T, '__unserialize', [[null, ['timezone_type' => 3, 'timezone' => 'Asia/Tokyo', 'x' => 1]]]);
    $add($T, '__unserialize', [[null, ['timezone_type' => 4, 'timezone' => 'UTC']]]);
    $add($T, '__set_state', [[null, ['timezone_type' => 1, 'timezone' => '+03:00']]]);
    $add($T, '__set_state', [[null, ['timezone_type' => 3, 'timezone' => "UTC\0"]]]);
    $add($T, '__wakeup', []);
    $add($T, '__construct', [[null, 'Asia/Tokyo']]);
    $add($T, '__construct', [[null, 'Nowhere/Land']]);
    $add($T, 'listIdentifiers', [[null, DateTimeZone::PER_COUNTRY], [null, 'LT']]);
    $add($T, 'listIdentifiers', [['countryCode', 'LT'], ['timezoneGroup', DateTimeZone::PER_COUNTRY]]);
}
foreach ([$V, ['ivs', 'next monday'], ['iv', 'PT0S']] as $T) {
    $add($T, 'format', [[null, '%a %R %d']]);
    $add($T, 'format', []);
    $add($T, 'format', [[null, null]]);
    $add($T, 'createFromDateString', [[null, '+3 days']]);
    $add($T, 'createFromDateString', [[null, 'garbage']]);
    $add($T, 'createFromDateString', [[null, '2024-01-01']]);
    $add($T, '__serialize', []);
    $add($T, '__unserialize', [[null, ['y' => 3, 'days' => 10, 'extra' => 'x']]]);
    $add($T, '__wakeup', []);
    $add($T, '__construct', [[null, 'P2D']]);
    $add($T, '__construct', [[null, 'garbage']]);
}

$compare = [];
$pairs = [
    [$D, $I], [$D, ['dt', '2024-01-31 10:20:30.500001']], [$D, ['dti', '2024-01-31 08:20:30.5 UTC']],
    [$D, ['dt', '2024-01-31 10:20:31']], [['dti', '2030-01-01'], $D], [$D, ['std', '']], [$D, ['int', '5']],
    [$Z, ['tz', 'Europe/Vilnius']], [$V, ['iv', 'P1Y2M3DT4H5M6S']], [$V, ['int', '1']], [$Z, $D],
];
foreach ($pairs as [$ta, $tb]) {
    $a = make($ta);
    $b = make($tb);
    take_diags();
    $row = [$ta, $tb];
    foreach (['==', '<', '>', '<=>'] as $op) {
        try {
            $row[] = match ($op) { '==' => $a == $b, '<' => $a < $b, '>' => $a > $b, '<=>' => $a <=> $b };
        } catch (Throwable $e) {
            $row[] = exc($e);
        }
    }
    $row[] = take_diags();
    $compare[] = $row;
}

$sequences = [];
// A failed re-construction leaves the object uninitialised.
$d = new DateTime('2024-01-01');
$steps = [];
try {
    $d->__construct('garbage');
} catch (Throwable $e) {
    $steps[] = ['construct', exc($e)];
}
foreach (['format' => ['Y'], 'getTimestamp' => [], 'modify' => ['+1 day'], 'getTimezone' => [], 'setTime' => [1, 2]] as $m => $a) {
    try {
        $steps[] = [$m, encode($d->$m(...$a))];
    } catch (Throwable $e) {
        $steps[] = [$m, exc($e)];
    }
}
try {
    $steps[] = ['clone', encode(clone $d)];
} catch (Throwable $e) {
    $steps[] = ['clone', exc($e)];
}
try {
    $steps[] = ['compare', $d == new DateTime()];
} catch (Throwable $e) {
    $steps[] = ['compare', exc($e)];
}
$steps[] = ['var_dump', preg_replace('/\)#\d+ \(/', ')#N (', (function () use ($d) { ob_start(); var_dump($d); return ob_get_clean(); })())];
$sequences[] = ['uninitialized', $steps];
// clone independence
$d = new DateTime('2024-01-01 10:00');
$c = clone $d;
$c->modify('+1 day');
$sequences[] = ['clone', [['orig', encode($d)], ['clone', encode($c)]]];
// immutable methods leave the original unchanged and copy dynamic properties
$i = new DateTimeImmutable('2024-01-01 10:00');
$i->foo = 'bar';
$j = $i->add(new DateInterval('P1D'));
$sequences[] = ['immutable', [['orig', encode($i)], ['new', encode($j)], ['foo', $j->foo], ['diags', take_diags()]]];

emit('methods.gen.php', ['calls' => $calls, 'compare' => $compare, 'sequences' => $sequences]);

<?php
/*
 * Oracle vectors for DatePeriod: the constructor overloads and createFromISO8601String() (with their errors),
 * iteration (foreach and getIterator()), the getters, the read-only virtual properties and the property
 * tables. Regenerate with tests/vectors/regen.sh period. Clock 1700000000.123456, default timezone
 * Europe/Vilnius.
 *
 *  "periods": [id, construction, iteration | "Class: message", getters, properties, dumps, diags]
 *    construction: [kind, args] with kind "rec" (start, interval spec, recurrences, options),
 *    "end" (start, interval spec, end, options), "iso" (new DatePeriod(string, options)),
 *    "create" (DatePeriod::createFromISO8601String(string, options)), "raw" (raw constructor arguments)
 */
require __DIR__ . '/common.inc.php';
now(1700000000, 123456);
date_default_timezone_set('Europe/Vilnius');

function build(string $kind, array $a)
{
    switch ($kind) {
        case 'rec':
            return new DatePeriod(make_date($a[0]), new DateInterval($a[1]), $a[2], $a[3]);
        case 'end':
            return new DatePeriod(make_date($a[0]), new DateInterval($a[1]), make_date($a[2]), $a[3]);
        case 'iso':
            return new DatePeriod($a[0], $a[1]);
        case 'create':
            return DatePeriod::createFromISO8601String($a[0], $a[1]);
        case 'raw':
            return new DatePeriod(...array_map(fn ($x) => is_array($x) ? make_date($x) : $x, $a));
    }
}

function make_date(array $d)
{
    return $d[0] === 'dti' ? new DateTimeImmutable($d[1]) : ($d[0] === 'iv' ? new DateInterval($d[1]) : new DateTime($d[1]));
}

function enc_value($v)
{
    if ($v instanceof DateTimeInterface) {
        return ['$class' => get_class($v), 'show' => enc(show($v))];
    }
    if ($v instanceof DateInterval) {
        return ['$class' => 'DateInterval', 'fmt' => $v->format('%y %m %d %h %i %s %f %R %a')];
    }
    if (is_array($v)) {
        return array_map('enc_value', $v);
    }
    return $v;
}

$specs = [
    ['rec', [['dt', '2024-01-31 10:00'], 'P1M', 4, 0]],
    ['rec', [['dti', '2024-01-31 10:00'], 'P1D', 3, DatePeriod::EXCLUDE_START_DATE]],
    ['rec', [['dt', '2024-03-30 12:00'], 'PT12H', 5, DatePeriod::INCLUDE_END_DATE]],
    ['rec', [['dt', '2024-01-01'], 'P1W', 0, 0]],
    ['rec', [['dt', '2024-01-01'], 'P1W', -1, 0]],
    ['rec', [['dt', '2024-01-01'], 'P1W', 2147483639, 0]],
    ['rec', [['dt', '2024-01-01'], 'P1W', 2147483638, 3]],
    ['end', [['dt', '2024-01-01'], 'P1D', ['dt', '2024-01-05'], 0]],
    ['end', [['dt', '2024-01-01'], 'P1D', ['dt', '2024-01-05'], DatePeriod::INCLUDE_END_DATE]],
    ['end', [['dt', '2024-01-01'], 'P1D', ['dt', '2024-01-05'], DatePeriod::EXCLUDE_START_DATE | DatePeriod::INCLUDE_END_DATE]],
    ['end', [['dti', '2024-01-01 00:00:00.5'], 'PT1H', ['dt', '2024-01-01 03:00:00.5'], 2]],
    ['end', [['dt', '2024-01-05'], 'P1D', ['dt', '2024-01-01'], 0]],
    ['end', [['dt', '2024-10-27 00:00'], 'PT1H', ['dt', '2024-10-27 06:00'], 0]],
    ['iso', ['R4/2012-07-01T00:00:00Z/P7D', 0]],
    ['iso', ['2012-07-01T00:00:00Z/P7D/2012-08-01T00:00:00Z', 0]],
    ['create', ['R2/2012-07-01T00:00:00Z/P1M', DatePeriod::EXCLUDE_START_DATE]],
    ['create', ['2012-07-01T00:00:00Z/P1D/2012-07-04T00:00:00Z', 0]],
    ['create', ['garbage', 0]],
    ['create', ['R2/P1D', 0]],
    ['create', ['2012-07-01T00:00:00Z/2012-08-01T00:00:00Z', 0]],
    ['create', ['2012-07-01T00:00:00Z/P1D', 0]],
    ['create', ['R0/2012-07-01T00:00:00Z/P1D', 0]],
    ['raw', [['dt', '2024-01-01'], ['iv', 'P1D'], 'x']],
    ['raw', [['dt', '2024-01-01'], ['iv', 'P1D'], '3', '1']],
    ['raw', [['dt', '2024-01-01']]],
    ['raw', ['R2/2012-07-01T00:00:00Z/P1D', 'x']],
    ['raw', [['dt', '2024-01-01'], 2]],
    ['raw', []],
];

$periods = [];
foreach ($specs as $id => [$kind, $args]) {
    take_diags();
    try {
        $p = build($kind, $args);
    } catch (Throwable $e) {
        $periods[] = [$id, [$kind, $args], exc($e), null, null, null, take_diags()];
        continue;
    }
    $before = [];
    foreach (['start', 'current', 'end', 'interval', 'recurrences', 'include_start_date', 'include_end_date'] as $n) {
        $before[$n] = [enc_value($p->$n), isset($p->$n), property_exists($p, $n)];
    }
    try {
        $it = [];
        $n = 0;
        foreach ($p as $k => $d) {
            $it[] = [$k, get_class($d), enc(show($d))];
            if (++$n > 10) {
                break;
            }
        }
    } catch (Throwable $e) {
        $it = exc($e);
    }
    $getters = [];
    foreach (['getStartDate', 'getEndDate', 'getDateInterval', 'getRecurrences'] as $m) {
        try {
            $getters[$m] = enc_value($p->$m());
        } catch (Throwable $e) {
            $getters[$m] = exc($e);
        }
    }
    $props = ['before' => $before, 'current_after' => enc_value($p->current)];
    foreach (['write' => fn () => $p->start = null, 'unset' => function () use ($p) { unset($p->recurrences); }, 'dyn' => fn () => $p->foo = 1] as $op => $f) {
        try {
            $f();
            $props[$op] = 'ok';
        } catch (Throwable $e) {
            $props[$op] = exc($e);
        }
    }
    $iter = [];
    $ii = $p->getIterator();
    $iter[] = get_class($ii);
    for ($k = 0; $k < 3; $k++) {
        $iter[] = [$ii->valid(), $ii->key(), $ii->valid() ? enc(show($ii->current())) : null];
        $ii->next();
    }
    $ii->rewind();
    $iter[] = [$ii->valid(), $ii->key()];
    ob_start();
    var_dump($p);
    $vd = preg_replace('/\)#\d+ \(/', ')#N (', ob_get_clean());
    $dumps = [enc($vd), json_encode($p), enc_value((array) $p), enc_value($p->__serialize()), $iter];
    $periods[] = [$id, [$kind, $args], $it, $getters, $props, $dumps, take_diags()];
}

$state = [];
$base = (new DatePeriod(new DateTime('2024-01-01'), new DateInterval('P1D'), 2))->__serialize();
foreach ([
    'valid' => $base,
    'no_start' => array_diff_key($base, ['start' => 1]),
    'null_start' => ['start' => null] + $base,
    'bad_interval' => ['interval' => 'P1D'] + $base,
    'big_recurrences' => ['recurrences' => 2147483648] + $base,
    'string_recurrences' => ['recurrences' => '3'] + $base,
    'int_include' => ['include_start_date' => 1] + $base,
    'extra' => $base + ['extra' => 'x'],
] as $name => $data) {
    try {
        $p = DatePeriod::__set_state($data);
        $r1 = enc_value((array) $p);
    } catch (Throwable $e) {
        $r1 = exc($e);
    }
    try {
        $p = new DatePeriod(new DateTime('2000-01-01'), new DateInterval('PT1H'), 1);
        $p->__unserialize($data);
        $r2 = enc_value((array) $p);
    } catch (Throwable $e) {
        $r2 = exc($e);
    }
    $state[] = [$name, $r1, $r2, take_diags()];
}

emit('period.gen.php', ['periods' => $periods, 'state' => $state]);

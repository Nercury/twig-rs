<?php
/*
 * Oracle vectors for DateInterval: ISO 8601 construction (date_interval_initialize()), createFromDateString()
 * and date_interval_create_from_date_string(), the property handlers (reads, isset, writes), format() edge
 * cases and __set_state() / __unserialize(). Regenerate with tests/vectors/regen.sh interval.
 *
 *  "construct": [spec, (array) $i as JSON | "Class: message"]
 *  "from_string": [spec, (array) | "Class: message", date_interval_create_from_date_string() (array)|false, diags]
 *  "format":    [spec, format, out]                    $i = new DateInterval(spec)
 *  "props":     [spec, op, name, value|null, result, diags]  op "get" ($i->name), "isset", "empty", "exists",
 *               "set" ($i->name = value; then (array) $i and format('%y %m %d %h %i %s %f %R %a'))
 *  "state":     [array, (array) DateInterval::__set_state(array) | "Class: message", format('%y %m %d %h %i %s %f %R %a')]
 */
require __DIR__ . '/common.inc.php';

$specs = ['P1D', 'PT1H', 'P1Y2M3DT4H5M6S', 'P2W', 'P1W2D', 'PT0S', 'P0D', 'PT36H', 'P0001-02-03T04:05:06',
    'P0001-02-03', 'PT1.5S', 'P1.5Y', 'P', 'PT', 'P1', 'P1X', 'xP1D', '', ' P1D ', 'P1D ', 'p1d', 'P-1D',
    'P99999999999999D', 'PT999999999S', '2024-01-01T00:00:00Z/2024-03-05T10:20:30Z', 'R5/2024-01-01T00:00:00Z/P1D',
    '2024-01-01T00:00:00Z/P1D', 'P1D/2024-01-01T00:00:00Z', '20240101T000000Z/20240201T000000Z', 'P1Y1Y',
    "P1D\0junk", 'PT1M30S', 'P1M30D', 'P12M', 'PT24H', 'P1DT', 'PY', 'P1YT2H3M'];
$construct = [];
foreach ($specs as $spec) {
    try {
        $i = new DateInterval($spec);
        $construct[] = [enc($spec), (array) $i];
    } catch (Throwable $e) {
        $construct[] = [enc($spec), exc($e)];
    }
}

$strs = ['+1 day', '1 day', '-1 day', '+1 month -2 days', 'next monday', 'last day of next month',
    'first day of next month', '+3 weekdays', '1 year ago', '+90 minutes', '+1.5 hours', 'tomorrow', 'midnight',
    'noon', 'now', '', 'garbage', '2024-01-01', '10:00', 'Europe/Paris', '+1 day EST', '@123', 'saturday this week',
    'last sunday of march', '+0 seconds', '+1 usec', '+2 fortnights', "+1 day\0x", 'Ä'];
$from_string = [];
foreach ($strs as $spec) {
    try {
        $i = DateInterval::createFromDateString($spec);
        $a = (array) $i;
        $r = [$a, $i->format('%y %m %d %h %i %s %f %R %a'), $i->y, $i->m, $i->d, $i->days, $i->f];
    } catch (Throwable $e) {
        $r = exc($e);
    }
    $f = date_interval_create_from_date_string($spec);
    $from_string[] = [enc($spec), $r, $f === false ? false : (array) $f, take_diags()];
}

$fmts = ['%y %Y %m %M %d %D %h %H %i %I %s %S %f %F %a %R %r %%', '', '%', '%%', '%q', 'abc%', '%%%', '%%y',
    '%Y-%M-%D %H:%I:%S', '%R%a days', '%r%y', '%F%f', 'ÄÖ %d', '%a', '%5d', "%\0", 'no specifiers'];
$format = [];
foreach (['P1Y2M3DT4H5M6S', 'P0D', 'PT36H', 'P400D', 'P10Y'] as $spec) {
    foreach ($fmts as $f) {
        $format[] = [$spec, enc($f), enc((new DateInterval($spec))->format($f))];
    }
}
// Inverted and day-counted intervals from diff().
foreach ([['2024-03-05 10:20:30.5', '2024-01-01'], ['2020-02-29', '2024-02-28 23:59:59.999999'], ['2024-01-01', '2024-01-01']] as [$a, $b]) {
    $i = (new DateTime($a, new DateTimeZone('UTC')))->diff(new DateTime($b, new DateTimeZone('UTC')));
    foreach ($fmts as $f) {
        $format[] = ["diff:$a|$b", enc($f), enc($i->format($f))];
    }
}

$props = [];
$names = ['y', 'm', 'd', 'h', 'i', 's', 'f', 'invert', 'days', 'from_string', 'date_string', 'foo'];
foreach (['P1Y2M3DT4H5M6S', 'P0D', 'str:+1 day', 'str:next monday', 'diff'] as $spec) {
    foreach ($names as $n) {
        foreach (['get', 'isset', 'empty', 'exists'] as $op) {
            $i = make($spec);
            $r = match ($op) {
                'get' => $i->$n,
                'isset' => isset($i->$n),
                'empty' => empty($i->$n),
                'exists' => property_exists($i, $n),
            };
            $props[] = [$spec, $op, $n, null, $r, take_diags()];
        }
    }
    foreach ([['y', 5], ['d', '7'], ['h', 2.9], ['s', true], ['f', 0.25], ['f', '1.5'], ['invert', 1], ['days', 9],
        ['from_string', true], ['foo', 'bar'], ['m', null], ['i', 'abc']] as [$n, $val]) {
        $i = make($spec);
        $i->$n = $val;
        $props[] = [$spec, 'set', $n, $val, [(array) $i, $i->format('%y %m %d %h %i %s %f %R %a')], take_diags()];
    }
    // A property read after the table has been built by an (array) cast.
    $i = make($spec);
    $before = [$i->from_string ?? 'unset', take_diags()];
    $a = (array) $i;
    $props[] = [$spec, 'after_cast', 'from_string', null, [$before, $i->from_string, isset($i->date_string)], take_diags()];
}

function make(string $spec): DateInterval
{
    if ($spec === 'diff') {
        return (new DateTime('2024-01-01', new DateTimeZone('UTC')))->diff(new DateTime('2023-11-15 12:30:00.25', new DateTimeZone('UTC')));
    }
    if (str_starts_with($spec, 'str:')) {
        return DateInterval::createFromDateString(substr($spec, 4));
    }
    return new DateInterval($spec);
}

$states = [
    ['y' => 1, 'm' => 2, 'd' => 3, 'h' => 4, 'i' => 5, 's' => 6, 'f' => 0.5, 'invert' => 1, 'days' => 400],
    ['y' => '1', 'days' => false],
    ['days' => '12abc', 'f' => '0.000001'],
    [],
    ['y' => [1], 'm' => new stdClass(), 'days' => true],
    ['date_string' => '+1 day'],
    ['date_string' => 'garbage'],
    ['date_string' => 5, 'y' => 3],
    ['from_string' => true, 'date_string' => 'next monday'],
    ['y' => 1, 'weekday' => 3, 'have_weekday_relative' => 1, 'special_type' => 1, 'special_amount' => '-5', 'have_special_relative' => 1],
    ['civil_or_wall' => 2, 'h' => 25],
    ['f' => 'abc', 'invert' => '1'],
];
$state = [];
foreach ($states as $a) {
    try {
        $i = DateInterval::__set_state($a);
        $state[] = [json_encode($a), (array) $i, $i->format('%y %m %d %h %i %s %f %R %a'), take_diags()];
    } catch (Throwable $e) {
        $state[] = [json_encode($a), exc($e), null, take_diags()];
    }
}

emit('interval.gen.php', ['construct' => $construct, 'from_string' => $from_string, 'format' => $format,
    'props' => $props, 'state' => $state]);

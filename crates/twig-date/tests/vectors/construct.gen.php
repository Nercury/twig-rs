<?php
/*
 * Oracle vectors for DateTime / DateTimeImmutable construction (php_date_initialize()) with a faked clock:
 * absolute, relative and zone-carrying strings, timezone arguments, default timezones and parse errors, plus
 * createFromFormat() and getLastErrors(). Regenerate with tests/vectors/regen.sh construct.
 *
 *  "construct": [class, input, tz|null, default_tz, now_sec, now_usec, result, last_errors]
 *               result: show($d) . ' ' . timezone_type, or "Class: message"; last_errors: getLastErrors()
 *  "create":    same through date_create() (result false on failure)
 *  "format":    [class, format, input, tz|null, default_tz, now_sec, now_usec, result|false, last_errors]
 */
require __DIR__ . '/common.inc.php';
mt_srand(20261126);

$inputs = ['2024-01-01', '2024-01-01 10:30:00', '2024-01-01 10:30:00.5', '2024-02-29 23:59:59.999999',
    '1999-12-31T23:59:59Z', '2024-06-15T12:00:00+05:30', '2024-01-01 12:00 Europe/Paris', '2024-01-01 12:00 CEST',
    '2024-01-01 EST', '2024-07-01 America/New_York', 'now', '', 'NOW', 'Now ', 'today', 'tomorrow', 'yesterday 14:00',
    'midnight', 'noon', '+1 day', '-1 month', '+1 week 2 days', 'next monday', 'last friday', 'last day of next month',
    'first day of this month', 'first monday of january 2025', 'this week', 'monday next week', '1 year ago',
    'noon tomorrow', '+90 minutes', '@0', '@1700000000', '@-1', '@1700000000.123456', '@1.5', '@-1.5',
    '10:30', '10:30:45.123', '23:59:60', '24:00', '00:00:60', '2021W10', '2021-W10-5', 'back of 7pm',
    'front of 7pm', '2024-02-30', '2024-13-01', 'Feb 30 2024', '2024-01-01 25:00', 'not a date', 'Ä',
    "2024-01-01\0junk", '99999999999999999999', '+99999999999 years', '1700000000', ' 1700000000', '-1700000000',
    '2024-01-01 00:00:00 +0000', '2024-01-01 00:00:00 -1130', '2024-01-01 00:00:00 +9999', 'Europe/Vilnius', 'CEST',
    '+01:00', 'UTC', 'utc', 'Z', '2024-01-01 Europe/Vilnius Asia/Tokyo', '2024-01-01 EST PST CET', '00:00 tomorrow',
    'Sat, 30 Apr 2016 17:52:13 GMT', 'Saturday 10am', '2024.01.02', '01/02/2024', '2/3/24', '1 Jan', 'Jan 2024',
    '0000-00-00 00:00:00', '-0001-11-30', '10000-01-01', '1e5', 'third saturday of +1 month', '+1 weekday',
    '-3 weekdays', 'last day of feb 2023', '2024-03-31 02:30:00 Europe/Vilnius', '2024-10-27 03:30:00 Europe/Vilnius'];
$tzs = [null, 'UTC', 'Europe/Vilnius', 'America/New_York', '+05:30', '-03:00', 'EST', 'CEST', 'Australia/Lord_Howe'];
$defaults = ['UTC', 'Europe/Vilnius', 'America/Los_Angeles', 'Asia/Tokyo'];
$nows = [[1700000000, 123456], [0, 0], [1711846799, 999999], [1730613600, 500000], [-1000000000, 1],
    [2000000000, 654321], [1709251199, 0], [951825600, 42]];

function last_errors()
{
    $e = DateTime::getLastErrors();
    return $e === false ? false : $e;
}

function result(DateTimeInterface $d): string
{
    $a = (array) $d;
    return enc(show($d)) . ' ' . ($a['timezone_type'] ?? 'none');
}

$construct = [];
$create = [];
foreach ($inputs as $input) {
    for ($k = 0; $k < 7; $k++) {
        $class = $k === 6 ? 'DateTimeImmutable' : 'DateTime';
        $tz = $k === 0 ? null : pick($tzs);
        $default = pick($defaults);
        [$sec, $usec] = pick($nows);
        date_default_timezone_set($default);
        now($sec, $usec);
        try {
            $d = new $class($input, $tz === null ? null : new DateTimeZone($tz));
            $res = result($d);
        } catch (Throwable $e) {
            $res = exc($e);
        }
        $construct[] = [$class, enc($input), $tz, $default, $sec, $usec, $res, last_errors()];
    }
    $tz = pick($tzs);
    [$sec, $usec] = pick($nows);
    date_default_timezone_set('Europe/Vilnius');
    now($sec, $usec);
    $d = date_create($input, $tz === null ? null : new DateTimeZone($tz));
    $create[] = ['DateTime', enc($input), $tz, 'Europe/Vilnius', $sec, $usec, $d === false ? false : result($d), last_errors()];
}

$formats = [
    ['Y-m-d', '2024-02-29'], ['Y-m-d H:i:s', '2024-02-29 13:14:15'], ['!Y-m-d', '2024-02-29'], ['Y-m-d|', '2024-02-29'],
    ['U', '1700000000'], ['U.u', '1700000000.123456'], ['d/m/Y H:i', '15/08/2023 09:05'], ['D, d M Y H:i:s O', 'Tue, 15 Aug 2023 09:05:00 +0200'],
    ['Y-m-d\\TH:i:sP', '2023-08-15T09:05:00+02:00'], ['H\\h i\\m', '09h 05m'], ['z Y', '45 2024'], ['Y z', '2024 45'],
    ['j-M-Y', '15-Feb-2009'], ['l jS F Y', 'Sunday 15th February 2009'], ['Y-m-d e', '2024-01-01 Europe/Paris'],
    ['Y-m-d T', '2024-01-01 EST'], ['?-?-?', '1-2-3'], ['Y-m-d *', '2024-01-01 garbage'], ['Y-m-d+', '2024-01-01 trailing'],
    ['Y#m#d', '2024/01;02'], ['g:i a', '9:05 pm'], ['G:i A', '21:05 PM'], ['H:i:s.u', '10:11:12.123456'], ['H:i:s.v', '10:11:12.123'],
    ['y-n-j', '24-2-9'], ['X-m-d', '+12024-01-01'], ['x-m-d', '2024-01-01'], ['Y-m-d', '2024-02-30'], ['Y-m-d', '2024-1-1'],
    ['Y-m-d', 'garbage'], ['Y-m-d', '2024-01-01 10:00'], ['Y-m-d H:i', '2024-01-01'], ['Y-m-d', ''], ['', ''], ['', 'x'],
    ['H', '25'], ['i', '61'], ['d', '32'], ['Y-m-d \\\\ H', '2024-01-01 \\ 10'], ['\\Y Y', 'Y 2024'], ['Y-m-d', "2024-01-01"],
    ['!d', '15'], ['|d', '15'], ['Y-m-d H:i:s', '2024-03-31 03:30:00'], ['Y-m-d H:i:s e', '2024-03-31 03:30:00 Europe/Vilnius'],
    ['U', '-1'], ['U', 'abc'], ['D Y-m-d', 'Mon 2024-01-03'], ['S', 'th'], ['jS', '3rd'], ['Y-m-d P', '2024-01-01 +05:30'],
    ['Y-m-d O', '2024-01-01 -0930'], ['Y-m-d p', '2024-01-01 Z'], ['M', 'Foo'], ['F Y', 'March 2024'], ['A', 'XM'],
];
$format = [];
foreach ($formats as [$f, $input]) {
    for ($k = 0; $k < 3; $k++) {
        $class = $k === 2 ? 'DateTimeImmutable' : 'DateTime';
        $tz = $k === 0 ? null : pick($tzs);
        $default = pick($defaults);
        [$sec, $usec] = pick($nows);
        date_default_timezone_set($default);
        now($sec, $usec);
        $d = $class::createFromFormat($f, $input, $tz === null ? null : new DateTimeZone($tz));
        $format[] = [$class, enc($f), enc($input), $tz, $default, $sec, $usec, $d === false ? false : result($d), last_errors()];
    }
}

emit('construct.gen.php', ['construct' => $construct, 'create' => $create, 'format' => $format]);

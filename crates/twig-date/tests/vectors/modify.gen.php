<?php
/*
 * Oracle vectors for DateTime::modify(), DateTimeImmutable::modify() and date_modify() (php_date_modify()):
 * relative and absolute modifiers over many base instants and zones, the "@ts" zone reset, DST transitions and
 * the PHP 8.3+ exception / warning rules. Regenerate with tests/vectors/regen.sh modify.
 *
 *  "modify": [zone, ts, us, modifier, result, last_errors]   result: show($d) after $d->modify(), or "Class: msg"
 *  "errors": [modifier, DateTime::modify, DateTimeImmutable::modify, date_modify result, date_modify diagnostics]
 */
require __DIR__ . '/common.inc.php';
mt_srand(20261127);

$zones = ['UTC', 'Europe/Vilnius', 'America/New_York', 'Australia/Lord_Howe', 'Asia/Kathmandu', '+05:30',
    '-03:00', 'EST', 'CEST', 'Pacific/Apia', 'America/Sao_Paulo', 'Europe/London'];
$modifiers = ['+1 day', '-1 day', '+1 month', '-1 month', '+1 year', '-1 year', '+1 week', '+2 weeks 3 days',
    '+1 hour', '-1 hour', '+24 hours', '+90 minutes', '+30 seconds', '+1 sec', '-1 usec', '+500000 usec',
    '+1.5 hours', 'first day of next month', 'last day of next month', 'first day of this month',
    'last day of previous month', 'first day of', 'last day of', 'midnight', 'noon', 'today', 'tomorrow',
    'yesterday', 'now', 'next monday', 'last friday', 'monday this week', 'sunday last week', 'saturday this week',
    'next weekday', '+3 weekdays', '-5 weekdays', 'first monday of next month', 'last sunday of march 2025',
    'third friday of this month', '10:00', '10:00:30.25', '23:59:59.999999', '10:00 tomorrow', 'tomorrow 10:00',
    '2024-02-29', '2024-02-29 12:00', '2025-02-29', '1999-12-31 23:59:59', '@0', '@1700000000', '@-86400',
    '@1700000000.5', 'Europe/Paris', 'CEST', '+02:00', '2024-01-01 Europe/Paris', '+1 day Europe/Paris',
    'back of 7pm', 'front of 7pm', '+0 day', '-0 days', '+1 fortnight', '+1 decade', '+10 centuries',
    '+1 month -1 day', 'last day of +1 month', 'first day of -1 month', '12/25', 'Dec 25', 'Dec 25 2030',
    '2021W10', '2021-W52-7', '+1000000 days', '-1000000 days', '+2147483648 seconds', 'next year', 'last year',
    'this month', 'ago', '1 day ago', '2 hours ago 30 minutes', 'noon yesterday', 'midnight +1 hour'];
$invalid = ['', 'garbage', '+1 dayz', "+1 day\0junk", 'Ä', '2024-13-45', '25:00', '+', '@', 'next', 'Europe/Nowhere',
    '10:00 10:00', '2024-01-01 2024-01-02', 'UTC UTC UTC', '+1 day +1 days EST EST'];

$bases = [];
foreach ($zones as $zone) {
    $bases[] = [$zone, 1700000000, 123456];
    $bases[] = [$zone, mt_rand(-2000000000, 4000000000), mt_rand(0, 999999)];
}
foreach (['Europe/Vilnius', 'America/New_York', 'Australia/Lord_Howe', 'America/Sao_Paulo'] as $zone) {
    $tr = (new DateTimeZone($zone))->getTransitions(1500000000, 1800000000);
    for ($k = 0; $k < 3; $k++) {
        $t = $tr[mt_rand(1, count($tr) - 1)]['ts'];
        $bases[] = [$zone, $t + pick([-7200, -3600, -1800, -1, 0, 1, 1800, 3600, 86400 - 3600]), 0];
    }
}
$bases[] = ['UTC', 1709164800, 0];      // 2024-02-29
$bases[] = ['UTC', 1706659200, 0];      // 2024-01-31
$bases[] = ['UTC', -62167219200, 0];    // 0000-01-01

function base(string $zone, int $ts, int $us, string $class = 'DateTime'): DateTimeInterface
{
    $d = new $class('@' . $ts);
    $d = $d->setTimezone(new DateTimeZone($zone));
    return $d->setMicrosecond($us);
}

$modify = [];
foreach ($bases as [$zone, $ts, $us]) {
    foreach ($modifiers as $m) {
        $d = base($zone, $ts, $us);
        try {
            $d->modify($m);
            $res = enc(show($d));
        } catch (Throwable $e) {
            $res = exc($e);
        }
        $e = DateTime::getLastErrors();
        $modify[] = [$zone, $ts, $us, $m, $res, $e === false ? false : $e];
    }
}

$errors = [];
foreach (array_merge($invalid, ['+1 day', '2024-02-30']) as $m) {
    $row = [enc($m)];
    foreach (['DateTime', 'DateTimeImmutable'] as $class) {
        $d = base('Europe/Vilnius', 1700000000, 0, $class);
        try {
            $r = $d->modify($m);
            $row[] = enc(show($r)) . ' / ' . enc(show($d));
        } catch (Throwable $e) {
            $row[] = exc($e) . ' / ' . enc(show($d));
        }
    }
    $d = base('Europe/Vilnius', 1700000000, 0);
    $r = date_modify($d, $m);
    $row[] = $r === false ? false : enc(show($r));
    $row[] = take_diags();
    $errors[] = $row;
}

emit('modify.gen.php', ['modify' => $modify, 'errors' => $errors]);

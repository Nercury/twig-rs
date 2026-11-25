<?php
// Generates array/minmax.json: min()/max() in their three call shapes.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/array/minmax.gen.php > array/minmax.json
//
// values:  enc() values
// pairs:   [i, j, codes, extra] for min($a, $b), max($a, $b) (direct calls: frameless),
//          min(...[$a, $b]), max(...[$a, $b]) (regular calls), min([$a, $b]), max([$a, $b])
// triples: [[i, j, k], codes, extra] for min($a, $b, $c), max($a, $b, $c)
// arrays:  [[indices], codes, extra] for min($array), max($array)
// errors:  [function, [args as enc()], res(), diags]
// codes:   one entry per call: the positions (digits) of the arguments/elements whose enc() equals
//          the result's, or "E" for an exception; extra: [[call index, err|null, diags], ...] for
//          calls that raised diagnostics or threw

declare(strict_types=0);
require __DIR__.'/../common.inc.php';
require __DIR__.'/../values.inc.php';
mt_srand(4242);

$values = [null, true, false, 0, 1, -1, 2, 0.0, -0.0, 1.0, 1.5, -1.5, NAN, INF, -INF, PHP_INT_MAX, PHP_INT_MIN,
    9007199254740992, 9007199254740993, 9007199254740992.0, 2.0 ** 63, -(2.0 ** 63), '', '0', '1', '1.0', '01',
    ' 1', '1 ', '-1', 'a', 'abc', 'A', '1abc', '1e0', '10', '9', '9a', 'INF', '9223372036854775808', [], [0], [1],
    [1, 2], [2, 1], ['a' => 1], ['b' => 1], new stdClass(), (object) ['a' => 1], (object) ['a' => 2],
    new S('abc'), new S('1'), Suit::Hearts, Suit::Spades, U::A];

function code($r, array $cands): string
{
    if ($r instanceof \Throwable) {
        return 'E';
    }
    $e = json_encode(enc($r));
    $s = '';
    foreach ($cands as $i => $c) {
        if (json_encode(enc($c)) === $e) {
            $s .= $i;
        }
    }
    return $s === '' ? '?' : $s;
}

/** Runs the calls, returning [codes, extra]. */
function run(array $calls, array $cands): array
{
    $codes = [];
    $extra = [];
    foreach ($calls as $k => $f) {
        [$r, $d] = capture($f);
        $codes[] = code($r, $cands);
        if ($d || $r instanceof \Throwable) {
            $extra[] = [$k, $r instanceof \Throwable ? res($r)['err'] : null, diags($d)];
        }
    }
    return [$codes, $extra];
}

$pairs = [];
foreach ($values as $i => $a) {
    foreach ($values as $j => $b) {
        [$codes, $extra] = run([
            static fn () => min($a, $b),
            static fn () => max($a, $b),
            static fn () => min(...[$a, $b]),
            static fn () => max(...[$a, $b]),
            static fn () => min([$a, $b]),
            static fn () => max([$a, $b]),
        ], [$a, $b]);
        $pairs[] = [$i, $j, $codes, $extra];
    }
}

$n = count($values);
$triples = [];
for ($t = 0; $t < 4000; $t++) {
    $idx = [mt_rand(0, $n - 1), mt_rand(0, $n - 1), mt_rand(0, $n - 1)];
    [$a, $b, $c] = [$values[$idx[0]], $values[$idx[1]], $values[$idx[2]]];
    [$codes, $extra] = run([
        static fn () => min($a, $b, $c),
        static fn () => max($a, $b, $c),
    ], [$a, $b, $c]);
    $triples[] = [$idx, $codes, $extra];
}

$arrays = [];
for ($t = 0; $t < 2000; $t++) {
    $len = mt_rand(1, 9);
    $idx = [];
    for ($k = 0; $k < $len; $k++) {
        $idx[] = mt_rand(0, $n - 1);
    }
    $arr = array_map(static fn ($i) => $values[$i], $idx);
    [$codes, $extra] = run([
        static fn () => min($arr),
        static fn () => max($arr),
        static fn () => min(...$arr),
        static fn () => max(...$arr),
    ], $arr);
    $arrays[] = [$idx, $codes, $extra];
}

$errors = [];
foreach ([[], [1], ['a'], [null], [[]], [new stdClass()], [[], 1]] as $args) {
    foreach (['min', 'max'] as $fn) {
        [$r, $d] = capture(static fn () => $fn(...$args));
        $errors[] = [$fn, array_map('enc', $args), res($r), diags($d)];
    }
}

emit('array/minmax.gen.php', [
    'values' => array_map('enc', $values),
    'pairs' => $pairs,
    'triples' => $triples,
    'arrays' => $arrays,
    'errors' => $errors,
    'messages' => messages(),
]);

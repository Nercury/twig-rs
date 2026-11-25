<?php
// Generates operators.json: every binary operator Twig compiles to (+ - * / % ** . & | ^ << >> xor,
// and Twig's // as (int) floor(a / b)) and the unary operators (-, +, ~, !) over a value matrix,
// with diagnostics and exceptions.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/operators.gen.php > operators.json
//
// values:   typed values (values.inc.php enc())
// binary:   {op: [[i, j, result, diags], ...]} for values[i] <op> values[j]
// unary:    {op: [[i, result, diags], ...]}
// result:   {"ok": typed} | {"err": [class, message index]};  diags: [[level, message index], ...]
// messages: hex strings indexed by the message indices

declare(strict_types=0);
require __DIR__.'/common.inc.php';
require __DIR__.'/values.inc.php';

$values = [
    null, true, false, 0, 1, -1, 2, 3, 7, -7, 63, 64, -64, PHP_INT_MAX, PHP_INT_MIN, 4611686018427387904,
    0.0, -0.0, 0.5, 1.5, -1.5, 7.9, 3.9, 1e20, -1e20, 9.2233720368547758E+18, NAN, INF, -INF, 0.1,
    '', '0', '1', '3', '7', '-3', '1.5', '7.5', '-0.0', '1e3', '1e', ' 1', '1 ', ' ', "\n", '1e-3',
    'abc', '12abc', '1x', '2y', '0x1A', '9.5e18', '9223372036854775808', '1e400', "\0", 'a', 'AB',
    '9223372036854775808e+', '.5', '-',
    [], [1], [1, 2], ['a' => 1], [5 => 3, 0 => 9], [1 => 'x'],
    new stdClass(), new S(), new S('7'), Suit::Hearts,
];

$binary = [
    '+' => static fn ($a, $b) => $a + $b,
    '-' => static fn ($a, $b) => $a - $b,
    '*' => static fn ($a, $b) => $a * $b,
    '/' => static fn ($a, $b) => $a / $b,
    '%' => static fn ($a, $b) => $a % $b,
    '**' => static fn ($a, $b) => $a ** $b,
    '.' => static fn ($a, $b) => $a . $b,
    '&' => static fn ($a, $b) => $a & $b,
    '|' => static fn ($a, $b) => $a | $b,
    '^' => static fn ($a, $b) => $a ^ $b,
    '<<' => static fn ($a, $b) => $a << $b,
    '>>' => static fn ($a, $b) => $a >> $b,
    'xor' => static fn ($a, $b) => $a xor $b,
    '//' => static fn ($a, $b) => (int) floor($a / $b),
];
$unary = [
    '-' => static fn ($a) => -$a,
    '+' => static fn ($a) => +$a,
    '~' => static fn ($a) => ~$a,
    '!' => static fn ($a) => !$a,
];

$out = ['values' => array_map('enc', $values), 'binary' => [], 'unary' => []];
foreach ($binary as $op => $f) {
    $rows = [];
    foreach ($values as $i => $a) {
        foreach ($values as $j => $b) {
            [$r, $d] = capture(static fn () => $f($a, $b));
            $rows[] = [$i, $j, res($r), diags($d)];
        }
    }
    $out['binary'][$op] = $rows;
}
foreach ($unary as $op => $f) {
    $rows = [];
    foreach ($values as $i => $a) {
        [$r, $d] = capture(static fn () => $f($a));
        $rows[] = [$i, res($r), diags($d)];
    }
    $out['unary'][$op] = $rows;
}

// One JSON document; rows on their own lines to keep diffs readable.
echo '{"generator":"operators.gen.php","php":"', PHP_VERSION, "\",\n";
echo '"values":', json_encode($out['values'], JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), ",\n";
foreach (['binary', 'unary'] as $section) {
    echo '"', $section, "\":{\n";
    $ops = array_keys($out[$section]);
    foreach ($ops as $n => $op) {
        echo json_encode((string) $op), ":[\n";
        $rows = $out[$section][$op];
        foreach ($rows as $k => $row) {
            echo json_encode($row, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), $k + 1 < count($rows) ? ",\n" : "\n";
        }
        echo ']', $n + 1 < count($ops) ? ",\n" : "\n";
    }
    echo "},\n";
}
echo '"messages":', json_encode(messages(), JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), "}\n";

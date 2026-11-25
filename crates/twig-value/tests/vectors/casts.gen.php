<?php
// Generates casts.json: (bool), (int), (float), (string), gettype(), get_debug_type() and the array key
// ([$v => 1]) of values of every type, with diagnostics and exceptions.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/casts.gen.php > casts.json

declare(strict_types=0);
require __DIR__.'/common.inc.php';
mt_srand(31337);

$values = [null, true, false, 0, 1, -1, 42, PHP_INT_MAX, PHP_INT_MIN, PHP_INT_MAX - 1, PHP_INT_MIN + 1,
    0.0, -0.0, 1.0, -1.0, 1.5, -1.5, 0.5, -0.5, 0.9, -0.9, 0.1 + 0.2, NAN, INF, -INF, 1e14, 1e15, 1e20, -1e20,
    1e19, -1e19, 2.0 ** 63, -(2.0 ** 63), 2.0 ** 64, -(2.0 ** 64), 2.0 ** 64 + 8192, 1e30, -1e30, 1e300,
    4.9e-324, -4.9e-324, 9007199254740993.0, 123456789012345.0, 1e-5, 1.7976931348623157E+308,
    '', '0', '0.0', '00', '1', '-1', 'abc', ' 1', '1 ', '1e3', 'NAN', 'INF', '1e400', '-0', '0x1A', ' ', "\0",
    'Array', '12abc', '9223372036854775808', '1e100', "\xff\xfe", 'true', 'false', 'null',
    [], [1], [1, 2, 3], new stdClass()];
// Random doubles over a wide range (float->int wrapping and formatting).
for ($i = 0; $i < 1500; $i++) {
    $e = mt_rand(-20, 90);
    $f = (mt_rand() / mt_getrandmax()) * (2.0 ** $e);
    if (mt_rand(0, 1)) {
        $f = -$f;
    }
    $values[] = $f;
    if ($i % 3 === 0) {
        $values[] = (float) @(int) $f;
    }
}
for ($i = 0; $i < 300; $i++) {
    $values[] = mt_rand(PHP_INT_MIN, PHP_INT_MAX);
}

$cases = [];
foreach ($values as $v) {
    $c = ['v' => typed($v)];
    $casts = [
        'bool' => static fn () => (bool) $v,
        'int' => static fn () => (int) $v,
        'float' => static fn () => (float) $v,
        'string' => static fn () => (string) $v,
    ];
    foreach ($casts as $name => $f) {
        [$r, $d] = capture($f);
        $c[$name] = result($r);
        $c[$name.'_diag'] = $d;
    }
    $c['gettype'] = gettype($v);
    $c['get_debug_type'] = get_debug_type($v);
    [$r, $d] = capture(static fn () => array_key_first([$v => 1]));
    $c['key'] = result($r);
    $c['key_diag'] = $d;
    $cases[] = $c;
}
emit('casts.gen.php', ['cases' => $cases]);

<?php
// Generates array_keys.json: offset normalisation of every value type in each access context
// (read, write, isset, unset, array_key_exists) with PHP 8.5 diagnostics and TypeErrors.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/array_keys.gen.php > array_keys.json
//
// "key" is the normalised key (verified by hitting a prepared element), or the thrown exception.

declare(strict_types=0);
require __DIR__.'/common.inc.php';

$values = [null, true, false, 0, 1, -1, PHP_INT_MAX, PHP_INT_MIN, 0.0, -0.0, 1.5, -1.5, 0.9, 1e20, -1e20,
    NAN, INF, -INF, 2.0 ** 63, 1e19, 2.0 ** 53 + 2, '', '0', '1', '-1', '01', '-0', ' 1', '1 ', '+1', '1.0',
    '1.5', '1e3', '0x1', 'a', '9223372036854775807', '9223372036854775808', '-9223372036854775808',
    '-9223372036854775809', "1\0", "\xff", [], [1], new stdClass()];

$rows = [];
foreach ($values as $v) {
    // Expected key: the write context (array literal) defines it.
    [$k] = capture(static fn () => array_key_first([$v => 1]));
    if ($k instanceof \Throwable) {
        $contexts = [
            'write' => static fn () => array_key_first([$v => 1]),
            'read' => static function () use ($v) {
                $a = [];
                return $a[$v];
            },
            'isset' => static function () use ($v) {
                $a = [];
                return isset($a[$v]);
            },
            'unset' => static function () use ($v) {
                $a = [];
                unset($a[$v]);
                return null;
            },
            'array_key_exists' => static function () use ($v) {
                $a = [];
                return array_key_exists($v, $a);
            },
        ];
    } else {
        $contexts = [
            'write' => static fn () => array_key_first([$v => 1]),
            'read' => static function () use ($v, $k) {
                $a = [$k => 'hit'];
                return $a[$v] === 'hit' ? $k : 'MISS';
            },
            'isset' => static function () use ($v, $k) {
                $a = [$k => 'hit'];
                return isset($a[$v]) ? $k : 'MISS';
            },
            'unset' => static function () use ($v, $k) {
                $a = [$k => 'hit'];
                unset($a[$v]);
                return [] === $a ? $k : 'MISS';
            },
            'array_key_exists' => static function () use ($v, $k) {
                $a = [$k => 'hit'];
                return array_key_exists($v, $a) ? $k : 'MISS';
            },
        ];
    }
    foreach ($contexts as $ctx => $f) {
        [$r, $d] = capture($f);
        $rows[] = ['v' => typed($v), 'ctx' => $ctx, 'key' => result($r), 'diag' => $d];
    }
}
emit('array_keys.gen.php', ['contexts' => $rows]);

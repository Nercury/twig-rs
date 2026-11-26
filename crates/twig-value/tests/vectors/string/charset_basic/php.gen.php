<?php
// php.json: iconv() as a PHP function: argument coercion (null deprecations, scalars, objects with __toString(),
// TypeErrors and their order), the 64-byte name limit and NUL bytes in names. Uses the shared value helpers of
// tests/vectors: a case is {"a": [enc(arg)...], "r": {"ok": typed} | {"err": [class, hex message]},
// "d": [[level, hex message]...]}.

require __DIR__ . '/../../common.inc.php';
require __DIR__ . '/../../values.inc.php';

$args = [
    ['UTF-8', 'ISO-8859-1', "a\u{e9}"],
    [null, 'UTF-8', 'a'],
    ['UTF-8', null, 'a'],
    ['UTF-8', 'UTF-16BE', null],
    [null, null, null],
    [[], 'UTF-8', 'a'],
    ['UTF-8', [], 'a'],
    ['UTF-8', 'UTF-8', []],
    [[], [], []],
    [null, [], 'a'],
    [new S('UTF-8'), new S('ISO-8859-1//TRANSLIT'), new S("\u{20ac}")],
    [new P(), 'UTF-8', 'a'],
    ['UTF-8', new P(), 'a'],
    ['UTF-8', 'ASCII', new P()],
    [1, 'UTF-8', 'a'],
    ['UTF-8', 'UTF-8', 1.5],
    ['UTF-8', 'UTF-8', 0.1 + 0.2],
    ['UTF-8', 'UTF-8', true],
    ['UTF-8', 'UTF-8', false],
    ['UTF-8', 'UTF-8', 123],
    [true, 'UTF-8', 'a'],
    ['UTF-8', 'UCS-2', -5],
    [Suit::Hearts, 'UTF-8', 'a'],
    ['UTF-8', 'UTF-8', Suit::Hearts],
    ['EUC-JP', 'UTF-8', 'a'],
    ['UTF-8', 'utf-7', ''],
    ['EUC-JP', 'NOPE', 'a'],
    ['NOPE', 'EUC-JP', ''],
    ['UTF-7', 'ISO-2022-JP', 'a'],
    [str_repeat('A', 63), 'UTF-8', 'a'],
    [str_repeat('A', 64), 'UTF-8', 'a'],
    ['UTF-8', str_repeat(' ', 59) . 'UTF-8', 'a'],
    ['UTF-8', str_repeat(' ', 58) . 'UTF-8', 'a'],
    ["UTF-8\x00" . str_repeat('x', 57), 'UTF-8', 'a'],
    ["UTF-8\x00" . str_repeat('x', 58), 'UTF-8', 'a'],
    ["UTF-8\x00x", "ASCII\x00y", "a\u{e9}"],
    ["NOPE\x00x", "ASCII\x00y", 'a'],
    ["EUC-JP\x00x", 'UTF-8', 'a'],
    ['', '', "\xff"],
    ['UTF-8', 'ASCII//IGNORE', "a\u{e9}\xff"],
    ['UTF-8', 'ASCII', "a\u{e9}"],
    ['UTF-8', 'UTF-16', "a\xe2\x82"],
];
$cases = [];
foreach ($args as $a) {
    [$r, $d] = capture(static fn () => iconv(...$a));
    $cases[] = ['a' => array_map('enc', $a), 'r' => result($r), 'd' => $d];
}
emit('php.gen.php', ['cases' => $cases]);

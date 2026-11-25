<?php
// Generates url.json: rawurlencode()/urlencode() of every byte, http_build_query() with nested arrays and
// objects (visibility, uninitialised typed properties, cycles), enums, floats, prefixes, separators and both
// encodings.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/string/url.gen.php > string/url.json

declare(strict_types=0);
require __DIR__.'/calls.inc.php';

mt_srand(633);
$cases = [];
$all = '';
for ($b = 0; $b < 256; $b++) {
    $all .= chr($b);
    $cases[] = ['rawurlencode', [chr($b)]];
    $cases[] = ['urlencode', [chr($b)]];
}
foreach (['', $all, "a b+c/d~e_f.g-h*i'j(k)l!m\x00\xff", str_repeat('ab ~', 10)] as $s) {
    $cases[] = ['rawurlencode', [$s]];
    $cases[] = ['urlencode', [$s]];
}
foreach (odd_values() as $v) {
    $cases[] = ['rawurlencode', [$v]];
    $cases[] = ['urlencode', [$v]];
}

$o = new stdClass();
$o->a = 1;
$o->{'b c'} = 'd e';
$o->{'0'} = 'zero';
$o->n = null;
$o->nested = ['x' => [1, 2], 'y' => new P()];
$self = new stdClass();
$self->a = 1;
$self->self = $self;
$self->arr = ['x' => $self, 'y' => 2];
$self->tail = 'end';
$t = new T();
$data = [
    [],
    ['a' => 1, 'b' => [1, 2], 'c d' => 'e f', 'g' => null, 'h' => true, 'i' => false, 'j' => 1.5, 'k' => ['x' => 'y']],
    [1, 2, [3, [4, 'five' => 5]]],
    ['f' => [0.1 + 0.2, 1e15, 1e14, -0.0, 1e100, -1.5e-10, NAN, INF, -INF, 100.0]],
    ['e' => Suit::Hearts, 'n' => Num::One],
    ['e' => U::A, 'after' => 1],
    ['obj' => $o],
    ['p' => new P(), 't' => $t],
    ['self' => $self],
    ['~a b' => '~c d', 'x/y' => 'a&b=c'],
    [PHP_INT_MIN => 'min', -1 => 'neg', 7 => ['k' => 'v']],
    ['empty' => [], 'after' => 'x'],
    [[], 'x'],
    ['s' => new S('stringable')],
    ['a' => ['b' => ['c' => ['d' => 'deep']]]],
    ['nulls' => [null, null], 'x' => 1],
    ['k' => "\xFF\x00"],
];
foreach ($data as $d) {
    $cases[] = ['http_build_query1', [$d]];
    $cases[] = ['http_build_query4', [$d, '', '&', PHP_QUERY_RFC3986]];
    $cases[] = ['http_build_query4', [$d, 'p_', ';', PHP_QUERY_RFC1738]];
    $cases[] = ['http_build_query4', [$d, 'n', '', PHP_QUERY_RFC3986]];
    $cases[] = ['http_build_query3', [$d, '', null]];
    $cases[] = ['http_build_query2', [$d, 'pre']];
    $cases[] = ['http_build_query4', [$d, '', '&amp;', 3]];
}
$cases[] = ['http_build_query1', [$o]];
$cases[] = ['http_build_query1', [$self]];
$cases[] = ['http_build_query2', [new P(), 'x']];
$cases[] = ['http_build_query1', [new ArrayObject(['a' => 1])]];
$cases[] = ['http_build_query4', [['a b' => 'c d'], '', '&', 2 + (1 << 32)]];
$cases[] = ['http_build_query4', [['a b' => 'c d'], '', '&', -2]];
foreach (odd_values() as $v) {
    $cases[] = ['http_build_query1', [$v]];
    $cases[] = ['http_build_query2', [[5 => 'x'], $v]];
    $cases[] = ['http_build_query3', [['a' => 1, 'b' => 2], '', $v]];
    $cases[] = ['http_build_query4', [['a b' => 1], '', '&', $v]];
    $cases[] = ['http_build_query1', [['v' => $v]]];
}
for ($k = 0; $k < 400; $k++) {
    $arr = [];
    $n = mt_rand(0, 4);
    for ($i = 0; $i < $n; $i++) {
        $key = mt_rand(0, 2) ? rand_str("ab ~&=", 0, 3) : mt_rand(-2, 5);
        $arr[$key] = mt_rand(0, 3) ? rand_str("xy ~+%", 0, 4) : [rand_str('z ', 0, 2), mt_rand(0, 1) ? null : 7];
    }
    $cases[] = ['http_build_query4', [$arr, ['', 'p'][mt_rand(0, 1)], ['&', ';'][mt_rand(0, 1)], mt_rand(1, 2)]];
}

emit('url.gen.php', ['cases' => run_cases($cases)]);

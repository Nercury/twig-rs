<?php
// Generates dumps.json: var_dump(), var_export() and print_r() of scalars, strings with special
// bytes, nested arrays, objects of several classes (visibility, uninitialized typed properties,
// malformed mangled names), enums and recursive object graphs.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/dumps.gen.php > dumps.json
//
// cases: {"v": typed value, "var_dump": hex, "var_export": hex, "print_r": hex, "*_diag": [...]}
// Typed values are values.inc.php enc() plus {"t":"up","n":k}: the object k levels up the
// current object path (recursive graphs). Object handles (#N) differ between PHP and Rust; the
// test normalises them.

declare(strict_types=0);
require __DIR__.'/common.inc.php';
require __DIR__.'/values.inc.php';

/** enc() with cycle detection: a back-reference becomes {"t":"up","n":k}. */
function enc_graph($v, array $path = []): array
{
    if (is_array($v)) {
        $items = [];
        foreach ($v as $k => $x) {
            $items[] = [is_int($k) ? ['t' => 'int', 'v' => $k] : ['t' => 'string', 'hex' => bin2hex($k)], enc_graph($x, $path)];
        }
        return ['t' => 'array', 'items' => $items];
    }
    if (is_object($v) && !($v instanceof \UnitEnum) && !($v instanceof \Closure)) {
        foreach ($path as $depth => $o) {
            if ($o === $v) {
                return ['t' => 'up', 'n' => count($path) - $depth - 1];
            }
        }
        $path[] = $v;
        $props = [];
        foreach ((array) $v as $k => $x) {
            $props[] = [bin2hex((string) $k), enc_graph($x, $path)];
        }
        return ['t' => 'object', 'class' => get_class($v), 'props' => $props];
    }
    return enc($v);
}

$values = [
    null, true, false, 0, -1, 42, PHP_INT_MAX, PHP_INT_MIN, 0.0, -0.0, 1.0, -1.0, 0.1, 0.1 + 0.2, 1.5, 1e25, -1e25,
    1e15, 1e17, 123456789012345.678, 1e14, 0.00001, 0.0001, NAN, INF, -INF, 4.9e-324, 1.5e-7, 2.0 ** 63,
    '', 'two', "q'uo\"te\\ \0nul\n", "\xff\xfe", "multi\nline", "' . \"\\0\" . '", "tab\there", "\0", '\\',
    [], [1, 2, 3], ['a' => 1, 'b' => [true, false, null]], [-5 => 'x', 'y'], ["k'ey" => 'v', "nu\0l" => 1, 'b\\s' => 2],
    [[]], [[1, [2, [3, []]]]], ['f' => 1.0, 'g' => [-0.0, 0.5, NAN]], ['s' => "a\nb", 9 => -9, '-1' => 'm', '01' => 'z'],
    new stdClass(), (object) ['a' => 1, 'b c' => [1.0, -0.0], '0' => 'zero'],
    (object) ['nested' => (object) ['x' => [1, (object) []]], 'e' => []],
    new P(), new T(), new S('x'), Suit::Hearts, Num::One, U::A, [Suit::Spades, U::B, 'k' => Num::One],
    (object) ["\0A\0p" => 3, "\0*\0q" => 4, "\0ab" => 1, "\0\0x" => 2, "pl\0ain" => 5],
];
$p = new P();
$p->a = [1, 'two'];
$values[] = $p;
$t = new T();
$t->typed = 7;
$values[] = $t;
$values[] = ['list' => [new P(), new T()], 'obj' => $p];
// Recursive graphs.
$r = new stdClass();
$r->a = 1;
$r->self = $r;
$values[] = $r;
$r2 = new stdClass();
$r2->arr = [1, $r2, 'k' => [$r2]];
$values[] = $r2;
$outer = new stdClass();
$inner = new stdClass();
$outer->inner = $inner;
$inner->outer = $outer;
$inner->list = [$inner, $outer];
$values[] = $outer;
$values[] = [$r, $r];

$cases = [];
foreach ($values as $v) {
    $c = ['v' => enc_graph($v)];
    [$r1, $d1] = capture(static function () use ($v) {
        ob_start();
        var_dump($v);
        return ob_get_clean();
    });
    $c['var_dump'] = bin2hex($r1);
    $c['var_dump_diag'] = $d1;
    [$r2_, $d2] = capture(static fn () => var_export($v, true));
    $c['var_export'] = bin2hex($r2_);
    $c['var_export_diag'] = $d2;
    [$r3, $d3] = capture(static fn () => print_r($v, true));
    $c['print_r'] = bin2hex($r3);
    $c['print_r_diag'] = $d3;
    $cases[] = $c;
}
emit('dumps.gen.php', ['cases' => $cases]);

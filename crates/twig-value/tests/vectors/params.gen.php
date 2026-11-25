<?php
// Generates params.json: coercive-mode parameter passing of values of every type.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/params.gen.php > params.json
//
// internal: builtins with an int, float, string, bool and int|float parameter (engine-semantics.md §11.1):
//   intdiv($v, 1), fdiv($v, 1), str_pad($v, 0), array_key_first(array_slice([5 => 'x'], 0, null, $v)), abs($v)
// user: userland functions with declared types (§11.2); the ", called in <file> on line <n>" suffix of the
//   TypeError message is stripped.
// Values are referenced by index into the VALUES list below; tests/vectors.rs builds the same list.

declare(strict_types=0);
require __DIR__.'/common.inc.php';

class S
{
    public function __toString(): string
    {
        return '12';
    }
}

function u_int(int $x) { return $x; }
function u_float(float $x) { return $x; }
function u_string(string $x) { return $x; }
function u_bool(bool $x) { return $x; }
function u_nint(?int $x) { return $x; }
function u_nstring(?string $x) { return $x; }
function u_int_float(int|float $x) { return $x; }
function u_string_int(string|int $x) { return $x; }
function u_int_bool(int|bool $x) { return $x; }
function u_float_string(float|string $x) { return $x; }
function u_bool_string_null(bool|string|null $x) { return $x; }
function u_false_int(false|int $x) { return $x; }
function u_array(array $x) { return $x; }
function u_iterable(iterable $x) { return $x; }
function u_countable(Countable|array $x) { return $x; }
function u_object(object $x) { return $x; }
function u_nstringable(?Stringable $x) { return $x; }
function u_mixed(mixed $x) { return $x; }

$values = [null, true, false, 0, 1, -1, 7, PHP_INT_MAX, PHP_INT_MIN, 0.0, -0.0, 1.0, 1.5, -1.5, 2.5, 0.1, NAN, INF,
    -INF, 1e15, 1e20, -1e20, 2.0 ** 63, -(2.0 ** 63), 9007199254740993.0, '', '0', '1', '-1', '1.5', '1.0', ' 1',
    '1 ', "\n1\t", '1x', '1 x', 'abc', '1e2', '1e100', '-1e100', '.5', '5.', '9223372036854775807',
    '9223372036854775808', '-9223372036854775809', '-0', '00', '0x1A', ' ', "1\0", 'NAN', 'INF', 'true',
    [], [1, 2], new stdClass(), new S(), new ArrayObject([1])];

$internal = [
    'int' => static fn ($v) => intdiv($v, 1),
    'float' => static fn ($v) => fdiv($v, 1),
    'string' => static fn ($v) => str_pad($v, 0),
    'bool' => static fn ($v) => array_key_first(array_slice([5 => 'x'], 0, null, $v)),
    'number' => static fn ($v) => abs($v),
];
$user = ['u_int', 'u_float', 'u_string', 'u_bool', 'u_nint', 'u_nstring', 'u_int_float', 'u_string_int',
    'u_int_bool', 'u_float_string', 'u_bool_string_null', 'u_false_int', 'u_array', 'u_iterable', 'u_countable',
    'u_object', 'u_nstringable', 'u_mixed'];

function strip_called_in($r)
{
    if ($r instanceof \TypeError) {
        return new \TypeError(preg_replace('/, called in .* on line \d+$/', '', $r->getMessage()));
    }
    return $r;
}

$rows = [];
foreach ($values as $vi => $v) {
    $row = ['v' => $vi, 'typed' => typed($v)];
    foreach ($internal as $name => $f) {
        [$r, $d] = capture(static fn () => $f($v));
        $row[$name] = [result($r), $d];
    }
    foreach ($user as $fn) {
        [$r, $d] = capture(static fn () => $fn($v));
        $row[$fn] = [result(strip_called_in($r)), $d];
    }
    $rows[] = $row;
}
emit('params.gen.php', ['cases' => $rows]);

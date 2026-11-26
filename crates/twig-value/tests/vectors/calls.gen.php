<?php
// Generates calls.json: how PHP passes arguments to functions and checks what userland functions return.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/calls.gen.php > calls.json
//
// "args": one call per case, written as a list of sends: ["pos", v] (a positional argument), ["named", n, v]
//   (`n: v`), ["spread", [[k, v], ...]] (`...[k => v, ...]`, an array literal) and ["iter", [[k, v], ...]]
//   (`...pairs(...)`, a generator yielding the pairs, keys may repeat). Values are ints, strings and lists of ints.
//   User functions return their parameters (a variadic one as its array) as JSON; internal functions report "ok".
//   The ` in <file> on line <n>` that PHP adds after `passed` in "Too few arguments" messages is stripped.
// "returns": a userland function with a declared return type returns a value; the result is the value or the
//   chain of thrown exceptions (outermost first). "throw" cases run with an error handler that throws
//   ErrorException for every diagnostic.

declare(strict_types=0);
require __DIR__.'/common.inc.php';

function u_h($a, $b) { return [$a, $b]; }
function u_g($a, $b = 'B') { return [$a, $b]; }
function u_t($a, $b, $c) { return [$a, $b, $c]; }
function u_o($a = 'A', $b = 'B') { return [$a, $b]; }
function u_v($a, ...$r) { return [$a, $r]; }
function u_w(...$r) { return [$r]; }
function u_n() { return []; }
function u_ov($a = 'A', ...$r) { return [$a, $r]; }

/** Yields the [key, value] pairs. */
function pairs(array $pairs): \Generator
{
    foreach ($pairs as [$k, $v]) {
        yield $k => $v;
    }
}

function php_value($v): string
{
    return is_array($v) ? '['.implode(', ', array_map('php_value', $v)).']' : var_export($v, true);
}

/** The PHP argument list of a list of sends. */
function call_code(array $sends): string
{
    $parts = [];
    foreach ($sends as $send) {
        switch ($send[0]) {
            case 'pos':
                $parts[] = php_value($send[1]);
                break;
            case 'named':
                $parts[] = $send[1].': '.php_value($send[2]);
                break;
            case 'spread':
                $parts[] = '...['.implode(', ', array_map(static fn ($p) => php_value($p[0]).' => '.php_value($p[1]), $send[1])).']';
                break;
            case 'iter':
                $parts[] = '...pairs('.var_export($send[1], true).')';
                break;
        }
    }
    return implode(', ', $parts);
}

$internal = ['abs', 'str_pad', 'sprintf', 'max', 'array_merge'];

// [function, sends]
$calls = [
    // named arguments
    ['u_h', [['named', 'a', 1]]],
    ['u_h', [['named', 'b', 1]]],
    ['u_h', [['pos', 1], ['named', 'b', 2]]],
    ['u_h', [['named', 'b', 2], ['named', 'a', 1]]],
    ['u_h', [['pos', 1], ['named', 'a', 2]]],
    ['u_h', [['pos', 1], ['pos', 2], ['pos', 3]]],
    ['u_h', [['pos', 1], ['pos', 2], ['named', 'c', 3]]],
    ['u_h', [['pos', 1]]],
    ['u_h', []],
    ['u_g', [['named', 'b', 2]]],
    ['u_g', [['named', 'a', 1]]],
    ['u_t', [['pos', 1], ['named', 'c', 3]]],
    ['u_t', [['named', 'c', 3]]],
    ['u_t', [['named', 'c', 3], ['named', 'a', 1]]],
    ['u_o', [['named', 'b', 2]]],
    ['u_o', []],
    ['u_v', [['pos', 1], ['pos', 2], ['pos', 3]]],
    ['u_v', [['pos', 1], ['named', 'x', 2]]],
    ['u_v', [['named', 'a', 1], ['named', 'x', 2]]],
    ['u_v', [['named', 'x', 2]]],
    ['u_v', [['pos', 1], ['named', 'a', 2]]],
    ['u_v', [['pos', 1], ['named', 'r', 2]]],
    ['u_v', [['pos', 1], ['pos', 2], ['named', 'x', 3], ['named', 'y', 4]]],
    ['u_w', [['pos', 1], ['named', 'x', 2]]],
    ['u_w', []],
    ['u_n', [['pos', 1]]],
    ['u_n', [['named', 'x', 1]]],
    ['u_ov', [['named', 'x', 1]]],
    ['u_ov', [['pos', 1], ['pos', 2], ['named', 'x', 3]]],
    ['abs', [['named', 'num', 5]]],
    ['abs', [['pos', 5], ['named', 'num', 5]]],
    ['abs', [['pos', 5], ['pos', 6]]],
    ['abs', []],
    ['abs', [['pos', 5], ['pos', 6], ['named', 'x', 1]]],
    ['str_pad', [['named', 'string', 'x']]],
    ['str_pad', [['named', 'length', 3]]],
    ['str_pad', [['pos', 'x'], ['pos', 3], ['named', 'pad_type', 0]]],
    ['str_pad', [['pos', 'x'], ['named', 'pad_type', 0]]],
    ['str_pad', [['pos', 'x'], ['pos', 3], ['pos', ' '], ['pos', 0], ['pos', 9]]],
    ['str_pad', [['named', 'pad_string', '-'], ['named', 'length', 3], ['named', 'string', 'x']]],
    ['sprintf', [['named', 'format', '']]],
    ['sprintf', [['pos', '%s'], ['pos', 1], ['named', 'x', 2]]],
    ['sprintf', [['named', 'x', 2]]],
    ['sprintf', [['named', 'values', 1]]],
    ['sprintf', []],
    ['max', [['named', 'value', [1, 2]]]],
    ['max', [['pos', 1], ['named', 'value', 2]]],
    ['array_merge', []],
    ['array_merge', [['named', 'x', [1]]]],
    // array literals unpacked
    ['u_h', [['spread', [['a', 1], ['b', 2]]]]],
    ['u_h', [['spread', [['b', 2], ['a', 1]]]]],
    ['u_h', [['pos', 1], ['spread', [['b', 2]]]]],
    ['u_h', [['spread', [[0, 1], ['b', 2]]]]],
    ['u_h', [['spread', [[0, 1], ['a', 2]]]]],
    ['u_h', [['spread', [['zz', 1]]]]],
    ['u_h', [['spread', [['a', 1], [0, 2]]]]],
    ['u_h', [['spread', [['zz', 1], [0, 2]]]]],
    ['u_h', [['spread', [['b', 1], ['b', 2]]]]],
    ['u_h', [['spread', [[5, 1], [9, 2]]]]],
    ['u_h', [['spread', [['1', 1], ['0', 2]]]]],
    ['u_h', [['spread', []]]],
    ['abs', [['pos', 1], ['spread', [[0, 1], ['a', 2]]]]],
    ['abs', [['pos', 1], ['pos', 2], ['spread', [['a', 2]]]]],
    ['abs', [['spread', [[0, 1], [1, 2], [2, 3]]]]],
    ['abs', [['spread', [['num', 1], [0, 2]]]]],
    ['u_v', [['spread', [['a', 1], ['x', 2], ['y', 3]]]]],
    ['u_v', [['spread', [['x', 2], [0, 1]]]]],
    ['sprintf', [['pos', '%s'], ['spread', [['x', 1]]]]],
    ['sprintf', [['spread', [['x', 1], [0, '%s']]]]],
    ['str_pad', [['spread', [['length', 3], ['string', 'x']]]]],
    ['str_pad', [['pos', 'x'], ['spread', [['string', 'y']]]]],
    ['max', [['spread', [['value', [1, 2]], [0, 3]]]]],
    ['max', [['spread', [['zz', 1], [0, 3]]]]],
    // generators unpacked
    ['u_t', [['iter', [['a', 1], ['a', 2]]]]],
    ['u_v', [['pos', 1], ['iter', [['x', 1], ['x', 2]]]]],
    ['u_w', [['iter', [['a', 1], ['a', 2]]]]],
    ['u_w', [['iter', [['a', 1], [0, 2]]]]],
    ['sprintf', [['pos', '%s'], ['iter', [['a', 1], ['a', 2]]]]],
    ['sprintf', [['pos', '%s'], ['iter', [['a', 1]]]]],
    ['array_merge', [['pos', [1]], ['iter', [['a', [2]], ['a', [3]]]]]],
    ['u_h', [['iter', [['0', 1], ['1', 2]]]]],
    ['u_h', [['iter', [['01', 1]]]]],
    ['u_h', [['iter', [[1.5, 1]]]]],
    ['u_h', [['iter', [[null, 1]]]]],
    ['u_h', [['iter', [['zz', 1], [0, 2]]]]],
    ['u_h', [['iter', [['a', 1], [0, 2]]]]],
    ['u_h', [['iter', [[5, 1], [7, 2]]]]],
    ['u_h', [['iter', [['a', 1], ['a', 2]]]]],
    ['u_h', [['iter', [['b', 1], ['a', 2], ['zz', 3]]]]],
    ['u_t', [['pos', 1], ['iter', [['c', 3], ['b', 2]]]]],
    ['abs', [['iter', [['num', 1], ['num', 2]]]]],
    // several unpacks
    ['u_t', [['spread', [['a', 1]]], ['spread', [[0, 2]]]]],
    ['u_t', [['spread', [['b', 1]]], ['spread', [[0, 2]]]]],
    ['u_t', [['spread', [['a', 1]]], ['spread', [[0, 2], [1, 3]]]]],
    ['u_t', [['spread', [['a', 1]]], ['spread', [[0, 2], [1, 3], [2, 4]]]]],
    ['u_t', [['spread', [['a', 1]]], ['spread', [[0, 2]]], ['spread', [['b', 3]]]]],
    ['u_t', [['spread', [['a', 1]]], ['spread', [[0, 2]]], ['spread', [['c', 3]]]]],
    ['u_h', [['spread', [['a', 1]]], ['spread', [[0, 2]]]]],
    ['u_h', [['spread', [['a', 1]]], ['spread', [[0, 2], [1, 3]]]]],
    ['u_h', [['spread', [['b', 1]]], ['spread', [[0, 2]]]]],
    ['u_v', [['spread', [['a', 1]]], ['spread', [[0, 2]]]]],
    ['u_v', [['spread', [['a', 1], ['x', 2]]], ['spread', [[0, 3]]]]],
    ['u_v', [['spread', [['x', 2]]], ['spread', [[0, 1]]]]],
    ['u_w', [['spread', [['x', 2]]], ['spread', [[0, 1]]]]],
    ['abs', [['spread', [['num', 1]]], ['spread', [[0, 2]]]]],
    ['str_pad', [['spread', [['length', 5]]], ['spread', [[0, 'x']]]]],
    ['str_pad', [['spread', [['string', 'x'], ['length', 5]]], ['spread', [[0, '-']]]]],
    ['str_pad', [['pos', 'x'], ['spread', [['length', 5]]], ['iter', [[0, '-'], ['pad_type', 0]]]]],
    ['u_t', [['spread', [['a', 1]]], ['iter', [[0, 2], ['zz', 3]]]]],
    ['u_h', [['spread', [['a', 1]]], ['iter', [['b', 2], [0, 3]]]]],
];

$args = [];
foreach ($calls as [$fn, $sends]) {
    $code = $fn.'('.call_code($sends).')';
    [$r, $d] = capture(static fn () => eval('return '.$code.';'));
    if ($r instanceof \Throwable) {
        $msg = preg_replace('/ passed in .* on line \d+ and /', ' passed and ', $r->getMessage());
        $result = ['err' => [get_class($r), $msg]];
    } elseif (in_array($fn, $internal, true)) {
        $result = ['ok' => true];
    } else {
        $result = ['ok' => json_encode($r, JSON_THROW_ON_ERROR)];
    }
    $args[] = ['fn' => $fn, 'php' => $code, 'sends' => $sends, 'result' => $result, 'diagnostics' => $d];
}

class S
{
    public function __toString(): string
    {
        return '12';
    }
}

class T
{
    public function __toString(): string
    {
        throw new \Exception('boom');
    }
}

$types = ['int', 'float', 'string', 'bool', '?int', '?string', 'int|float', 'string|int', 'int|bool', 'float|string',
    'bool|string|null', 'false|int', 'array', 'iterable', 'Countable|array', 'object', '?Stringable', 'mixed'];
$functions = [];
foreach ($types as $i => $type) {
    $functions[$type] = 'r_'.$i;
    eval('function r_'.$i.'($x): '.$type.' { return $x; }');
}

$values = [null, true, false, 0, 1, 7, 1.0, 1.5, -1.5, 1e20, NAN, INF, '', '0', '1', '1.5', ' 1', '1 ', '1x', 'abc',
    '1e2', '9223372036854775808', [], [1, 2], new stdClass(), new S(), new T(), new ArrayObject([1])];

function chain(\Throwable $e): array
{
    $chain = [];
    for (; null !== $e; $e = $e->getPrevious()) {
        $chain[] = [get_class($e), bin2hex($e->getMessage())];
    }
    return $chain;
}

$returns = [];
foreach (['collect', 'throw'] as $mode) {
    if ('throw' === $mode) {
        set_error_handler(static function (int $no, string $msg): bool {
            throw new \ErrorException($msg, 0, $no);
        });
    }
    foreach ($values as $vi => $v) {
        foreach ($functions as $type => $fn) {
            [$r, $d] = capture(static fn () => $fn($v));
            $returns[] = [
                'mode' => $mode,
                'type' => $type,
                'fn' => $fn,
                'v' => $vi,
                'typed' => typed($v),
                'result' => $r instanceof \Throwable ? ['err' => chain($r)] : ['ok' => typed($r)],
                'diagnostics' => $d,
            ];
        }
    }
}

emit('calls.gen.php', ['args' => $args, 'returns' => $returns]);

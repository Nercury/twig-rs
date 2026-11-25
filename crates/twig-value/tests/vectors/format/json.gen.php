<?php
// Generates format/json.json: json_encode() against PHP 8.5.13.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/format/json.gen.php > format/json.json
//
// values:    typed values (../values.inc.php enc(); classes S, P, T, stdClass, ArrayObject, enums,
//            closures, J, JSelf, JThrow of classes.inc.php)
// flags:     the flag combinations of the matrix
// matrix:    [value index, [[flag index, result, error code, diags?], ...]] for json_encode(value, flags)
// strings:   [string (JSON text or {"hex"}), [[flag index, result, error code], ...]]
// floats:    [double bits, result without flags, result with JSON_PRESERVE_ZERO_FRACTION]
// depth:     [value index, flags, depth, result, error code]
// deep:      [levels, depth, result, error code] for 'x' nested in `levels` lists, JSON_PARTIAL_OUTPUT_ON_ERROR
// cases:     [[argument value indices], result, error code, diags?] for whole calls (coercions, errors)
// scenarios: [name, flags, result, error code] for object graphs with cycles (built by tests/format.rs)
// result:    the JSON text (JSON string or {"hex"}), false, or {"err": [class, message], "code": n}
// error code: json_last_error() after the call (reset to 0 before each call)

declare(strict_types=0);
require __DIR__.'/../common.inc.php';
require __DIR__.'/../values.inc.php';
require __DIR__.'/classes.inc.php';
mt_srand(20261129);

function jres($r)
{
    if ($r instanceof \Throwable) {
        return ['err' => [get_class($r), out_enc($r->getMessage())], 'code' => $r->getCode()];
    }
    return $r === false ? false : out_enc($r);
}

/** json_encode(...$args) after resetting json_last_error(): [result, error code, diags]. */
function jcall(array $args): array
{
    json_encode(null);
    [$r, $d] = capture(static fn () => json_encode(...$args));
    return [jres($r), json_last_error(), out_diags($d)];
}

function jrow(array $inputs, array $call): array
{
    [$r, $code, $d] = $call;
    $row = $inputs;
    $row[] = $r;
    $row[] = $code;
    if ($d) {
        $row[] = $d;
    }
    return $row;
}

$values = [];
function vi($v): int
{
    global $values;
    $values[] = enc($v);
    return count($values) - 1;
}

$std = new stdClass();
$std->a = 1;
$std->{'b c'} = [1.5, null];
$std->{'0'} = 'zero';
$std->{"\xffbad"} = 'bad key';
$dyn = new P();
$dyn->dynamic = 'd';
$t = new T();
$t2 = new T();
$t2->typed = 5;
$ao = new ArrayObject(['a' => 1, 5 => [2, 3], 'x' => new J('in')]);
$matrix_values = [
    null, true, false, 0, -1, PHP_INT_MAX, PHP_INT_MIN, 0.0, -0.0, 1.0, 0.1, 1e15, 1e17, 1e25, -1.5e-7,
    123456789.12345678, 5e-324, NAN, INF, -INF,
    '', 'abc', 'a/b', "\"\\/\x08\x0c\n\r\t\x00\x1f\x7f", "<a href='x'>&amp;</a>", "\u{e9}\u{1f600}\u{2028}\u{2029}",
    "bad \xff utf", '123', '-1.5e3', ' 7', '1e999', '0x1A',
    [], [1, 2, 3], [1 => 'a'], [0 => 'a', 2 => 'b'], ['a' => 1, 'b' => [true, null]], [-5 => 'neg'],
    ['0' => 'x', '1' => 'y'], ["\0a" => 1], ["bad\xffkey" => 1, 'ok' => 2], [[[]]], [[], (object) []],
    ['list' => [1, [2, [3]]], 'map' => ['k' => ['k2' => 'v']]], [NAN, 1, "\xff", 2], ["\xff", NAN],
    $std, new stdClass(), new P(), $dyn, $t, $t2, new S('str'), Suit::Hearts, Num::One, U::A, [U::A, 1],
    ['h' => Suit::Spades], fn () => 1, $ao, new ArrayObject([1, 2]), new ArrayObject(),
    new J(1), new J([1, 'a' => 2]), new J(new J('nested')), new J(null), new J(NAN), new J("\xff"),
    new J(U::B), new JSelf(), [new JSelf(), new J(2)], new JThrow(), [1, new JThrow()],
];
$flag_list = [
    0, JSON_HEX_TAG | JSON_HEX_AMP | JSON_HEX_APOS | JSON_HEX_QUOT, JSON_FORCE_OBJECT, JSON_NUMERIC_CHECK,
    JSON_UNESCAPED_SLASHES, JSON_PRETTY_PRINT, JSON_UNESCAPED_UNICODE, JSON_PARTIAL_OUTPUT_ON_ERROR,
    JSON_PRESERVE_ZERO_FRACTION, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_LINE_TERMINATORS,
    JSON_INVALID_UTF8_IGNORE, JSON_INVALID_UTF8_SUBSTITUTE, JSON_INVALID_UTF8_SUBSTITUTE | JSON_UNESCAPED_UNICODE,
    JSON_THROW_ON_ERROR, JSON_THROW_ON_ERROR | JSON_PARTIAL_OUTPUT_ON_ERROR, JSON_PRETTY_PRINT | JSON_FORCE_OBJECT,
    JSON_PRETTY_PRINT | JSON_PARTIAL_OUTPUT_ON_ERROR, JSON_NUMERIC_CHECK | JSON_PRESERVE_ZERO_FRACTION,
    JSON_INVALID_UTF8_IGNORE | JSON_INVALID_UTF8_SUBSTITUTE, (1 << 32) | JSON_PRETTY_PRINT, -1,
];
$matrix = [];
foreach ($matrix_values as $v) {
    $rows = [];
    foreach ($flag_list as $fi => $flags) {
        $rows[] = jrow([$fi], jcall([$v, $flags]));
    }
    $matrix[] = [vi($v), $rows];
}

$string_list = [
    '', 'abc', "a\"b\\c/d", "\x08\x0c\n\r\t", "\x00\x01\x1f\x20\x7f", "<>&'\"", "\u{e9}", "\u{1f600}",
    "\u{2028}\u{2029}", "\u{ffff}", "\u{10ffff}", "\u{fffd}", "\u{80}\u{7ff}\u{800}\u{fffe}\u{10000}",
    "\x80", "\xc0\x80", "\xc1\xbf", "\xc2", "\xc2\x41", "\xc2\xc2\xa9", "\xe0\x80\x80", "\xe0\xa0", "\xe0\xa0\x41",
    "\xe0\x9f\xbf", "\xed\xa0\x80", "\xed\xbf\xbf", "\xef\xbf", "\xf0\x80\x80\x80", "\xf0\x8f\xbf\xbf",
    "\xf0\x90\x80", "\xf0\x90\x80\x41", "\xf4\x8f\xbf\xbf", "\xf4\x90\x80\x80", "\xf5\x80\x80\x80", "\xf8\x88\x80\x80\x80",
    "\xff", "\xfe\xff", "a\xffb\xfec", "\xe2\x82", "\xe2\x82\xac\x80", "\xf0\x9f\x98", "\xf0\x9f\x98\x41",
    "\xe2\x82\xe2\x82\xac", "\xf0\x9f\xf0\x9f\x98\x80", "x\xc3", "\xc3\xa9\xff\xc3\xa9",
    '1', '-1', '1.5', '1e3', ' 1', '1 ', "\n1", '0x1A', '1e999', '-1e999', '9223372036854775807',
    '9223372036854775808', '-9223372036854775809', '.5', '5.', '-0', '-0.0', '00', '007', '1_000', '+1',
    '-.5e-3', '1e', 'e1', 'abc1', '0.1', '100000000000000000000', '1.0',
    str_repeat("\u{1f600}<a>", 20), str_repeat('x', 300) . "\xff",
];
$string_flags = [0, 1, 2, 3, 4, 6, 7, 9, 10, 11, 12, 13, 14, 17, 18, 20];
$strings = [];
foreach ($string_list as $str) {
    $rows = [];
    foreach ($string_flags as $fi) {
        $rows[] = jrow([$fi], jcall([$str, $flag_list[$fi]]));
    }
    $strings[] = [out_enc($str), $rows];
}

$floats = [];
for ($i = 0; $i < 15000; $i++) {
    $x = humanfloat();
    $floats[] = [fbits($x), jcall([$x])[0], jcall([$x, JSON_PRESERVE_ZERO_FRACTION])[0]];
}

$nest = static function (int $levels, $leaf) {
    $v = $leaf;
    for ($i = 0; $i < $levels; $i++) {
        $v = [$v];
    }
    return $v;
};
$depth = [];
$deep_values = [];
foreach ([0, 1, 2, 3, 5] as $levels) {
    $deep_values[] = $nest($levels, 1);
    $deep_values[] = $nest($levels, INF);
    $deep_values[] = $nest($levels, ['k' => "\xff"]);
}
$o3 = new stdClass();
$o3->a = new stdClass();
$o3->a->b = new stdClass();
$o3->a->b->c = [1];
$deep_values[] = $o3;
$deep_values[] = new J($nest(3, 1));
foreach ($deep_values as $v) {
    $idx = vi($v);
    foreach ([JSON_PARTIAL_OUTPUT_ON_ERROR, 0, JSON_PRETTY_PRINT | JSON_PARTIAL_OUTPUT_ON_ERROR, JSON_THROW_ON_ERROR] as $flags) {
        foreach ([-1, 0, 1, 2, 3, 4, 6, 512] as $d) {
            $depth[] = jrow([$idx, $flags, $d], jcall([$v, $flags, $d]));
        }
    }
}
// Deep lists of 'x' (not in the value table: emit() itself is limited to depth 512).
$deep = [];
foreach ([511, 512, 513, 600] as $levels) {
    $v = $nest($levels, 'x');
    foreach ([512, 600, 601, 2147483647] as $d) {
        $deep[] = jrow([$levels, $d], jcall([$v, JSON_PARTIAL_OUTPUT_ON_ERROR, $d]));
    }
}

$cases = [
    [],
    [1, 0, 512, 'x'],
    [1, '128'],
    [1, '1.5'],
    [1, 1.5],
    [1, 'x'],
    [1, null],
    [1, true],
    [1, []],
    [1, 0, '3'],
    [1, 0, 'x'],
    [1, 0, null],
    [[1], 0, 2147483648],
    [[1], 0, -2147483649],
    [[1], 0, PHP_INT_MAX],
    [[1], 0, 2147483647],
    [[1], 0, -2147483648],
    [[[1]], 0, 1.0],
    [new stdClass(), 1e20],
];
$case_rows = [];
foreach ($cases as $args) {
    $case_rows[] = jrow([array_map('vi', $args)], jcall($args));
}

$scenario_builders = [
    'std_self' => static function () {
        $o = new stdClass();
        $o->a = 1;
        $o->self = $o;
        return $o;
    },
    'std_mutual' => static function () {
        $a = new stdClass();
        $b = new stdClass();
        $a->x = 1;
        $a->b = $b;
        $b->a = $a;
        return [$a, 'tail'];
    },
    'std_shared' => static function () {
        $o = new stdClass();
        $o->v = 'shared';
        return [$o, $o, ['again' => $o]];
    },
    'j_self_value' => static function () {
        $j = new J();
        $j->v = $j;
        return $j;
    },
    'j_mutual' => static function () {
        $a = new J();
        $b = new J($a);
        $a->v = $b;
        return [1, $a];
    },
    'j_in_own_array' => static function () {
        $j = new J();
        $j->v = ['me' => $j, 'n' => 1];
        return $j;
    },
    'ao_self' => static function () {
        $ao = new ArrayObject();
        $ao['x'] = 1;
        $ao['self'] = $ao;
        return $ao;
    },
];
$scenarios = [];
foreach ($scenario_builders as $name => $build) {
    foreach ([0, JSON_PARTIAL_OUTPUT_ON_ERROR, JSON_PRETTY_PRINT | JSON_PARTIAL_OUTPUT_ON_ERROR, JSON_THROW_ON_ERROR] as $flags) {
        $scenarios[] = jrow([$name, $flags], jcall([$build(), $flags]));
    }
}

emit('format/json.gen.php', [
    'values' => $values,
    'flags' => $flag_list,
    'matrix' => $matrix,
    'strings' => $strings,
    'floats' => $floats,
    'depth' => $depth,
    'deep' => $deep,
    'cases' => $case_rows,
    'scenarios' => $scenarios,
]);

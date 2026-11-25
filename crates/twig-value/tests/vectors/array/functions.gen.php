<?php
// Generates array/functions.json: the array functions of builtins::array other than sorting,
// range(), min()/max() and randomness, with argument coercion, diagnostics and errors.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/array/functions.gen.php > array/functions.json
//
// values:   encn() values (ArrayIterator objects as {"t":"iter","array":encn(storage)})
// calls:    [function, [argument indices], callback name|null, resn(result), diags, callback calls]
//           (callbacks are array.inc.php's callbacks(); the argument list excludes the callback)

declare(strict_types=0);
require __DIR__.'/../common.inc.php';
require __DIR__.'/../values.inc.php';
require __DIR__.'/array.inc.php';

$values = [];
$index = [];
/** Adds a value to the table (by name) and returns its index. */
function v(string $name, $value): int
{
    global $values, $index;
    $values[] = $value;
    return $index[$name] = count($values) - 1;
}

foreach ([
    'null' => null, 'true' => true, 'false' => false, '0' => 0, '1' => 1, '2' => 2, '-1' => -1, '3' => 3,
    '5' => 5, '8' => 8, '10' => 10, '-10' => -10, '-5' => -5, '-2' => -2, '1.5' => 1.5, '1.0' => 1.0, '-0.5' => -0.5,
    'nan' => NAN, 'inf' => INF, 'big' => 1e20, 's' => '', 's0' => '0', 's1' => '1', 's2' => '2', 'sa' => 'a',
    'sb' => 'b', 'sx' => 'x', 's1.5' => '1.5', 's 1' => ' 1', 's01' => '01', 'sid' => 'id', 'sk' => 'k',
    'max' => PHP_INT_MAX, 'pad+' => 1073741825, 'pad-' => -1073741825, 'smissing' => 'missing',
] as $name => $value) {
    v($name, $value);
}

$h = [0, 1, 2, 3, 4];
unset($h[1], $h[3]);
$t = [7, 8, 9];
unset($t[2]);
$arrays = [
    'empty' => [],
    'list' => [1, 2, 3],
    'assoc' => ['a' => 1, 'b' => 2],
    'mixed' => [5 => 'x', 3 => 'y', 'k' => 'z'],
    'rev' => [1 => 1, 0 => 0],
    'numkeys' => ['10' => 'a', '01' => 'b', '-5' => 'c', 'x' => 'd'],
    'neg' => [-3 => 'neg', 7 => 'seven'],
    'falsy' => [null, false, 0, '', '0', 'a', [], [0], 0.0, '0.0', ' '],
    'nested' => ['a' => null, 'b' => [1], 'c' => 1.5, 'd' => 'str'],
    'holes' => $h,
    'trimmed' => $t,
    'maxkey' => [PHP_INT_MAX => 'max'],
    'keys' => [1, '1', 1.5, true, null, 'a', '', ' 1', '01', 2.0, false, 'b', 1e20, -0.0],
    'keyerr' => [1, [2], 'c'],
    'objkeys' => ['k', new S('sk')],
    'badobj' => ['k', new stdClass()],
    'flip' => ['a', 'b', 1, 'a', 2.5, null, true, '1', '01'],
    'rows' => [
        ['id' => 1, 'a' => 'x', 'b' => 'y'],
        ['id' => 2, 'a' => 'z'],
        ['b' => 'w'],
        ['id' => 3.5, 'a' => null],
        ['id' => null, 'a' => 'n'],
        ['id' => true, 'a' => 't'],
        ['id' => 'k', 0 => 'zero', 1 => 'one', 'a' => 'last'],
        5,
        'str',
    ],
    'objrows' => [
        (object) ['id' => 10, 'a' => 'o1', 0 => 'z'],
        (object) ['a' => 'o2'],
        new S('q'),
        ['id' => 'k', 'a' => 'arr'],
        (object) ['id' => 11, 'a' => null],
    ],
    'badrows' => [['id' => 1, 'a' => 'ok'], ['id' => [1], 'a' => 'bad'], ['id' => 3, 'a' => 'after']],
    'dupidx' => [['id' => 'x', 'a' => 1], ['id' => 'y', 'a' => 2], ['id' => 'x', 'a' => 3], ['a' => 4]],
    'maxidx' => [['id' => PHP_INT_MAX, 'a' => 1], ['a' => 2]],
    'nums' => [3, 1.5, '2', true, null, 'a'],
    'strs' => ['a', 'b', 'c', 'd', 'e'],
];
foreach ($arrays as $name => $a) {
    v('a:' . $name, $a);
}
v('o:std', new stdClass());
v('o:S', new S('abc'));
v('o:Sa', new S('a'));
v('o:obj', (object) ['a' => 1, 'id' => 7]);
v('it:list', new ArrayIterator([1, 2, 3]));
v('it:assoc', new ArrayIterator(['a' => 1, 5 => 'five', '7' => 'seven']));
v('it:empty', new ArrayIterator([]));

function idx(string $name): int
{
    global $index;
    if (!isset($index[$name])) {
        throw new LogicException("unknown value $name");
    }
    return $index[$name];
}

$calls = [];
$cbCalls = 0;
$cbs = callbacks($cbCalls);

/** Runs one call; $args are value names. */
function call(string $fn, array $args, ?string $cb = null): void
{
    global $calls, $values, $cbs, $cbCalls;
    $ix = array_map('idx', $args);
    $vals = array_map(static fn ($i) => $values[$i], $ix);
    $f = $cb === null ? null : $cbs[$cb];
    $cbCalls = 0;
    [$r, $d] = capture(static function () use ($fn, $vals, $f, $cb) {
        switch ($fn) {
            case 'array_filter':
                return $cb === null ? array_filter(...$vals) : array_filter($vals[0], $f, ...array_slice($vals, 1));
            case 'array_map':
                return array_map($f, ...$vals);
            case 'array_reduce':
                return array_reduce($vals[0], $f, ...array_slice($vals, 1));
            default:
                return $fn(...$vals);
        }
    });
    $calls[] = [$fn, $ix, $cb, resn($r), diags($d), $cbCalls];
}

$arr = array_map(static fn ($n) => 'a:' . $n, array_keys($arrays));
$basic = ['a:empty', 'a:list', 'a:assoc', 'a:mixed', 'a:rev', 'a:numkeys', 'a:neg', 'a:falsy', 'a:nested',
    'a:holes', 'a:trimmed', 'a:maxkey'];
$notArrays = ['null', '1', 'sx', 'o:std', 'it:list'];

// array_merge
call('array_merge', []);
foreach ($basic as $a) {
    call('array_merge', [$a]);
    foreach ($basic as $b) {
        call('array_merge', [$a, $b]);
    }
    call('array_merge', [$a, 'a:assoc', 'a:list']);
}
foreach ($notArrays as $x) {
    call('array_merge', [$x]);
    call('array_merge', ['a:list', $x]);
}

// array_chunk
foreach (array_merge($basic, ['null']) as $a) {
    foreach (['1', '2', '3', '10', '0', '-1', 's2', 'sx', 'null', '1.5', 'true'] as $len) {
        call('array_chunk', [$a, $len]);
        foreach (['false', 'true', 'null', 'sx'] as $p) {
            call('array_chunk', [$a, $len, $p]);
        }
    }
}

// array_column
foreach (['a:rows', 'a:objrows', 'a:badrows', 'a:dupidx', 'a:maxidx', 'a:list', 'a:empty', 'sx'] as $a) {
    foreach (['sa', 'sb', 'sid', '0', '1', 'null', 's1', '1.5', 'true', 'a:empty', 'o:Sa', 'o:std', 'nan', 'big'] as $col) {
        call('array_column', [$a, $col]);
        foreach (['sid', 'null', 'sa', '0', 'smissing', 'a:empty', '1.5'] as $ind) {
            call('array_column', [$a, $col, $ind]);
        }
    }
}

// array_slice
foreach (array_merge($basic, ['a:strs', 'sx']) as $a) {
    foreach (['0', '1', '-1', '2', '-2', '10', '-10', 's1', 'sx', 'null', '1.5', 'max'] as $off) {
        call('array_slice', [$a, $off]);
        foreach (['null', '0', '1', '2', '-1', '-5', '10', 'sx', '1.5', 'max'] as $len) {
            call('array_slice', [$a, $off, $len]);
            call('array_slice', [$a, $off, $len, 'true']);
        }
    }
    call('array_slice', [$a, '1', 'null', 'null']);
    call('array_slice', [$a, '1', '2', 'sx']);
}

// array_keys / array_values / array_reverse / array_key_first / array_key_last / array_is_list
foreach (array_merge($basic, ['a:keys', 'a:nums'], $notArrays) as $a) {
    call('array_keys', [$a]);
    foreach (['null', '1', 's1', 'sa', 'true', 'false', 'a:empty', '0', '1.0'] as $f) {
        call('array_keys', [$a, $f]);
        call('array_keys', [$a, $f, 'true']);
    }
    call('array_values', [$a]);
    call('array_reverse', [$a]);
    call('array_reverse', [$a, 'true']);
    call('array_reverse', [$a, 'null']);
    call('array_key_first', [$a]);
    call('array_key_last', [$a]);
    call('array_is_list', [$a]);
}

// in_array / array_search
$needles = ['null', 'true', 'false', '0', '1', '2', '1.5', '1.0', 'nan', 's', 's0', 's1', 'sa', 's1.5', 's 1', 's01',
    'a:empty', 'a:list', 'o:S', 'o:std'];
foreach (['a:falsy', 'a:keys', 'a:nums', 'a:mixed', 'a:nested', 'a:empty', 'sx'] as $hay) {
    foreach ($needles as $n) {
        foreach (['in_array', 'array_search'] as $fn) {
            call($fn, [$n, $hay]);
            call($fn, [$n, $hay, 'true']);
        }
    }
    call('in_array', ['1', $hay, 'null']);
}

// array_key_exists
foreach (['a:numkeys', 'a:mixed', 'a:falsy', 'a:neg', 'a:maxkey', 'a:empty', 'sx'] as $a) {
    foreach (['null', 'true', 'false', '0', '1', '-5', '3', '5', '1.5', '1.0', '-0.5', 'nan', 'inf', 'big', 's', 's0',
        's1', 'sa', 'sk', 'sx', 's01', 's 1', 'max', 'a:empty', 'o:S', 'o:std'] as $k) {
        call('array_key_exists', [$k, $a]);
    }
}

// array_pad
foreach (array_merge($basic, ['sx']) as $a) {
    foreach (['0', '1', '3', '5', '-5', '-1', '8', 'sx', 'null', '1.5', 'pad+', 'pad-'] as $len) {
        call('array_pad', [$a, $len, 'null']);
        call('array_pad', [$a, $len, 'sa']);
    }
}

// array_fill_keys / array_flip
foreach (['a:keys', 'a:keyerr', 'a:objkeys', 'a:badobj', 'a:list', 'a:empty', 'a:mixed', 'sx'] as $a) {
    call('array_fill_keys', [$a, 'sa']);
    call('array_fill_keys', [$a, 'a:list']);
}
foreach (array_merge($basic, ['a:flip', 'a:keys', 'sx']) as $a) {
    call('array_flip', [$a]);
}

// array_filter
foreach (array_merge($basic, ['a:nums', 'sx']) as $a) {
    call('array_filter', [$a]);
    call('array_filter', [$a, 'null']);
    foreach ([null, 'truthy', 'odd', 'both', 'nargs', 'throw'] as $cb) {
        foreach ([[], ['0'], ['1'], ['2'], ['3'], ['sx'], ['null']] as $mode) {
            if ($cb === null && $mode === []) {
                continue;
            }
            if ($cb === null) {
                call('array_filter', array_merge([$a, 'null'], $mode));
            } else {
                call('array_filter', array_merge([$a], $mode), $cb);
            }
        }
    }
}

// array_map
foreach ([null, 'first', 'pack', 'throw'] as $cb) {
    foreach ($basic as $a) {
        call('array_map', [$a], $cb);
        call('array_map', [$a, 'a:list'], $cb);
        call('array_map', ['a:strs', $a, 'a:assoc'], $cb);
    }
    foreach ($notArrays as $x) {
        call('array_map', [$x], $cb);
        call('array_map', ['a:list', $x], $cb);
    }
}

// array_reduce
foreach (array_merge($basic, ['a:nums', 'sx']) as $a) {
    foreach (['concat', 'count', 'throw'] as $cb) {
        call('array_reduce', [$a], $cb);
        call('array_reduce', [$a, 'null'], $cb);
        call('array_reduce', [$a, 'sx'], $cb);
        call('array_reduce', [$a, '5'], $cb);
    }
}

// iterator_to_array / iterator_count
foreach (array_merge($basic, ['it:list', 'it:assoc', 'it:empty', 'null', '1', 'sx', 'o:std', 'o:S']) as $a) {
    call('iterator_to_array', [$a]);
    foreach (['true', 'false', 'null', 'sx'] as $p) {
        call('iterator_to_array', [$a, $p]);
    }
    call('iterator_count', [$a]);
}

// array_intersect_key / array_diff_key
foreach (['array_intersect_key', 'array_diff_key'] as $fn) {
    call($fn, []);
    foreach ($basic as $a) {
        call($fn, [$a]);
        foreach (['a:list', 'a:assoc', 'a:mixed', 'a:numkeys', 'a:empty'] as $b) {
            call($fn, [$a, $b]);
            call($fn, [$a, $b, 'a:rev']);
        }
    }
    call($fn, ['a:list', 'sx']);
    call($fn, ['sx', 'a:list']);
}

emit('array/functions.gen.php', [
    'values' => array_map(static fn ($v) => $v instanceof ArrayIterator
        ? ['t' => 'iter', 'array' => encn($v->getArrayCopy())]
        : encn($v), $values),
    'calls' => $calls,
    'messages' => messages(),
]);

<?php
// Generates fixture_data.json: what the PHP oracle knows about every evaluated --DATA-- context of the upstream
// fixtures, beyond the typed trees of tests/twig/oracle-corpus.json.
//
//   cd crates/twig-test-support/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/fixture_data.gen.php > fixture_data.json
//
// Every fixture runs in its own PHP process (like research/php-oracle/tools/export-corpus.php: inline class
// declarations, anonymous class names and date_default_timezone_set() must not leak between fixtures). Each
// DATA/EXPECT case is replayed like TwigOracle\run_fixture() (= IntegrationTestCase::doIntegrationTest()): the
// CONFIG is evaluated, the cumulative spaces are appended to the templates, the harness environment is built and
// every template is loaded; a load error ends the fixture. Then the DATA code is evaluated with $twig in scope and
// recorded, in this order (later steps see the state left by earlier ones, exactly as the Rust tests replay it):
//
//   "data":   the context as a rich typed tree: TwigOracle\typed() (lib.php) with the same fields in the same order,
//             plus
//               float  "bits"           IEEE-754 bit pattern (hex), exact even for NAN signs
//               array  [k, node, k_b64] third element when a string key is not valid UTF-8 (k is then lossy)
//               object "methods"        get_class_methods() from the global scope (PHP's order)
//                      "parents"        class_parents() order
//                      "interfaces_raw" class_implements() order (typed() sorts its "interfaces" copy)
//                      "props_hidden"   private/protected properties of userland classes (mangled names) from
//                                       get_mangled_object_vars(), after the typed() fields were computed
//                      "flags"          ArrayObject/ArrayIterator: getFlags()
//                      "iterator_class" ArrayObject: getIteratorClass()
//                      "storage"        SplObjectStorage: [[object, info], ...]
//                      "inner"          CallbackFilterIterator: the inner iterator
//                      "accepts"        CallbackFilterIterator: for each element of the inner iterator whether
//                                       the filter yields it (the callback itself cannot be inspected); the inner
//                                       iterator is rewound afterwards
//   "dumps":  var_dump(), var_export(), print_r() of every top-level context value, with diagnostics (skipped for
//             values containing a Twig\TemplateWrapper: they dump the whole Environment)
//   "probes": operations on every object of the context (DFS order, each object once) and their PHP results:
//             method calls, string/bool/array casts, count(), foreach, iterator_to_array(), iterator_count(),
//             property reads, isset(). Results are rich nodes that share the object identities of "data".
//
// Strings that are not valid UTF-8 come as a lossy "<field>" plus "<field>_b64" (lib.php put_string()).
// ArgumentCountError texts drop PHP's " in <file> on line <n>" after "passed" (reference/deviations.md).

declare(strict_types=0);

require '/oracle/vendor/autoload.php';
require '/oracle/lib.php';

use function TwigOracle\eval_with_twig;
use function TwigOracle\integration_environment;
use function TwigOracle\parse_fixture;
use function TwigOracle\put_string;

const JSON_FLAGS = JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION | JSON_THROW_ON_ERROR;
const ITERATE_LIMIT = 50;

// ---------------------------------------------------------------------------------------------------------------
// Rich typed trees
// ---------------------------------------------------------------------------------------------------------------

final class Rich
{
    /** @var array<int, true> */
    public static array $seen = [];
    /** Objects encoded so far, kept alive so that spl_object_id() values are never reused. */
    public static array $keep = [];
}

function str_node(string $s): array
{
    $o = ['t' => 'string'];
    put_string($o, 'v', $s);

    return $o;
}

function key_item($k, array $node): array
{
    if (\is_string($k) && !mb_check_encoding($k, 'UTF-8')) {
        return [mb_convert_encoding($k, 'UTF-8', 'UTF-8'), $node, base64_encode($k)];
    }

    return [$k, $node];
}

function describe_error(\Throwable $e): array
{
    $msg = preg_replace('/ passed in .*? on line \d+ and /', ' passed and ', $e->getMessage());
    $out = ['class' => $e::class];
    put_string($out, 'message', $msg);

    return $out;
}

/** TwigOracle\typed() plus the extra fields documented at the top of this file. */
function rich($v, int $depth = 0): array
{
    if ($depth > 64) {
        return ['t' => 'truncated'];
    }
    switch (true) {
        case null === $v: return ['t' => 'null'];
        case \is_bool($v): return ['t' => 'bool', 'v' => $v];
        case \is_int($v): return ['t' => 'int', 'v' => $v];
        case \is_float($v):
            $repr = is_nan($v) ? 'NAN' : (is_infinite($v) ? ($v > 0 ? 'INF' : '-INF') : var_export($v, true));

            return ['t' => 'float', 'v' => $repr, 'bits' => bin2hex(strrev(pack('d', $v)))];
        case \is_string($v):
            return str_node($v);
        case \is_array($v):
            $items = [];
            foreach ($v as $k => $item) {
                $items[] = key_item($k, rich($item, $depth + 1));
            }

            return ['t' => 'array', 'v' => $items];
        case \is_resource($v) || 'resource (closed)' === get_debug_type($v):
            return ['t' => 'resource', 'type' => get_resource_type($v)];
        case \is_object($v):
            return rich_object($v, $depth);
    }

    return ['t' => get_debug_type($v)];
}

function rich_object(object $v, int $depth): array
{
    $id = spl_object_id($v);
    if (isset(Rich::$seen[$id])) {
        return ['t' => 'ref', 'class' => $v::class, 'id' => $id];
    }
    Rich::$seen[$id] = true;
    Rich::$keep[] = $v;
    $o = ['t' => 'object', 'class' => $v::class, 'id' => $id];
    if ($v instanceof \UnitEnum) {
        $o['t'] = 'enum';
        $o['case'] = $v->name;
        if ($v instanceof \BackedEnum) {
            $o['value'] = rich($v->value, $depth + 1);
        }

        return $o;
    }
    if ($v instanceof \Closure) {
        $o['t'] = 'closure';

        return $o;
    }
    $ref = new \ReflectionObject($v);
    $userland = !$ref->isInternal();
    if ($v instanceof \Twig\Markup) {
        $o['content'] = rich((string) $v, $depth + 1);
        $o['charset'] = $v->getCharset();
    } elseif ($v instanceof \DateTimeInterface) {
        $o['datetime'] = $v->format('Y-m-d\TH:i:s.uP');
        $o['timezone'] = $v->getTimezone()->getName();
        $o['timestamp'] = $v->getTimestamp();
    } elseif ($v instanceof \DateTimeZone) {
        $o['name'] = $v->getName();
    } elseif ($v instanceof \DateInterval) {
        $o['spec'] = ['y' => $v->y, 'm' => $v->m, 'd' => $v->d, 'h' => $v->h, 'i' => $v->i, 's' => $v->s, 'f' => $v->f, 'invert' => $v->invert, 'days' => $v->days];
    } elseif ($v instanceof \Twig\TemplateWrapper) {
        $o['template'] = $v->getTemplateName();
        $userland = false; // its private properties are the whole Environment
    } elseif ($v instanceof \ArrayObject || $v instanceof \ArrayIterator) {
        $o['entries'] = rich($v->getArrayCopy(), $depth + 1);
    } elseif ($v instanceof \SimpleXMLElement) {
        $o['xml'] = $v->asXML();
    } else {
        $o['interfaces'] = array_values(class_implements($v));
        sort($o['interfaces']);
        if (method_exists($v, '__toString') && !$v instanceof \Throwable) {
            try {
                $o['toString'] = rich((string) $v, $depth + 1);
            } catch (\Throwable $e) {
                $o['toString_error'] = TwigOracle\describe_throwable($e);
            }
        }
        $props = [];
        foreach (get_object_vars($v) as $k => $pv) {
            $props[] = [$k, rich($pv, $depth + 1)];
        }
        $o['props'] = $props;
    }

    // Extras (not in lib.php's typed()).
    $o['methods'] = get_class_methods($v);
    $o['parents'] = array_values(class_parents($v));
    $o['interfaces_raw'] = array_values(class_implements($v));
    if ($userland) {
        $hidden = [];
        foreach (get_mangled_object_vars($v) as $k => $pv) {
            if (\is_string($k) && '' !== $k && "\0" === $k[0]) {
                $hidden[] = key_item($k, rich($pv, $depth + 1));
            }
        }
        $o['props_hidden'] = $hidden;
    }
    if ($v instanceof \ArrayObject || $v instanceof \ArrayIterator) {
        $o['flags'] = $v->getFlags();
        if ($v instanceof \ArrayObject) {
            $o['iterator_class'] = $v->getIteratorClass();
        }
    }
    if ($v instanceof \SplObjectStorage) {
        $storage = [];
        foreach ($v as $i => $obj) {
            $storage[] = [rich($obj, $depth + 1), rich($v[$obj], $depth + 1)];
        }
        $v->rewind();
        $o['storage'] = $storage;
    }
    if ($v instanceof \CallbackFilterIterator) {
        $inner = $v->getInnerIterator();
        $o['inner'] = rich($inner, $depth + 1);
        $accepted = [];
        foreach ($v as $k => $x) {
            $accepted[] = $k;
        }
        $accepts = [];
        foreach ($inner as $k => $x) {
            $accepts[] = \in_array($k, $accepted, true);
        }
        $inner->rewind();
        $o['accepts'] = $accepts;
    }

    return $o;
}

// ---------------------------------------------------------------------------------------------------------------
// Diagnostics capture
// ---------------------------------------------------------------------------------------------------------------

const LEVELS = [\E_WARNING => 'W', \E_NOTICE => 'N', \E_DEPRECATED => 'D', \E_USER_DEPRECATED => 'UD', \E_USER_WARNING => 'UW', \E_USER_NOTICE => 'UN'];

/** Runs $f with every diagnostic recorded and swallowed (execution continues, like plain PHP). */
function capture(callable $f): array
{
    $diags = [];
    set_error_handler(static function (int $no, string $msg) use (&$diags): bool {
        $d = [LEVELS[$no] ?? (string) $no];
        $o = [];
        put_string($o, 'm', $msg);
        $d[] = $o['m'];
        if (isset($o['m_b64'])) {
            $d[] = $o['m_b64'];
        }
        $diags[] = $d;

        return true;
    });
    try {
        $r = ['ok' => $f()];
    } catch (\Throwable $e) {
        $r = ['err' => $e];
    } finally {
        restore_error_handler();
    }
    $r['diag'] = $diags;

    return $r;
}

/** Whether a value contains a TemplateWrapper (its dump is the whole Environment: megabytes, not reproducible). */
function contains_template_wrapper($v, array &$ids = []): bool
{
    if (\is_array($v)) {
        foreach ($v as $x) {
            if (contains_template_wrapper($x, $ids)) {
                return true;
            }
        }

        return false;
    }
    if (!\is_object($v) || isset($ids[spl_object_id($v)])) {
        return false;
    }
    $ids[spl_object_id($v)] = true;
    if ($v instanceof \Twig\TemplateWrapper) {
        return true;
    }
    if ($v instanceof \ArrayObject || $v instanceof \ArrayIterator) {
        return contains_template_wrapper($v->getArrayCopy(), $ids);
    }

    return !(new \ReflectionObject($v))->isInternal() && contains_template_wrapper((array) $v, $ids);
}

function dumps_of($value): array
{
    if (contains_template_wrapper($value)) {
        return ['skipped' => 'Twig\TemplateWrapper'];
    }
    $out = [];
    foreach (['var_dump' => static function ($v) { ob_start(); var_dump($v); return ob_get_clean(); },
              'var_export' => static fn ($v) => var_export($v, true),
              'print_r' => static fn ($v) => print_r($v, true)] as $name => $fn) {
        $r = capture(static fn () => $fn($value));
        if (isset($r['err'])) {
            $out[$name.'_error'] = describe_error($r['err']);
        } else {
            put_string($out, $name, $r['ok']);
        }
        if ($r['diag']) {
            $out[$name.'_diag'] = $r['diag'];
        }
    }

    return $out;
}

// ---------------------------------------------------------------------------------------------------------------
// Probes
// ---------------------------------------------------------------------------------------------------------------

function call_op(string $method, array $args = [], array $named = []): array
{
    return ['op' => 'call', 'method' => $method, 'args' => $args, 'named' => $named];
}

function probes_for(object $o): array
{
    $class = $o::class;
    $display = explode("\0", $class)[0];
    $base = [['op' => 'methods'], ['op' => 'bool'], ['op' => 'array_cast']];
    $ops = match ($display) {
        'Twig\Tests\TwigTestFoo' => [
            call_op('bar'), call_op('bar', ['x']), call_op('bar', ['x', 'y']), call_op('bar', [], ['param2' => 'y']),
            call_op('bar', [0, '0']), call_op('bar', ['', 'a']), call_op('bar', [[]]), call_op('bar', [[1]]),
            call_op('bar', [1.5, true]), call_op('BAR', ['z']), call_op('bar', ['a', 'b', 'c']),
            call_op('bar', [], ['param3' => 1]), call_op('bar', ['a'], ['param1' => 'b']), call_op('bar', [null, 'n']),
            call_op('getFoo'), call_op('getfoo'), call_op('getEmpty'), call_op('getNull'), call_op('getSelf'),
            call_op('is'), call_op('in'), call_op('not'),
            call_op('strToLower', ["ABC D\xC3\xA9f"]), call_op('strToLower', [null]), call_op('strToLower', [[]]),
            call_op('strToLower', [12]), call_op('strToLower'), call_op('nope'),
            ['op' => 'prop', 'name' => 'position'], ['op' => 'prop', 'name' => 'array'], ['op' => 'prop', 'name' => 'foo'],
            ['op' => 'prop', 'name' => 'missing'], ['op' => 'isset_prop', 'name' => 'position'],
            ['op' => 'isset_prop', 'name' => 'foo'], ['op' => 'string'], ['op' => 'count'],
            call_op('key'), call_op('current'), call_op('valid'), call_op('next'), call_op('current'), call_op('valid'),
            ['op' => 'iterate'], call_op('current'), call_op('valid'), call_op('next'), call_op('rewind'), call_op('current'), ['op' => 'to_array', 'keys' => true], ['op' => 'to_array', 'keys' => false],
            ['op' => 'iterator_count'], ['op' => 'iterate', 'limit' => 1], call_op('key'),
        ],
        'Twig\Tests\MagicCallStub' => [
            call_op('foo'), call_op('__call', ['x', []]), call_op('count'), ['op' => 'string'], ['op' => 'count'],
            ['op' => 'prop', 'name' => 'x'], ['op' => 'isset_prop', 'name' => 'x'],
        ],
        'Twig\Tests\ToStringStub' => [
            ['op' => 'string'], call_op('__toString'), call_op('__tostring'), ['op' => 'prop', 'name' => 'string'],
            ['op' => 'isset_prop', 'name' => 'string'], ['op' => 'count'], call_op('nope'), call_op('__construct', ['re']),
            ['op' => 'string'],
        ],
        'Twig\Tests\CountableStub' => [
            ['op' => 'count'], call_op('count'), ['op' => 'string'], call_op('__toString'),
            ['op' => 'prop', 'name' => 'count'], ['op' => 'isset_prop', 'name' => 'count'],
        ],
        'Twig\Tests\IteratorAggregateStub' => [
            ['op' => 'iterate'], ['op' => 'to_array', 'keys' => true], ['op' => 'iterator_count'], call_op('getIterator'),
            ['op' => 'count'], ['op' => 'string'], ['op' => 'prop', 'name' => 'data'],
        ],
        'Twig\Tests\SimpleIteratorForTesting' => [
            call_op('key'), call_op('current'), call_op('valid'), ['op' => 'string'], call_op('key'), call_op('valid'),
            call_op('rewind'), ['op' => 'iterate'], ['op' => 'iterator_count'], call_op('next'), call_op('key'),
            call_op('rewind'), call_op('next'), call_op('key'), ['op' => 'count'], ['op' => 'to_array', 'keys' => true],
        ],
        'TwigTestStringy' => [
            ['op' => 'string'], call_op('__toString'), ['op' => 'prop', 'name' => 'v'], ['op' => 'isset_prop', 'name' => 'v'],
        ],
        'TwigTestHolder' => [
            call_op('getStringable'), ['op' => 'prop', 'name' => 'name'], ['op' => 'prop', 'name' => 'stringable'],
            ['op' => 'isset_prop', 'name' => 'stringable'], ['op' => 'string'], call_op('getstringable', [1]),
        ],
        'TestClassForMagicCallAttributes' => [
            call_op('getBar'), call_op('foo'), call_op('bar'), call_op('getbar'), call_op('other', [1, 2]),
            call_op('foo', [], ['x' => 1]), ['op' => 'string'], ['op' => 'prop', 'name' => 'foo'],
        ],
        'MyObj' => [['op' => 'string'], call_op('__toString'), ['op' => 'count']],
        'UserForAutoEscapeTest' => [call_op('getName'), ['op' => 'string'], call_op('getname')],
        'ItemsIterator', 'ItemsIteratorCountable' => [
            call_op('current'), call_op('key'), call_op('valid'), call_op('next'), call_op('current'), call_op('key'),
            call_op('next'), call_op('valid'), call_op('current'), call_op('key'), call_op('next'), call_op('valid'),
            ['op' => 'iterate'], ['op' => 'iterate', 'limit' => 1], call_op('current'), call_op('rewind'),
            call_op('current'), ['op' => 'count'], ['op' => 'to_array', 'keys' => true], ['op' => 'iterator_count'],
            ['op' => 'prop', 'name' => 'values'],
        ],
        'class@anonymous' => [call_op('getName'), call_op('getname'), ['op' => 'string']],
        'IteratorAggregate@anonymous' => [
            ['op' => 'prop', 'name' => 'yielded'], ['op' => 'iterate', 'limit' => 2], ['op' => 'prop', 'name' => 'yielded'],
            ['op' => 'iterate'], ['op' => 'prop', 'name' => 'yielded'], ['op' => 'to_array', 'keys' => true],
            ['op' => 'iterator_count'], ['op' => 'prop', 'name' => 'yielded'], ['op' => 'count'],
        ],
        'SplObjectStorage' => [
            ['op' => 'count'], call_op('count'), ['op' => 'iterate'], call_op('current'), call_op('key'), call_op('valid'),
            call_op('rewind'), call_op('valid'), call_op('key'), call_op('getInfo'), call_op('current'),
            ['op' => 'dim', 'offset' => storage_first($o)], ['op' => 'isset_dim', 'offset' => storage_first($o)],
            call_op('contains', [storage_first($o)]), call_op('offsetExists', [storage_first($o)]),
            call_op('offsetGet', [storage_first($o)]), ['op' => 'dim', 'offset' => new \stdClass()],
            ['op' => 'isset_dim', 'offset' => new \stdClass()], ['op' => 'dim', 'offset' => 'str'],
            call_op('offsetGet', ['str']), call_op('contains', [new \stdClass()]), ['op' => 'string'],
        ],
        'SplFileInfo' => [
            ['op' => 'string'], call_op('__toString'), call_op('getPathname'), call_op('getFilename'), call_op('getPath'),
            call_op('getBasename'), call_op('getBasename', ['cle']), call_op('getExtension'),
        ],
        'CallbackFilterIterator' => [
            call_op('valid'), call_op('current'), call_op('key'), ['op' => 'iterate'], ['op' => 'iterator_count'],
            ['op' => 'to_array', 'keys' => true], call_op('valid'), call_op('current'), call_op('rewind'), call_op('valid'),
            call_op('current'), call_op('key'), call_op('accept'), call_op('next'), call_op('valid'),
            call_op('getInnerIterator'), ['op' => 'count'], ['op' => 'string'],
        ],
        'SimpleXMLElement' => [
            ['op' => 'count'], call_op('count'), ['op' => 'string'], call_op('getName'), call_op('asXML'),
            ['op' => 'iterate'], ['op' => 'iterate'], ['op' => 'iterator_count'], ['op' => 'to_array', 'keys' => false],
            ['op' => 'to_array', 'keys' => true],
        ],
        'ArrayObject', 'ArrayIterator' => [['op' => 'iterate'], ['op' => 'count'], ['op' => 'string']],
        'stdClass' => [['op' => 'iterate'], ['op' => 'count'], ['op' => 'string']],
        default => null,
    };
    if (null === $ops) {
        return [];
    }

    return array_merge($base, $ops);
}

function storage_first(\SplObjectStorage $s): ?object
{
    $s->rewind();
    $first = $s->valid() ? $s->current() : null;
    $s->rewind();

    return $first;
}

function run_op(object $o, array $op)
{
    switch ($op['op']) {
        case 'methods': return get_class_methods($o);
        case 'bool': return (bool) $o;
        case 'array_cast': return (array) $o;
        case 'string': return (string) $o;
        case 'count': return \count($o);
        case 'call': return $o->{$op['method']}(...$op['args'], ...$op['named']);
        case 'prop': return $o->{$op['name']};
        case 'isset_prop': return isset($o->{$op['name']});
        case 'dim': return $o[$op['offset']];
        case 'isset_dim': return isset($o[$op['offset']]);
        case 'iterator_count': return iterator_count($o);
        case 'to_array': return iterator_to_array($o, $op['keys']);
        case 'iterate':
            $limit = $op['limit'] ?? ITERATE_LIMIT;
            $out = [];
            foreach ($o as $k => $x) {
                $out[] = [$k, $x];
                if (\count($out) >= $limit) {
                    break;
                }
            }

            return $out;
    }
    throw new \LogicException('unknown op '.$op['op']);
}

/** Encodes an op for JSON (arguments as rich nodes) and runs it. */
function probe(object $o, array $op): array
{
    $enc = $op;
    if ('call' === $op['op']) {
        $enc['args'] = array_map(static fn ($a) => rich($a), $op['args']);
        $named = [];
        foreach ($op['named'] as $k => $a) {
            $named[] = [$k, rich($a)];
        }
        $enc['named'] = $named;
    }
    if (\array_key_exists('offset', $op)) {
        $enc['offset'] = rich($op['offset']);
    }
    $r = capture(static fn () => run_op($o, $op));
    if (isset($r['err'])) {
        $enc['err'] = describe_error($r['err']);
    } else {
        $enc['ok'] = rich($r['ok']);
    }
    if ($r['diag']) {
        $enc['diag'] = $r['diag'];
    }

    return $enc;
}

/** Objects of a value in DFS order (each once): the probe targets. */
function collect_objects($v, array &$out, array &$ids): void
{
    if (\is_array($v)) {
        foreach ($v as $x) {
            collect_objects($x, $out, $ids);
        }

        return;
    }
    if (!\is_object($v) || $v instanceof \UnitEnum || $v instanceof \Closure) {
        return;
    }
    $id = spl_object_id($v);
    if (isset($ids[$id])) {
        return;
    }
    $ids[$id] = true;
    $out[] = $v;
    if ($v instanceof \ArrayObject || $v instanceof \ArrayIterator) {
        collect_objects($v->getArrayCopy(), $out, $ids);
    } elseif ($v instanceof \SplObjectStorage) {
        foreach ($v as $obj) {
            collect_objects($obj, $out, $ids);
        }
        $v->rewind();
    } elseif ($v instanceof \CallbackFilterIterator) {
        collect_objects($v->getInnerIterator(), $out, $ids);
    } elseif (!$v instanceof \Twig\TemplateWrapper && !$v instanceof \SimpleXMLElement && !(new \ReflectionObject($v))->isInternal()) {
        foreach ((array) $v as $x) {
            collect_objects($x, $out, $ids);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// One fixture (child process)
// ---------------------------------------------------------------------------------------------------------------

function run_one(string $rel): array
{
    $p = parse_fixture(file_get_contents(TwigOracle\FIXTURES_DIR.'/'.$rel));
    if (null === $p) {
        throw new \InvalidArgumentException("Test \"$rel\" is not valid.");
    }
    if ($p['condition']) {
        $ret = '';
        eval('$ret = '.$p['condition'].';');
        if (!$ret) {
            return [];
        }
    }
    $cases = [];
    $templateSources = $p['templates'];
    foreach ($p['outputs'] as $i => $match) {
        $config = $match[2] ? eval_with_twig($match[2], null) : [];
        foreach ($templateSources as $name => $template) {
            $templateSources[$name] = $template.str_repeat(' ', $i);
        }
        $twig = integration_environment($templateSources, $config);
        try {
            (new TwigOracle\Diagnostics())->run(static function () use ($twig, $templateSources) {
                foreach (array_keys($templateSources) as $templateName) {
                    $twig->load($templateName);
                }
            }, 'throw');
        } catch (\Exception $e) {
            break; // the harness stops the fixture at a load error
        }
        $case = ['file' => $rel, 'index' => $i];
        try {
            $context = (new TwigOracle\Diagnostics())->run(static fn () => eval_with_twig($match[1], $twig), 'throw');
        } catch (\Throwable $e) {
            $case['data_error'] = describe_error($e);
            $cases[] = $case;
            continue;
        }
        $case['data'] = rich($context);
        $dumps = [];
        if (\is_array($context)) {
            foreach ($context as $k => $value) {
                $dumps[] = [$k, dumps_of($value)];
            }
        }
        $case['dumps'] = $dumps;
        $targets = [];
        $ids = [];
        collect_objects($context, $targets, $ids);
        $probes = [];
        foreach ($targets as $target) {
            $ops = probes_for($target);
            if (!$ops) {
                continue;
            }
            $results = [];
            foreach ($ops as $op) {
                $results[] = probe($target, $op);
            }
            $probes[] = ['id' => spl_object_id($target), 'class' => $target::class, 'ops' => $results];
        }
        $case['probes'] = $probes;
        $cases[] = $case;
    }

    return $cases;
}

// ---------------------------------------------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------------------------------------------

if (($argv[1] ?? null) === '--single') {
    echo json_encode(run_one($argv[2]), JSON_FLAGS);
    exit(0);
}

$dir = realpath(TwigOracle\FIXTURES_DIR);
$files = [];
foreach (new RecursiveIteratorIterator(new RecursiveDirectoryIterator($dir), RecursiveIteratorIterator::LEAVES_ONLY) as $file) {
    if (preg_match('/\.test$/', (string) $file)) {
        $files[] = str_replace($dir.'/', '', (string) $file);
    }
}
sort($files, SORT_STRING);

echo '{"generator":"fixture_data.gen.php","php":"', PHP_VERSION, '","twig":"', Twig\Environment::VERSION, '","cases":[';
$first = true;
foreach ($files as $rel) {
    $proc = proc_open([PHP_BINARY, __FILE__, '--single', $rel], [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
    $json = stream_get_contents($pipes[1]);
    $err = stream_get_contents($pipes[2]);
    fclose($pipes[1]);
    fclose($pipes[2]);
    $status = proc_close($proc);
    $cases = json_decode($json, true);
    if (0 !== $status || !\is_array($cases)) {
        fwrite(STDERR, "$rel: child failed ($status): $err\n");
        exit(1);
    }
    foreach ($cases as $case) {
        echo $first ? "\n" : ",\n", json_encode($case, JSON_FLAGS);
        $first = false;
    }
}
echo "\n]}\n";

<?php

/*
 * Shared helpers for the twig-rs PHP oracle (runner.php, tools/*.php).
 * Runs inside the oracle image: /oracle/vendor/autoload.php provides Twig 3.31.1 (source install, with tests/).
 */

declare(strict_types=0);

namespace TwigOracle;

use Twig\Environment;
use Twig\Error\Error as TwigError;
use Twig\Loader\ArrayLoader;

const ERROR_TYPES = [
    \E_ERROR => 'E_ERROR', \E_WARNING => 'E_WARNING', \E_PARSE => 'E_PARSE', \E_NOTICE => 'E_NOTICE',
    \E_CORE_ERROR => 'E_CORE_ERROR', \E_CORE_WARNING => 'E_CORE_WARNING', \E_COMPILE_ERROR => 'E_COMPILE_ERROR',
    \E_COMPILE_WARNING => 'E_COMPILE_WARNING', \E_USER_ERROR => 'E_USER_ERROR', \E_USER_WARNING => 'E_USER_WARNING',
    \E_USER_NOTICE => 'E_USER_NOTICE', \E_RECOVERABLE_ERROR => 'E_RECOVERABLE_ERROR', \E_DEPRECATED => 'E_DEPRECATED',
    \E_USER_DEPRECATED => 'E_USER_DEPRECATED',
];

/**
 * Collects PHP diagnostics (warnings, notices, deprecations) raised while a callback runs.
 * mode "collect": every diagnostic is recorded and swallowed (execution continues like plain PHP CLI with
 *                 display_errors=0).
 * mode "throw":   E_USER_DEPRECATED / E_DEPRECATED are recorded; everything else is converted to an
 *                 \ErrorException (this is what PHPUnit 9 does with convertWarningsToExceptions etc.).
 */
final class Diagnostics
{
    public array $deprecations = [];   // E_USER_DEPRECATED messages, in order (what Twig's harness compares)
    public array $diagnostics = [];    // every diagnostic: {type, message}

    public function run(callable $fn, string $mode = 'collect')
    {
        set_error_handler(function (int $type, string $msg, string $file = '', int $line = 0) use ($mode): bool {
            if (!(error_reporting() & $type) && \E_USER_DEPRECATED !== $type) {
                // silenced with @ (Twig's trigger_deprecation() uses @trigger_error, still record it)
                $this->diagnostics[] = ['type' => ERROR_TYPES[$type] ?? (string) $type, 'message' => $msg, 'silenced' => true];

                return true;
            }
            $this->diagnostics[] = ['type' => ERROR_TYPES[$type] ?? (string) $type, 'message' => $msg];
            if (\E_USER_DEPRECATED === $type) {
                $this->deprecations[] = $msg;

                return true;
            }
            if ('throw' === $mode && \E_DEPRECATED !== $type) {
                throw new \ErrorException($msg, 0, $type, $file, $line);
            }

            return true;
        });
        try {
            return $fn();
        } finally {
            restore_error_handler();
        }
    }
}

/** Puts a possibly-binary string into $out[$key]; adds "$key_b64" when it is not valid UTF-8. */
function put_string(array &$out, string $key, ?string $s): void
{
    if (null === $s) {
        $out[$key] = null;

        return;
    }
    if (mb_check_encoding($s, 'UTF-8')) {
        $out[$key] = $s;
    } else {
        $out[$key] = mb_convert_encoding($s, 'UTF-8', 'UTF-8'); // invalid bytes -> '?'
        $out[$key.'_b64'] = base64_encode($s);
    }
}

/** Describes any Throwable (Twig errors get template name/line/column and the raw message). */
function describe_throwable(\Throwable $e, int $depth = 0): array
{
    $out = ['class' => $e::class];
    put_string($out, 'message', $e->getMessage());
    // the exact string Twig's IntegrationTestCase compares against --EXCEPTION--
    put_string($out, 'formatted', \sprintf('%s: %s', $e::class, $e->getMessage()));
    if ($e instanceof TwigError) {
        put_string($out, 'rawMessage', $e->getRawMessage());
        $src = $e->getSourceContext();
        put_string($out, 'templateName', $src?->getName());
        $out['templateLine'] = $e->getTemplateLine();
        $out['templateColumn'] = $e->getTemplateColumn();
    } else {
        $out['code'] = $e->getCode();
    }
    if ($e->getPrevious() && $depth < 5) {
        $out['previous'] = describe_throwable($e->getPrevious(), $depth + 1);
    }

    return $out;
}

/**
 * Typed, lossless-ish JSON view of a PHP value, meant to be turned into twig-rs test data.
 *   null/bool/int: {"t":"null"} {"t":"bool","v":true} {"t":"int","v":1}
 *   float:  {"t":"float","v":"0.1"}            (v = var_export() repr, i.e. shortest round-trip; or INF/-INF/NAN)
 *   string: {"t":"string","v":"..."}            (+ "v_b64" when not valid UTF-8)
 *   array:  {"t":"array","v":[[key,value],...]} (key: JSON int for int keys, JSON string for string keys; order kept)
 *   object: {"t":"object","class":"...","id":N, ...class specific fields...}
 *   resource: {"t":"resource","type":"stream"}
 */
function typed($v, array &$seen = [], int $depth = 0): array
{
    if ($depth > 64) {
        return ['t' => 'truncated'];
    }
    switch (true) {
        case null === $v: return ['t' => 'null'];
        case \is_bool($v): return ['t' => 'bool', 'v' => $v];
        case \is_int($v): return ['t' => 'int', 'v' => $v];
        case \is_float($v):
            if (is_nan($v)) {
                return ['t' => 'float', 'v' => 'NAN'];
            }
            if (is_infinite($v)) {
                return ['t' => 'float', 'v' => $v > 0 ? 'INF' : '-INF'];
            }

            return ['t' => 'float', 'v' => var_export($v, true)];
        case \is_string($v):
            $o = ['t' => 'string'];
            put_string($o, 'v', $v);

            return $o;
        case \is_array($v):
            $items = [];
            foreach ($v as $k => $item) {
                $items[] = [$k, typed($item, $seen, $depth + 1)];
            }

            return ['t' => 'array', 'v' => $items];
        case \is_resource($v) || 'resource (closed)' === get_debug_type($v):
            return ['t' => 'resource', 'type' => get_resource_type($v)];
        case \is_object($v):
            $id = spl_object_id($v);
            if (isset($seen[$id])) {
                return ['t' => 'ref', 'class' => $v::class, 'id' => $id];
            }
            $seen[$id] = true;
            $o = ['t' => 'object', 'class' => $v::class, 'id' => $id];
            if ($v instanceof \UnitEnum) {
                $o['t'] = 'enum';
                $o['case'] = $v->name;
                if ($v instanceof \BackedEnum) {
                    $o['value'] = typed($v->value, $seen, $depth + 1);
                }

                return $o;
            }
            if ($v instanceof \Closure) {
                $o['t'] = 'closure';

                return $o;
            }
            if ($v instanceof \Twig\Markup) {
                $o['content'] = typed((string) $v, $seen, $depth + 1);
                $o['charset'] = $v->getCharset();

                return $o;
            }
            if ($v instanceof \DateTimeInterface) {
                $o['datetime'] = $v->format('Y-m-d\TH:i:s.uP');
                $o['timezone'] = $v->getTimezone()->getName();
                $o['timestamp'] = $v->getTimestamp();

                return $o;
            }
            if ($v instanceof \DateTimeZone) {
                $o['name'] = $v->getName();

                return $o;
            }
            if ($v instanceof \DateInterval) {
                $o['spec'] = ['y' => $v->y, 'm' => $v->m, 'd' => $v->d, 'h' => $v->h, 'i' => $v->i, 's' => $v->s, 'f' => $v->f, 'invert' => $v->invert, 'days' => $v->days];

                return $o;
            }
            if ($v instanceof \Twig\TemplateWrapper) {
                $o['template'] = $v->getTemplateName();

                return $o;
            }
            if ($v instanceof \ArrayObject || $v instanceof \ArrayIterator) {
                $o['entries'] = typed($v->getArrayCopy(), $seen, $depth + 1);

                return $o;
            }
            if ($v instanceof \SimpleXMLElement) {
                $o['xml'] = $v->asXML();

                return $o;
            }
            $o['interfaces'] = array_values(class_implements($v));
            sort($o['interfaces']);
            if (method_exists($v, '__toString') && !$v instanceof \Throwable) {
                try {
                    $o['toString'] = typed((string) $v, $seen, $depth + 1);
                } catch (\Throwable $e) {
                    $o['toString_error'] = describe_throwable($e);
                }
            }
            $props = [];
            foreach (get_object_vars($v) as $k => $pv) {
                $props[] = [$k, typed($pv, $seen, $depth + 1)];
            }
            $o['props'] = $props;

            return $o;
    }

    return ['t' => get_debug_type($v)];
}

/**
 * typed() plus what twig-test-support needs to rebuild objects exactly (the "rich" trees of
 * crates/twig-test-support/tests/vectors/fixture_data.gen.php, same fields in the same order):
 *   float  "bits"           IEEE-754 bit pattern (hex), exact even for NAN signs
 *   array  [k, node, k_b64] third element when a string key is not valid UTF-8 (k is then lossy)
 *   object "methods"        get_class_methods() from the global scope (PHP's order)
 *          "parents"        class_parents() order
 *          "interfaces_raw" class_implements() order (typed() sorts its "interfaces" copy)
 *          "props_hidden"   private/protected properties of userland classes (mangled names), from
 *                           get_mangled_object_vars()
 *          "flags"          ArrayObject/ArrayIterator: getFlags()
 *          "iterator_class" ArrayObject: getIteratorClass()
 *          "storage"        SplObjectStorage: [[object, info], ...]
 *          "inner"          CallbackFilterIterator: the inner iterator
 *          "accepts"        CallbackFilterIterator: for each element of the inner iterator whether the
 *                           filter yields it (the inner iterator is rewound afterwards)
 *          "props"          also for DateTimeInterface, DateTimeZone and DateInterval (not in
 *                           fixture_data.gen.php): get_object_vars(), so the properties their handlers write
 *                           into the table plus the dynamic properties the data code added, in table order
 *                           (twig-fuzz recreates the dynamic ones in their places, and createFromDateString()
 *                           intervals from their date_string)
 *          "wall"           DateInterval: true when add() and sub() use wall-clock time for its time part
 *                           (interval_is_wall(); twig-fuzz rebuilds the interval with that mode)
 * Like typed(), it calls __toString() and iterates CallbackFilterIterators and SplObjectStorages.
 */
final class RichEncoder
{
    /** @var array<int, true> */
    private array $seen = [];
    /** Objects encoded so far, kept alive so that spl_object_id() values are never reused. */
    private array $keep = [];

    public function encode($v, int $depth = 0): array
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
                $o = ['t' => 'string'];
                put_string($o, 'v', $v);

                return $o;
            case \is_array($v):
                $items = [];
                foreach ($v as $k => $item) {
                    $items[] = self::keyItem($k, $this->encode($item, $depth + 1));
                }

                return ['t' => 'array', 'v' => $items];
            case \is_resource($v) || 'resource (closed)' === get_debug_type($v):
                return ['t' => 'resource', 'type' => get_resource_type($v)];
            case \is_object($v):
                return $this->object($v, $depth);
        }

        return ['t' => get_debug_type($v)];
    }

    /** get_object_vars() from outside the object, as [key, node] items in table order. */
    private function props(object $v, int $depth): array
    {
        $props = [];
        foreach (get_object_vars($v) as $k => $pv) {
            $props[] = self::keyItem($k, $this->encode($pv, $depth + 1));
        }

        return $props;
    }

    private static function keyItem($k, array $node): array
    {
        if (\is_string($k) && !mb_check_encoding($k, 'UTF-8')) {
            return [mb_convert_encoding($k, 'UTF-8', 'UTF-8'), $node, base64_encode($k)];
        }

        return [$k, $node];
    }

    private function object(object $v, int $depth): array
    {
        $id = spl_object_id($v);
        if (isset($this->seen[$id])) {
            return ['t' => 'ref', 'class' => $v::class, 'id' => $id];
        }
        $this->seen[$id] = true;
        $this->keep[] = $v;
        $o = ['t' => 'object', 'class' => $v::class, 'id' => $id];
        if ($v instanceof \UnitEnum) {
            $o['t'] = 'enum';
            $o['case'] = $v->name;
            if ($v instanceof \BackedEnum) {
                $o['value'] = $this->encode($v->value, $depth + 1);
            }

            return $o;
        }
        if ($v instanceof \Closure) {
            $o['t'] = 'closure';

            return $o;
        }
        $userland = !(new \ReflectionObject($v))->isInternal();
        if ($v instanceof \Twig\Markup) {
            $o['content'] = $this->encode((string) $v, $depth + 1);
            $o['charset'] = $v->getCharset();
        } elseif ($v instanceof \DateTimeInterface) {
            $o['datetime'] = $v->format('Y-m-d\TH:i:s.uP');
            $o['timezone'] = $v->getTimezone()->getName();
            $o['timestamp'] = $v->getTimestamp();
            $o['props'] = $this->props($v, $depth);
        } elseif ($v instanceof \DateTimeZone) {
            $o['name'] = $v->getName();
            $o['props'] = $this->props($v, $depth);
        } elseif ($v instanceof \DateInterval) {
            $o['spec'] = ['y' => $v->y, 'm' => $v->m, 'd' => $v->d, 'h' => $v->h, 'i' => $v->i, 's' => $v->s, 'f' => $v->f, 'invert' => $v->invert, 'days' => $v->days];
            $o['props'] = $this->props($v, $depth);
            if (interval_is_wall($v)) {
                $o['wall'] = true;
            }
        } elseif ($v instanceof \Twig\TemplateWrapper) {
            $o['template'] = $v->getTemplateName();
            $userland = false; // its private properties are the whole Environment
        } elseif ($v instanceof \ArrayObject || $v instanceof \ArrayIterator) {
            $o['entries'] = $this->encode($v->getArrayCopy(), $depth + 1);
        } elseif ($v instanceof \SimpleXMLElement) {
            $o['xml'] = $v->asXML();
        } else {
            $o['interfaces'] = array_values(class_implements($v));
            sort($o['interfaces']);
            if (method_exists($v, '__toString') && !$v instanceof \Throwable) {
                try {
                    $o['toString'] = $this->encode((string) $v, $depth + 1);
                } catch (\Throwable $e) {
                    $o['toString_error'] = describe_throwable($e);
                }
            }
            $props = [];
            foreach (get_object_vars($v) as $k => $pv) {
                $props[] = [$k, $this->encode($pv, $depth + 1)];
            }
            $o['props'] = $props;
        }

        // Extras (not in typed()).
        $o['methods'] = get_class_methods($v);
        $o['parents'] = array_values(class_parents($v));
        $o['interfaces_raw'] = array_values(class_implements($v));
        if ($userland) {
            $hidden = [];
            foreach (get_mangled_object_vars($v) as $k => $pv) {
                if (\is_string($k) && '' !== $k && "\0" === $k[0]) {
                    $hidden[] = self::keyItem($k, $this->encode($pv, $depth + 1));
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
                $storage[] = [$this->encode($obj, $depth + 1), $this->encode($v[$obj], $depth + 1)];
            }
            $v->rewind();
            $o['storage'] = $storage;
        }
        if ($v instanceof \CallbackFilterIterator) {
            $inner = $v->getInnerIterator();
            $o['inner'] = $this->encode($inner, $depth + 1);
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
}

/**
 * Whether add() and sub() of the interval take its time part in wall-clock time: php_interval_obj's civil_or_wall
 * is PHP_DATE_WALL, which only `new DateInterval()` sets (diff(), createFromDateString(), __set_state() and
 * unserialize() give PHP_DATE_CIVIL), and PHP shows it nowhere (PHPSRC/ext/date/php_date.c: php_date_add(),
 * php_date_sub()). The modes differ only for an interval with a time part, where the arithmetic crosses a change of
 * the UTC offset: the interval is added to and subtracted from instants around the end of DST in New York
 * (2021-11-07 06:00 UTC) and compared with an unserialize()d copy, which has the civil mode.
 */
function interval_is_wall(\DateInterval $interval): bool
{
    try {
        if (0 === $interval->h && 0 === $interval->i && 0 === $interval->s && 0.0 === (float) $interval->f) {
            return false;
        }
        $civil = unserialize(serialize($interval));
        $zone = new \DateTimeZone('America/New_York');
        foreach ([-3599, -1800, -1, 1, 1800, 3599] as $offset) {
            $probe = (new \DateTimeImmutable('@'.(1636264800 + $offset)))->setTimezone($zone);
            if ($probe->add($interval) != $probe->add($civil) || $probe->sub($interval) != $probe->sub($civil)) {
                return true;
            }
        }
    } catch (\Throwable) {
        // an interval that is not initialised: nothing to rebuild either
    }

    return false;
}

/** RichEncoder::encode() of a value, with every diagnostic it raises swallowed. */
function typed_rich($v): array
{
    set_error_handler(static fn (): bool => true);
    try {
        return (new RichEncoder())->encode($v);
    } catch (\Throwable $e) {
        return ['t' => 'error', 'error' => describe_throwable($e)];
    } finally {
        restore_error_handler();
    }
}

/** Captures var_dump() output of a value. */
function var_dump_string($v): string
{
    ob_start();
    var_dump($v);

    return (string) ob_get_clean();
}

/**
 * Builds an Environment the way Twig\Test\IntegrationTestCase::doIntegrationTest() does for
 * Twig\Tests\IntegrationTest (tests/IntegrationTest.php): same defaults, global, runtime loaders, extensions
 * and undefined-callable callbacks. $config is merged over ['cache' => false, 'strict_variables' => true].
 */
function integration_environment(array $templates, array $config): Environment
{
    static $test = null;
    if (null === $test) {
        // Loading the file declares every helper class used by fixture DATA (TwigTestFoo, ToStringStub, ...).
        $test = new \Twig\Tests\IntegrationTest('testIntegration');
    }
    // getRuntimeLoaders(), getUndefined*Callbacks() ... are protected: call them through reflection.
    $call = static fn (string $method) => (new \ReflectionMethod($test, $method))->invoke($test);

    $config = array_merge(['cache' => false, 'strict_variables' => true], $config);
    $twig = new Environment(new ArrayLoader($templates), $config);
    $twig->addGlobal('global', 'global');
    foreach ($call('getRuntimeLoaders') as $runtimeLoader) {
        $twig->addRuntimeLoader($runtimeLoader);
    }
    foreach ($test->getExtensions() as $extension) {
        $twig->addExtension($extension);
    }
    foreach ($call('getTwigFilters') as $filter) {
        $twig->addFilter($filter);
    }
    foreach ($call('getTwigTests') as $t) {
        $twig->addTest($t);
    }
    foreach ($call('getTwigFunctions') as $function) {
        $twig->addFunction($function);
    }
    foreach ($call('getUndefinedFilterCallbacks') as $callback) {
        $twig->registerUndefinedFilterCallback($callback);
    }
    foreach ($call('getUndefinedFunctionCallbacks') as $callback) {
        $twig->registerUndefinedFunctionCallback($callback);
    }
    foreach ($call('getUndefinedTestCallbacks') as $callback) {
        $twig->registerUndefinedTestCallback($callback);
    }
    foreach ($call('getUndefinedTokenParserCallbacks') as $callback) {
        $twig->registerUndefinedTokenParserCallback($callback);
    }

    return $twig;
}

/** Plain Environment: Twig defaults + optional core extras (what a normal application gets). */
function plain_environment(array $templates, array $config, array $extensions, array $globals, ?array $sandbox): Environment
{
    $twig = new Environment(new ArrayLoader($templates), array_merge(['cache' => false], $config));
    foreach ($globals as $k => $v) {
        $twig->addGlobal($k, $v);
    }
    foreach ($extensions as $name) {
        switch ($name) {
            case 'debug': $twig->addExtension(new \Twig\Extension\DebugExtension()); break;
            case 'string_loader': $twig->addExtension(new \Twig\Extension\StringLoaderExtension()); break;
            case 'sandbox': break; // handled below
            default: throw new \InvalidArgumentException(\sprintf('Unknown extension "%s" (supported: debug, string_loader, sandbox).', $name));
        }
    }
    if (null !== $sandbox || \in_array('sandbox', $extensions, true)) {
        $p = $sandbox['policy'] ?? [];
        $policy = new \Twig\Sandbox\SecurityPolicy($p['tags'] ?? [], $p['filters'] ?? [], $p['methods'] ?? [], $p['properties'] ?? [], $p['functions'] ?? [], $p['tests'] ?? []);
        if (isset($p['strict'])) {
            $policy->setStrict((bool) $p['strict']);
        }
        $sourcePolicy = null;
        if (isset($sandbox['source_policy'])) {
            // the templates of these names run sandboxed (SourcePolicyInterface, deprecated since Twig 3.27)
            $sourcePolicy = new class((array) $sandbox['source_policy']) implements \Twig\Sandbox\SourcePolicyInterface {
                public function __construct(private array $names)
                {
                }

                public function enableSandbox(\Twig\Source $source): bool
                {
                    return \in_array($source->getName(), $this->names, true);
                }
            };
        }
        // (the deprecation a source policy triggers comes before the oracle records anything, like other setup notices)
        $twig->addExtension(new \Twig\Extension\SandboxExtension($policy, (bool) ($sandbox['global'] ?? false), $sourcePolicy));
    }

    return $twig;
}

/** Twig\Test\IntegrationTestCase::parseTemplates() (verbatim regex). */
function parse_templates(string $test): array
{
    $templates = [];
    preg_match_all('/--TEMPLATE(?:\((.*?)\))?--(.*?)(?=\-\-TEMPLATE|$)/s', $test, $matches, \PREG_SET_ORDER);
    foreach ($matches as $match) {
        $templates[$match[1] ?: 'index.twig'] = $match[2];
    }

    return $templates;
}

/** Parses one .test file exactly like IntegrationTestCase::assembleTests() (verbatim regexes). */
function parse_fixture(string $test): ?array
{
    if (preg_match('/--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)\s*(?:--DATA--\s*(.*))?\s*--EXCEPTION--\s*(.*)/sx', $test, $match)) {
        return [
            'message' => $match[1], 'condition' => $match[2], 'deprecation' => $match[3],
            'templates' => parse_templates($match[4]), 'exception' => $match[6],
            'outputs' => [[null, $match[5], null, '']],
        ];
    }
    if (preg_match('/--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)--DATA--.*?--EXPECT--.*/s', $test, $match)) {
        preg_match_all('/--DATA--(.*?)(?:--CONFIG--(.*?))?--EXPECT--(.*?)(?=\-\-DATA\-\-|$)/s', $test, $outputs, \PREG_SET_ORDER);

        return [
            'message' => $match[1], 'condition' => $match[2], 'deprecation' => $match[3],
            'templates' => parse_templates($match[4]), 'exception' => false, 'outputs' => $outputs,
        ];
    }

    return null;
}

/** Evaluates fixture-style PHP ("return [...]" statements) with $twig in scope, like the harness does. */
function eval_with_twig(?string $code, ?Environment $twig)
{
    return (static function () use ($code, $twig) {
        return eval($code.';');
    })();
}

/**
 * Oracle convenience for data_php/config_php: fixture-style statements when the code contains a "return" keyword,
 * otherwise the code is treated as a single expression (e.g. "['a' => new \ArrayObject([1])]").
 */
function eval_data_code(string $code, ?Environment $twig)
{
    if (!preg_match('/\breturn\b/i', $code)) {
        $code = 'return ('.rtrim(trim($code), "; \t\n\r").')';
    }

    return eval_with_twig($code, $twig);
}

const FIXTURES_DIR = __DIR__.'/vendor/twig/twig/tests/Fixtures';

/**
 * Faithful re-implementation of Twig\Test\IntegrationTestCase::doIntegrationTest() (Twig 3.31.1,
 * src/Test/IntegrationTestCase.php:233-382) for Twig\Tests\IntegrationTest, except that it records the actual
 * results instead of asserting. "pass" reproduces the harness verdict.
 * Options: "typed" => true adds the typed tree of each DATA context and the CoreExtension/timezone state
 * observed right before rendering.
 */
function run_fixture(array $case): array
{
    $rel = $case['fixture'];
    $path = str_starts_with($rel, '/') ? $rel : FIXTURES_DIR.'/'.$rel;
    $test = file_get_contents($path);
    $res = ['id' => $case['id'] ?? null, 'kind' => 'fixture', 'file' => $rel];
    $p = parse_fixture($test);
    if (null === $p) {
        throw new \InvalidArgumentException(\sprintf('Test "%s" is not valid.', $rel));
    }
    $res['message'] = $p['message'];
    $res['legacy'] = str_contains($path, '.legacy.test');
    $res['exception_expected'] = $p['exception'];
    $res['deprecation_expected'] = $p['deprecation'];
    $res['skipped'] = null;
    if ($p['condition']) {
        $ret = '';
        eval('$ret = '.$p['condition'].';');
        if (!$ret) {
            $res['skipped'] = 'condition: '.trim($p['condition']);
            $res['cases'] = [];

            return $res;
        }
    }

    $wantTyped = (bool) ($case['typed'] ?? false);
    $templateSources = $p['templates'];
    $res['cases'] = [];
    foreach ($p['outputs'] as $i => $match) {
        $c = ['index' => $i];
        $config = $match[2] ? eval_with_twig($match[2], null) : [];
        foreach ($templateSources as $name => $template) {
            $templateSources[$name] = $template.str_repeat(' ', $i); // cumulative, exactly like the harness
        }
        if ($wantTyped) {
            $c['config'] = typed(array_merge(['cache' => false, 'strict_variables' => true], $config));
        }
        $twig = integration_environment($templateSources, $config);
        // Like the harness: load-phase deprecations are always collected; render-phase ones are compared only when
        // the fixture has a --DEPRECATION-- section. Other diagnostics are thrown (PHPUnit 9 converts warnings and
        // notices into exceptions; here they become \ErrorException).
        $loadDiag = new Diagnostics();
        $renderDiag = new Diagnostics();
        $c['phase'] = 'load';
        try {
            $templates = $loadDiag->run(static function () use ($twig, $templateSources) {
                $templates = [];
                foreach (array_keys($templateSources) as $templateName) {
                    $templates[$templateName] = $twig->load($templateName);
                }

                return $templates;
            }, 'throw');
        } catch (\Exception $e) {
            $c['error'] = describe_throwable($e);
            $c['output'] = null;
            if (false !== $p['exception']) {
                $message = $e->getMessage();
                $last = substr($message, -1);
                $c['pass'] = trim($p['exception']) === trim(\sprintf('%s: %s', $e::class, $message)) && ('.' === $last || '?' === $last);
            } else {
                $c['pass'] = false;
            }
            $c['deprecations'] = $loadDiag->deprecations;
            $res['cases'][] = $c;
            break; // the harness returns here
        }

        $c['phase'] = 'render';
        try {
            $output = trim($renderDiag->run(static function () use ($templates, $match, $twig, $wantTyped, &$c) {
                $context = eval_with_twig($match[1], $twig);
                if ($wantTyped) {
                    $c['data'] = typed($context);
                    $core = $twig->getExtension(\Twig\Extension\CoreExtension::class);
                    $c['state'] = [
                        'php_default_timezone' => date_default_timezone_get(),
                        'core_timezone' => $core->getTimezone()->getName(),
                        'core_date_format' => $core->getDateFormat(),
                        'core_number_format' => $core->getNumberFormat(),
                    ];
                }

                return $templates['index.twig']->render($context);
            }, 'throw'), "\n ");
            $c['error'] = null;
            put_string($c, 'output', $output);
        } catch (\Exception $e) {
            $c['error'] = describe_throwable($e);
            if (false !== $p['exception']) {
                $c['output'] = null;
                $c['pass'] = format_matches(trim($p['exception']), trim(\sprintf('%s: %s', $e::class, $e->getMessage())));
                $c['deprecations'] = $loadDiag->deprecations;
                $c['render_deprecations'] = $renderDiag->deprecations;
                $res['cases'][] = $c;
                break;
            }
            $wrapped = new \Twig\Error\Error(\sprintf('%s: %s', $e::class, $e->getMessage()), -1, null, $e);
            $output = trim(\sprintf('%s: %s', $wrapped::class, $wrapped->getMessage()));
            put_string($c, 'output', $output);
        }
        $compared = '' !== $p['deprecation'] ? array_merge($loadDiag->deprecations, $renderDiag->deprecations) : $loadDiag->deprecations;
        if (false !== $p['exception']) {
            $c['pass'] = false; // an exception was expected but none was thrown
        } else {
            $expected = trim($match[3], "\n ");
            put_string($c, 'expected', $expected);
            $c['pass'] = $expected === $output && $p['deprecation'] === implode("\n", $compared);
        }
        $c['deprecations'] = $compared;
        $c['render_deprecations'] = $renderDiag->deprecations;
        $res['cases'][] = $c;
    }

    return $res;
}

/** PHPUnit 9.6 assertStringMatchesFormat() (Framework/Constraint/String/StringMatchesFormatDescription.php). */
function format_matches(string $format, string $string): bool
{
    $string = preg_replace('/\r\n/', "\n", $string);
    $format = preg_replace('/\r\n/', "\n", $format);
    $regex = strtr(
        preg_quote($format, '/'),
        [
            '%%' => '%',
            '%e' => '\\'.\DIRECTORY_SEPARATOR,
            '%s' => '[^\r\n]+',
            '%S' => '[^\r\n]*',
            '%a' => '.+',
            '%A' => '.*',
            '%w' => '\s*',
            '%i' => '[+-]?\d+',
            '%d' => '\d+',
            '%x' => '[0-9a-fA-F]+',
            '%f' => '[+-]?\.?\d+\.?\d*(?:[Ee][+-]?\d+)?',
            '%c' => '.',
        ]
    );

    return (bool) preg_match('/^'.$regex.'$/s', $string);
}

<?php
/*
 * Oracle vectors for the parameters of the public methods of the date classes (the arginfo PHP checks named
 * arguments against, `zend_handle_named_arg()`), read with Reflection. Regenerate with
 * tests/vectors/regen.sh method_params.
 *
 *  "classes": [[class, [method, ...]], ...]   methods in get_class_methods() order, each
 *     [name, static, [[param, optional, default], ...]]
 *     default: {"value": <JSON>} when ReflectionParameter::isDefaultValueAvailable(), else null
 *  "unpack": [[class, method, spread, result], ...]
 *     result of `$object->$method(...$spread)` for spreads that fail while the arguments are sent:
 *     "Class: message" (the call is never made when sending fails); spread: [[key, value], ...] in order
 */
require __DIR__ . '/common.inc.php';
now(1700000000, 123456);
date_default_timezone_set('UTC');

$start = new DateTimeImmutable('2020-01-01');
$period = new DatePeriod($start, new DateInterval('P1D'), 2);
$objects = [
    'DateTime' => new DateTime('2020-01-02 03:04:05'),
    'DateTimeImmutable' => new DateTimeImmutable('2020-01-02 03:04:05'),
    'DateTimeZone' => new DateTimeZone('Europe/Vilnius'),
    'DateInterval' => new DateInterval('P1D'),
    'DatePeriod' => $period,
    'InternalIterator' => $period->getIterator(),
];

$classes = [];
$unpack = [];
foreach ($objects as $class => $object) {
    $methods = [];
    foreach (get_class_methods($object) as $name) {
        $m = new ReflectionMethod($object, $name);
        $params = [];
        foreach ($m->getParameters() as $p) {
            $params[] = [
                $p->getName(),
                $p->isOptional(),
                $p->isDefaultValueAvailable() ? ['value' => $p->getDefaultValue()] : null,
            ];
        }
        $methods[] = [$name, $m->isStatic(), $params];

        // An unknown name before a positional element fails at the name; a declared name before one fails at
        // the positional element. Neither reaches the method.
        $spreads = [['zz' => 1, 0 => 2]];
        if ($params) {
            $spreads[] = [$params[0][0] => 1, 0 => 2];
        }
        foreach ($spreads as $spread) {
            try {
                $object->$name(...$spread);
                $result = 'no error';
            } catch (Throwable $e) {
                $result = exc($e);
            }
            take_diags();
            $pairs = [];
            foreach ($spread as $key => $value) {
                $pairs[] = [$key, $value];
            }
            $unpack[] = [$class, $name, $pairs, $result];
        }
    }
    $classes[] = [$class, $methods];
}

emit('method_params.gen.php', ['classes' => $classes, 'unpack' => $unpack]);

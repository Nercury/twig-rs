<?php
// Shared by the array/*.gen.php generators (on top of ../common.inc.php and ../values.inc.php):
// the array encoding with the next free key, and the named callbacks the Rust tests mirror.

declare(strict_types=0);

/**
 * values.inc.php enc() for everything but arrays; arrays additionally carry "next": the key that
 * `$a[] = x` would use (nNextFreeElement, 0 when unset), or null when that key is occupied.
 */
function encn($v): array
{
    if (is_array($v)) {
        $items = [];
        foreach ($v as $k => $x) {
            $items[] = [is_int($k) ? ['t' => 'int', 'v' => $k] : ['t' => 'string', 'hex' => bin2hex($k)], encn($x)];
        }
        $t = $v;
        try {
            $t[] = 0;
            $next = array_key_last($t);
        } catch (\Error $e) {
            $next = null;
        }
        return ['t' => 'array', 'items' => $items, 'next' => $next];
    }
    return enc($v);
}

/** {"ok": encn(result)} or {"err": [class, message index]}. */
function resn($r): array
{
    if ($r instanceof \Throwable) {
        return ['err' => [get_class($r), msg(bin2hex($r->getMessage()))]];
    }
    return ['ok' => encn($r)];
}

/** Comparators for usort()/uasort()/uksort(), counting their calls in $calls. */
function comparators(int &$calls): array
{
    return [
        'cmp' => static function ($a, $b) use (&$calls) {
            $calls++;
            return $a <=> $b;
        },
        'rev' => static function ($a, $b) use (&$calls) {
            $calls++;
            return $b <=> $a;
        },
        'sub' => static function ($a, $b) use (&$calls) {
            $calls++;
            return $a - $b;
        },
        'gt' => static function ($a, $b) use (&$calls) {
            $calls++;
            return $a > $b;
        },
        'lt' => static function ($a, $b) use (&$calls) {
            $calls++;
            return $a < $b;
        },
        'half' => static function ($a, $b) use (&$calls) {
            $calls++;
            return ($a <=> $b) * 0.5;
        },
        'big' => static function ($a, $b) use (&$calls) {
            $calls++;
            return ($a <=> $b) * 1e30;
        },
        'str' => static function ($a, $b) use (&$calls) {
            $calls++;
            return (string) ($a <=> $b);
        },
        'strx' => static function ($a, $b) use (&$calls) {
            $calls++;
            return ($a <=> $b) . 'x';
        },
        'null' => static function ($a, $b) use (&$calls) {
            $calls++;
            return null;
        },
        'arr' => static function ($a, $b) use (&$calls) {
            $calls++;
            return [$a];
        },
        'obj' => static function ($a, $b) use (&$calls) {
            $calls++;
            return new stdClass();
        },
        'nan' => static function ($a, $b) use (&$calls) {
            $calls++;
            return NAN;
        },
        'throw' => static function ($a, $b) use (&$calls) {
            $calls++;
            if ($calls === 3) {
                throw new Exception('stop at 3');
            }
            return $a <=> $b;
        },
    ];
}

/** Callbacks for array_filter()/array_map()/array_reduce(), counting their calls in $calls. */
function callbacks(int &$calls): array
{
    return [
        // array_filter
        'truthy' => static function (...$args) use (&$calls) {
            $calls++;
            return $args[0];
        },
        'odd' => static function (...$args) use (&$calls) {
            $calls++;
            return is_int($args[0]) && $args[0] % 2 !== 0;
        },
        'both' => static function (...$args) use (&$calls) {
            $calls++;
            return count($args) === 2 && $args[1] !== 'b' && $args[1] !== 1;
        },
        'nargs' => static function (...$args) use (&$calls) {
            $calls++;
            return count($args) === 1 ? 0.0 : 'yes';
        },
        // array_map
        'pack' => static function (...$args) use (&$calls) {
            $calls++;
            return $args;
        },
        'first' => static function (...$args) use (&$calls) {
            $calls++;
            return $args[0];
        },
        // array_reduce
        'concat' => static function (...$args) use (&$calls) {
            $calls++;
            return $args[0] . '|' . (is_array($args[1]) ? 'A' : $args[1]);
        },
        'count' => static function (...$args) use (&$calls) {
            $calls++;
            return ($args[0] ?? 0) + 1;
        },
        // everywhere
        'throw' => static function (...$args) use (&$calls) {
            $calls++;
            if ($calls === 2) {
                throw new RuntimeException('callback stop');
            }
            return $args[0];
        },
    ];
}

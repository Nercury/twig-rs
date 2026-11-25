<?php
// Shared by operators.gen.php, compare.gen.php and dumps.gen.php: test classes, the typed encoding
// of values with full contents, and a message table for diagnostics and exceptions.

declare(strict_types=0);

/** __toString() returns its constructor argument. */
class S
{
    public function __construct(private string $s = 'abc')
    {
    }

    public function __toString(): string
    {
        return $this->s;
    }
}

/** Declared properties of every visibility. */
class P
{
    public $a = 1;
    protected $b = 2;
    private $c = 3;
}

/** A typed property without default (uninitialized) next to a nullable one. */
class T
{
    public ?int $typed;
    public $n = null;
}

enum Suit: string
{
    case Hearts = 'H';
    case Spades = 'S';
}

enum Num: int
{
    case One = 1;
}

enum U
{
    case A;
    case B;
}

/** Message table: hex message -> index. */
$GLOBALS['__msgs'] = [];
function msg(string $hex): int
{
    if (!isset($GLOBALS['__msgs'][$hex])) {
        $GLOBALS['__msgs'][$hex] = count($GLOBALS['__msgs']);
    }
    return $GLOBALS['__msgs'][$hex];
}

function messages(): array
{
    return array_keys($GLOBALS['__msgs']);
}

/** Diagnostics as [level, message index] pairs. */
function diags(array $d): array
{
    return array_map(static fn ($p) => [$p[0], msg($p[1])], $d);
}

/** Typed JSON with full contents (arrays and object properties included). */
function enc($v): array
{
    if (is_array($v)) {
        $items = [];
        foreach ($v as $k => $x) {
            $items[] = [is_int($k) ? ['t' => 'int', 'v' => $k] : ['t' => 'string', 'hex' => bin2hex($k)], enc($x)];
        }
        return ['t' => 'array', 'items' => $items];
    }
    if ($v instanceof \UnitEnum) {
        return ['t' => 'enum', 'class' => get_class($v), 'case' => $v->name];
    }
    if ($v instanceof \Closure) {
        return ['t' => 'closure'];
    }
    if (is_object($v)) {
        $props = [];
        foreach ((array) $v as $k => $x) {
            $props[] = [bin2hex((string) $k), enc($x)];
        }
        $o = ['t' => 'object', 'class' => get_class($v), 'props' => $props];
        if ($v instanceof S) {
            $o['str'] = bin2hex((string) $v);
        }
        return $o;
    }
    return typed($v);
}

/** {"ok": enc(result)} or {"err": [class, message index]}. */
function res($r): array
{
    if ($r instanceof \Throwable) {
        return ['err' => [get_class($r), msg(bin2hex($r->getMessage()))]];
    }
    return ['ok' => enc($r)];
}

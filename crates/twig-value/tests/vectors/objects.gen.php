<?php
// Generates objects.json: PHP 8.5.13 behaviour of the object protocol (property access, magic methods,
// method calls, string conversion, iteration, ArrayAccess, Countable, enums, closures, ArrayObject,
// ArrayIterator, RecursiveArrayIterator and subclasses of them) as seen from outside the classes.
// tests/vectors.rs re-implements the test classes below as Rust host objects and replays every
// scenario by name.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/objects.gen.php > objects.json

declare(strict_types=0);
require __DIR__.'/common.inc.php';

$GLOBALS['log'] = [];

class P
{
    public $x = 1;
    public int $t;
    public ?int $n = null;
    private $priv = 2;
    protected $prot = 3;
    public static $st = 4;
    public readonly int $ro;

    public function __construct()
    {
        $this->ro = 5;
    }

    public function getX()
    {
        return $this->x;
    }

    private function hidden()
    {
        return 'hidden';
    }
}

class M
{
    public $real = 'r';
    private $secret = 's';

    public function __get($n)
    {
        return "get:$n";
    }

    public function __isset($n)
    {
        return 'no' !== $n;
    }

    public function __call($n, $a)
    {
        return "call:$n:".count($a).':'.implode(',', array_keys($a));
    }
}

class S
{
    public function __toString(): string
    {
        return 'str';
    }
}

class S5
{
    public function __toString()
    {
        return 5;
    }
}

class SBad
{
    public function __toString()
    {
        return [];
    }
}

class It implements Iterator
{
    private $i = 0;
    private $k = ['a', 'b', 'c'];

    public function rewind(): void
    {
        $GLOBALS['log'][] = 'rewind';
        $this->i = 0;
    }

    public function valid(): bool
    {
        $GLOBALS['log'][] = 'valid';
        return $this->i < 3;
    }

    public function current(): mixed
    {
        $GLOBALS['log'][] = 'current';
        return $this->i * 10;
    }

    public function key(): mixed
    {
        $GLOBALS['log'][] = 'key';
        return $this->k[$this->i];
    }

    public function next(): void
    {
        $GLOBALS['log'][] = 'next';
        $this->i++;
    }
}

class Agg implements IteratorAggregate
{
    public function getIterator(): Iterator
    {
        return new ArrayIterator(['x' => 1, 'y' => 2]);
    }
}

class C implements Countable, ArrayAccess
{
    private $data = ['a' => 1, 'z' => null];

    public function count(): int
    {
        return 42;
    }

    public function offsetExists($o): bool
    {
        $GLOBALS['log'][] = 'offsetExists';
        return array_key_exists($o, $this->data);
    }

    public function offsetGet($o): mixed
    {
        $GLOBALS['log'][] = 'offsetGet';
        return $this->data[$o] ?? null;
    }

    public function offsetSet($o, $v): void
    {
        $GLOBALS['log'][] = 'offsetSet';
        if (null === $o) {
            $this->data[] = $v;
        } else {
            $this->data[$o] = $v;
        }
    }

    public function offsetUnset($o): void
    {
        $GLOBALS['log'][] = 'offsetUnset';
        unset($this->data[$o]);
    }
}

// Subclasses of the SPL array classes that override nothing (ArrayObject::with_class() and
// ArrayIterator::with_class() in Rust).
final class TwigValueAoSub extends ArrayObject
{
}

final class TwigValueAiSub extends ArrayIterator
{
}

final class TwigValueRaiSub extends RecursiveArrayIterator
{
}

enum Suit: string
{
    case Hearts = 'H';
    case Spades = 'S';
    const Wild = self::Spades;
}

enum Num: int
{
    case One = 1;
    case Two = 2;
}

enum U
{
    case A;
    case B;
}

function deep($v)
{
    if (is_array($v)) {
        $e = [];
        foreach ($v as $k => $x) {
            $e[] = [is_int($k) ? ['i' => $k] : ['s' => bin2hex($k)], deep($x)];
        }
        return ['t' => 'array', 'e' => $e];
    }
    if (is_object($v)) {
        $r = ['t' => 'object', 'class' => get_class($v)];
        if ($v instanceof UnitEnum) {
            $r['case'] = $v->name;
        }
        return $r;
    }
    return typed($v);
}

$scenarios = [
    // --- declared, typed, private, protected, static, readonly properties
    'P read x' => fn () => (new P())->x,
    'P read t' => fn () => (new P())->t,
    'P isset t' => fn () => isset((new P())->t),
    'P read n' => fn () => (new P())->n,
    'P isset n' => fn () => isset((new P())->n),
    'P isset x' => fn () => isset((new P())->x),
    'P read priv' => fn () => (new P())->priv,
    'P isset priv' => fn () => isset((new P())->priv),
    'P read prot' => fn () => (new P())->prot,
    'P read st' => fn () => (new P())->st,
    'P read nope' => fn () => (new P())->nope,
    'P read nope quiet' => fn () => (new P())->nope ?? 'dflt',
    'P read t quiet' => fn () => (new P())->t ?? 'dflt',
    'P exists priv' => fn () => property_exists(new P(), 'priv'),
    'P exists nope' => fn () => property_exists(new P(), 'nope'),
    'P exists t' => fn () => property_exists(new P(), 't'),
    'P write dyn' => function () {
        $p = new P();
        $p->dyn = 7;
        return $p->dyn;
    },
    'P write ro' => function () {
        $p = new P();
        $p->ro = 6;
    },
    'P write priv' => function () {
        $p = new P();
        $p->priv = 6;
    },
    'P write t' => function () {
        $p = new P();
        $p->t = 9;
        return $p->t;
    },
    'P unset x read' => function () {
        $p = new P();
        unset($p->x);
        return $p->x;
    },
    'P unset t read' => function () {
        $p = new P();
        unset($p->t);
        return $p->t;
    },
    'P unset ro' => function () {
        $p = new P();
        unset($p->ro);
    },
    'P array cast' => fn () => (array) new P(),
    'P object vars' => fn () => get_object_vars(new P()),
    'P call getX' => fn () => (new P())->GETX(),
    'P call nope' => fn () => (new P())->nope(),
    'P call hidden' => fn () => (new P())->hidden(),
    'P methods' => fn () => get_class_methods(new P()),
    // --- magic methods
    'M read real' => fn () => (new M())->real,
    'M read secret' => fn () => (new M())->secret,
    'M read nope' => fn () => (new M())->nope,
    'M isset real' => fn () => isset((new M())->real),
    'M isset secret' => fn () => isset((new M())->secret),
    'M isset nope' => fn () => isset((new M())->nope),
    'M isset no' => fn () => isset((new M())->no),
    'M empty nope' => fn () => empty((new M())->nope),
    'M quiet no' => fn () => (new M())->no ?? 'dflt',
    'M quiet nope' => fn () => (new M())->nope ?? 'dflt',
    'M call foo' => fn () => (new M())->foo(1, 2),
    'M call named' => fn () => (new M())->foo(1, b: 2),
    'M write secret' => function () {
        $m = new M();
        $m->secret = 1;
    },
    'M exists secret' => fn () => property_exists(new M(), 'secret'),
    // --- string conversion
    'S string' => fn () => (string) new S(),
    'S5 string' => fn () => (string) new S5(),
    'SBad string' => fn () => (string) new SBad(),
    'S int' => fn () => (int) new S(),
    'S bool' => fn () => (bool) new S(),
    'P string' => fn () => (string) new P(),
    'S implements' => fn () => class_implements(new S()),
    // --- iteration
    'It foreach' => function () {
        $GLOBALS['log'] = [];
        $out = [];
        foreach (new It() as $k => $v) {
            $out[$k] = $v;
        }
        return [$out, $GLOBALS['log']];
    },
    'It to_array' => function () {
        $GLOBALS['log'] = [];
        return [iterator_to_array(new It()), $GLOBALS['log']];
    },
    'It to_array values' => function () {
        $GLOBALS['log'] = [];
        return [iterator_to_array(new It(), false), $GLOBALS['log']];
    },
    'It count' => function () {
        $GLOBALS['log'] = [];
        return [iterator_count(new It()), $GLOBALS['log']];
    },
    'Agg to_array' => fn () => iterator_to_array(new Agg()),
    'Agg count' => fn () => iterator_count(new Agg()),
    'P foreach' => function () {
        $out = [];
        foreach (new P() as $k => $v) {
            $out[$k] = $v;
        }
        return $out;
    },
    // --- ArrayAccess / Countable
    'C count' => fn () => count(new C()),
    'C read a' => function () {
        $GLOBALS['log'] = [];
        $c = new C();
        return [$c['a'], $GLOBALS['log']];
    },
    'C read quiet zz' => function () {
        $GLOBALS['log'] = [];
        $c = new C();
        return [$c['zz'] ?? 'dflt', $GLOBALS['log']];
    },
    'C isset a' => function () {
        $GLOBALS['log'] = [];
        return [isset((new C())['a']), $GLOBALS['log']];
    },
    'C isset z' => function () {
        $GLOBALS['log'] = [];
        return [isset((new C())['z']), $GLOBALS['log']];
    },
    'C empty a' => function () {
        $GLOBALS['log'] = [];
        return [empty((new C())['a']), $GLOBALS['log']];
    },
    'C write' => function () {
        $GLOBALS['log'] = [];
        $c = new C();
        $c[] = 5;
        $c['k'] = 6;
        unset($c['a']);
        return [$c['k'], $GLOBALS['log']];
    },
    'P dim read' => function () {
        $p = new P();
        return $p['x'];
    },
    'P count' => fn () => count(new P()),
    // --- stdClass
    'std dyn' => function () {
        $o = new stdClass();
        $o->a = 1;
        $o->{'0'} = 'zero';
        return [(array) $o, isset($o->a), isset($o->b), property_exists($o, 'a')];
    },
    'std read nope' => fn () => (new stdClass())->nope,
    'std clone' => function () {
        $o = new stdClass();
        $o->a = 1;
        $c = clone $o;
        $c->a = 2;
        return [$o->a, $c->a];
    },
    'object cast array' => fn () => (array) (object) ['a' => 1, 5 => 'five'],
    'object cast scalar' => fn () => (array) (object) 'x',
    'array cast closure' => fn () => count((array) (fn () => 1)),
    // --- enums
    'enum from' => fn () => Suit::from('H'),
    'enum from bad' => fn () => Suit::from('X'),
    'enum from int' => fn () => Suit::from(5),
    'enum tryFrom bad' => fn () => Suit::tryFrom('X'),
    'enum int from' => fn () => Num::from('2'),
    'enum int from bad' => fn () => Num::from(7),
    'enum int from str' => fn () => Num::from('abc'),
    'enum int from float' => fn () => Num::from(1.5),
    'enum cases' => fn () => Suit::cases(),
    'enum cases via case' => fn () => Suit::Hearts->cases(),
    'enum name value' => fn () => [Suit::Hearts->name, Suit::Hearts->value, U::A->name],
    'enum unit value' => fn () => U::A->value,
    'enum write name' => function () {
        $c = Suit::Hearts;
        $c->name = 'x';
    },
    'enum write dyn' => function () {
        $c = Suit::Hearts;
        $c->foo = 'x';
    },
    'enum read nope' => fn () => Suit::Hearts->nope,
    'enum clone' => fn () => clone Suit::Hearts,
    'enum string' => fn () => (string) Suit::Hearts,
    'enum int' => fn () => (int) U::A,
    'enum bool' => fn () => (bool) U::A,
    'enum array cast' => fn () => (array) Suit::Hearts,
    'enum constant' => fn () => [constant('Suit::Wild'), constant('Suit::Hearts')],
    'enum implements' => fn () => [class_implements(Suit::Hearts), class_implements(U::A)],
    'enum methods' => fn () => [get_class_methods(Suit::Hearts), get_class_methods(U::A)],
    'enum identical' => fn () => Suit::from('H') === Suit::Hearts,
    // --- closures
    'closure invoke' => fn () => (fn ($a, $b) => $a + $b)(2, 3),
    'closure __invoke' => fn () => (fn ($a) => $a * 2)->__invoke(4),
    'closure read' => fn () => (fn () => 1)->foo,
    'closure write' => function () {
        $f = fn () => 1;
        $f->foo = 1;
    },
    'closure isset' => fn () => isset((fn () => 1)->foo),
    'closure clone' => function () {
        $f = fn () => 7;
        $g = clone $f;
        return [$g(), $f === $g];
    },
    'closure string' => fn () => (string) (fn () => 1),
    'closure methods' => fn () => get_class_methods(fn () => 1),
    'not callable' => fn () => (new P())(),
    // --- ArrayObject
    'ao read missing' => fn () => (new ArrayObject(['a' => 1]))['zz'],
    'ao read missing int' => fn () => (new ArrayObject([1]))[5],
    'ao read null' => fn () => (new ArrayObject(['' => 'e']))[null],
    'ao read array key' => fn () => (new ArrayObject([]))[[]],
    'ao read float' => fn () => (new ArrayObject([1 => 'one']))[1.5],
    'ao read numeric string' => fn () => (new ArrayObject([10 => 'ten']))['10'],
    'ao quiet missing' => fn () => (new ArrayObject([]))['zz'] ?? 'dflt',
    'ao offsetExists null' => fn () => (new ArrayObject(['' => null]))->offsetExists(null),
    'ao offsetExists null value' => fn () => (new ArrayObject(['a' => null]))->offsetExists('a'),
    'ao isset null value' => fn () => isset((new ArrayObject(['a' => null]))['a']),
    'ao isset array key' => fn () => isset((new ArrayObject([]))[[]]),
    'ao empty' => fn () => [empty((new ArrayObject(['a' => 0]))['a']), empty((new ArrayObject(['a' => 1]))['a'])],
    'ao write' => function () {
        $ao = new ArrayObject([5 => 'a']);
        $ao[] = 'b';
        $ao[null] = 'c';
        $ao['k'] = 'd';
        $ao->append('e');
        $ao->offsetSet(null, 'f');
        unset($ao[5]);
        $ao->offsetUnset('k');
        return $ao->getArrayCopy();
    },
    'ao unset null' => function () {
        $ao = new ArrayObject(['' => 1, 'a' => 2]);
        unset($ao[null]);
        return $ao->getArrayCopy();
    },
    'ao write array key' => function () {
        $ao = new ArrayObject([]);
        $ao[[]] = 1;
    },
    'ao unset array key' => function () {
        $ao = new ArrayObject([]);
        unset($ao[[]]);
    },
    'ao count' => fn () => [count(new ArrayObject([1, 2, 3])), (new ArrayObject([1, 2]))->count()],
    'ao iterate' => fn () => iterator_to_array(new ArrayObject(['a' => 1, 3 => 'x'])),
    'ao foreach' => function () {
        $out = [];
        foreach (new ArrayObject(['a' => 1, 3 => 'x']) as $k => $v) {
            $out[] = [$k, $v];
        }
        return $out;
    },
    'ao array cast' => fn () => (array) new ArrayObject(['a' => 1, 2 => 'b']),
    'ao object vars' => fn () => get_object_vars(new ArrayObject(['a' => 1])),
    'ao exchange' => function () {
        $ao = new ArrayObject([1]);
        $old = $ao->exchangeArray(['n' => 2]);
        return [$old, $ao->getArrayCopy()];
    },
    'ao flags' => function () {
        $ao = new ArrayObject([], ArrayObject::ARRAY_AS_PROPS);
        $f1 = $ao->getFlags();
        $ao->setFlags(1);
        return [$f1, $ao->getFlags()];
    },
    'ao as props' => function () {
        $ao = new ArrayObject(['foo' => 'bar'], ArrayObject::ARRAY_AS_PROPS);
        $r = [$ao->foo, isset($ao->foo), isset($ao->nope)];
        $ao->baz = 1;
        $r[] = $ao->getArrayCopy();
        return $r;
    },
    'ao no props' => fn () => (new ArrayObject(['foo' => 'bar']))->foo,
    'ao getIterator' => function () {
        $ao = new ArrayObject(['a' => 1]);
        $it = $ao->getIterator();
        $it['b'] = 2;
        return [get_class($it), $ao->getArrayCopy(), $ao->getIteratorClass()];
    },
    'ao clone' => function () {
        $ao = new ArrayObject(['a' => 1]);
        $c = clone $ao;
        $c['b'] = 2;
        return [$ao->getArrayCopy(), $c->getArrayCopy()];
    },
    'ao implements' => fn () => [class_implements(new ArrayObject()), class_parents(new ArrayObject())],
    'ao methods' => fn () => get_class_methods(new ArrayObject()),
    'ao constants' => fn () => [constant('ArrayObject::STD_PROP_LIST'), constant('ArrayObject::ARRAY_AS_PROPS')],
    'ao construct bad' => fn () => new ArrayObject([], 'x'),
    'ao setFlags bad' => fn () => (new ArrayObject())->setFlags('x'),
    'ao offsetGet arity' => fn () => (new ArrayObject())->offsetGet(),
    'ao getArrayCopy arity' => fn () => (new ArrayObject())->getArrayCopy(1),
    // --- ArrayIterator
    'ai iterate' => fn () => iterator_to_array(new ArrayIterator(['a' => 1, 'b' => 2])),
    'ai manual' => function () {
        $it = new ArrayIterator(['a' => 1, 'b' => 2]);
        $r = [$it->valid(), $it->key(), $it->current()];
        $it->next();
        $r[] = $it->key();
        $it->next();
        $r[] = $it->valid();
        $r[] = $it->key();
        $r[] = $it->current();
        $it->rewind();
        $r[] = $it->current();
        return $r;
    },
    'ai seek' => function () {
        $it = new ArrayIterator([10, 20, 30]);
        $it->seek(2);
        return $it->current();
    },
    'ai seek bad' => function () {
        $it = new ArrayIterator([10, 20, 30]);
        $it->seek(3);
    },
    'ai seek negative' => function () {
        $it = new ArrayIterator([10]);
        $it->seek(-1);
    },
    'ai count' => fn () => count(new ArrayIterator([1, 2])),
    'ai implements' => fn () => class_implements(new ArrayIterator([])),
    'ai methods' => fn () => get_class_methods(new ArrayIterator([])),
    'ai foreach' => function () {
        $out = [];
        foreach (new ArrayIterator(['x' => 1, 'y' => 2]) as $k => $v) {
            $out[$k] = $v;
        }
        return $out;
    },
    'ai clone shares storage' => function () {
        $it = new ArrayIterator(['a' => 1]);
        $c = clone $it;
        $c['b'] = 2;
        return $it->getArrayCopy();
    },
    // --- RecursiveArrayIterator
    'rai children' => function () {
        $it = new RecursiveArrayIterator(['a' => [1, 2], 'b' => 3, 'c' => new ArrayObject([5])]);
        $r = [];
        foreach ($it as $k => $v) {
            $r[] = [$k, $it->hasChildren()];
        }
        $it->rewind();
        $child = $it->getChildren();
        $r[] = [get_class($child), $child->getArrayCopy(), $child->getFlags()];
        $it->next();
        $it->next();
        $it->next();
        $r[] = [$it->valid(), $it->hasChildren(), $it->getChildren()];
        return $r;
    },
    'rai empty' => fn () => [(new RecursiveArrayIterator([]))->hasChildren(), (new RecursiveArrayIterator([]))->getChildren()],
    'rai child flags' => function () {
        $it = new RecursiveArrayIterator(['a' => [1]], 3);
        return [$it->getChildren()->getFlags(), $it->getFlags()];
    },
    'rai child arrays only' => function () {
        $it = new RecursiveArrayIterator(['a' => [1, 2], 'o' => new ArrayObject([5])], RecursiveArrayIterator::CHILD_ARRAYS_ONLY);
        $r = [$it->hasChildren(), $it->getChildren()->getArrayCopy(), $it->getChildren()->getFlags()];
        $it->next();
        $r[] = $it->hasChildren();
        $r[] = $it->getChildren();
        return $r;
    },
    'rai object child of its class' => function () {
        $inner = new RecursiveArrayIterator([1]);
        $it = new RecursiveArrayIterator(['i' => $inner]);
        return [$it->hasChildren(), $it->getChildren() === $inner];
    },
    'rai scalar child' => fn () => (new RecursiveArrayIterator([5]))->getChildren(),
    // the set of interfaces (PHP orders them per inheritance step, which ClassBuilder does not model)
    'rai implements sorted' => function () {
        $names = array_keys(class_implements(new RecursiveArrayIterator([])));
        sort($names);
        return $names;
    },
    'rai parents' => fn () => class_parents(new RecursiveArrayIterator([])),
    'rai methods' => fn () => get_class_methods(new RecursiveArrayIterator([])),
    'rai constant' => fn () => constant('RecursiveArrayIterator::CHILD_ARRAYS_ONLY'),
    'rai iterate' => fn () => iterator_to_array(new RecursiveArrayIterator(['a' => [1], 'b' => 2])),
    'rai debug' => fn () => get_mangled_object_vars(new RecursiveArrayIterator(['x' => 1])),
    'rai clone class' => fn () => get_class(clone new RecursiveArrayIterator([1])),
    'rai haschildren arity' => fn () => (new RecursiveArrayIterator([]))->hasChildren(1),
    'ai haschildren' => fn () => (new ArrayIterator([[1]]))->hasChildren(),
    'ao rai iterator class' => function () {
        $it = (new ArrayObject(['a' => [1]], 0, 'RecursiveArrayIterator'))->getIterator();
        return [get_class($it), $it->hasChildren(), get_class($it->getChildren())];
    },
    // --- subclasses without overrides
    'ao subclass' => function () {
        $o = new TwigValueAoSub(['k' => 'v', 'n' => [1]]);
        $c = clone $o;
        return [get_class($o), $o instanceof ArrayObject, $o['k'], count($o), get_class($o->getIterator()),
            iterator_to_array($o), get_class($c), $c == $o, $o == new ArrayObject(['k' => 'v', 'n' => [1]]),
            get_mangled_object_vars($o), class_parents($o)];
    },
    'ai subclass' => function () {
        $it = new TwigValueAiSub(['a' => 1, 'b' => 2]);
        $it->next();
        $c = clone $it;
        return [get_class($it), $it->key(), iterator_to_array($it), get_class($c), $it == new ArrayIterator(['a' => 1, 'b' => 2]),
            get_mangled_object_vars($it)];
    },
    'rai subclass children' => function () {
        $it = new TwigValueRaiSub(['a' => [1, [2]]]);
        $child = $it->getChildren();
        return [get_class($child), $child->hasChildren(), $child->getArrayCopy()];
    },
];

$rows = [];
foreach ($scenarios as $name => $f) {
    [$r, $d] = capture($f);
    $rows[] = ['name' => $name, 'result' => $r instanceof \Throwable
        ? ['err' => [get_class($r), bin2hex($r->getMessage())]]
        : ['ok' => deep($r)], 'diag' => $d];
}
emit('objects.gen.php', ['scenarios' => $rows]);

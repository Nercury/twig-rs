<?php
// Generates twig_classes.rs: every class, interface, trait and enum of Twig 3.31.1 (`TWIG/src`, all autoloadable)
// and the PHP classes they extend or implement that `twig-value` does not provide (`Exception`, `Throwable`,
// `IteratorIterator`, `OuterIterator`), as the oracle's reflection reports them: kind and modifiers, parent,
// interfaces, the constants each class declares (value and visibility), enum cases, and the methods each class
// declares (including those it takes from traits) with their modifiers. Inherited members come back through the
// parent and the interfaces when the classes are built. `src/Resources/*.php` hold functions, not classes;
// `PHPUnit\Framework\TestCase` (the parent of the two `Twig\Test` classes, a development dependency of the oracle
// image) is not modelled.
//
// Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
//
//     docker run --rm -i twig-oracle:3.31.1 php < src/extension/runtime/misc/twig_classes.gen.php \
//         | rustfmt --edition 2024 > src/extension/runtime/misc/twig_classes.rs

require '/oracle/vendor/autoload.php';

const SRC = '/oracle/vendor/twig/twig/src';
// The classes and interfaces of `twig_value::object::builtin` (`ClassTable::with_builtins()`).
const TWIG_VALUE_BUILTINS = ['Traversable', 'Iterator', 'IteratorAggregate', 'ArrayAccess', 'Countable', 'Stringable',
    'JsonSerializable', 'Serializable', 'SeekableIterator', 'UnitEnum', 'BackedEnum', 'stdClass', 'Closure',
    'ArrayObject', 'ArrayIterator'];

function fail(string $message): never
{
    fwrite(\STDERR, $message."\n");
    exit(1);
}

function rust_str(string $s): string
{
    $out = '"';
    for ($i = 0; $i < strlen($s); ++$i) {
        $c = $s[$i];
        $o = ord($c);
        if ('"' === $c || '\\' === $c) {
            $out .= '\\'.$c;
        } elseif ($o >= 0x20 && $o < 0x7F) {
            $out .= $c;
        } else {
            fail("unexpected byte in a name: $s");
        }
    }

    return $out.'"';
}

function rust_bytes(string $s): string
{
    $out = 'b"';
    for ($i = 0; $i < strlen($s); ++$i) {
        $c = $s[$i];
        $o = ord($c);
        if ('"' === $c || '\\' === $c) {
            $out .= '\\'.$c;
        } elseif ($o >= 0x20 && $o < 0x7F) {
            $out .= $c;
        } elseif ("\n" === $c) {
            $out .= '\\n';
        } elseif ("\r" === $c) {
            $out .= '\\r';
        } elseif ("\t" === $c) {
            $out .= '\\t';
        } else {
            $out .= sprintf('\\x%02x', $o);
        }
    }

    return $out.'"';
}

function rust_value(mixed $v): string
{
    if (is_array($v)) {
        $entries = [];
        foreach ($v as $key => $item) {
            $entries[] = '('.(is_int($key) ? 'K::Int('.$key.')' : 'K::Str('.rust_bytes($key).')').', '.rust_value($item).')';
        }

        return 'C::Array(&['.implode(', ', $entries).'])';
    }

    return match (true) {
        null === $v => 'C::Null',
        is_bool($v) => 'C::Bool('.($v ? 'true' : 'false').')',
        is_int($v) => \PHP_INT_MIN === $v ? 'C::Int(i64::MIN)' : 'C::Int('.$v.')',
        is_float($v) => 'C::Float(0x'.bin2hex(strrev(pack('E', $v))).')',
        is_string($v) => 'C::Str('.rust_bytes($v).')',
        default => fail('unsupported constant value '.get_debug_type($v)),
    };
}

function visibility(ReflectionClassConstant|ReflectionMethod $member): string
{
    return $member->isPublic() ? 'V::Public' : ($member->isProtected() ? 'V::Protected' : 'V::Private');
}

// Every class-like of TWIG/src.
$names = [];
$files = new RecursiveIteratorIterator(new RecursiveDirectoryIterator(SRC, FilesystemIterator::SKIP_DOTS));
foreach ($files as $file) {
    if ('php' !== $file->getExtension()) {
        continue;
    }
    $relative = substr($file->getPathname(), strlen(SRC) + 1, -4);
    if (str_starts_with($relative, 'Resources/')) {
        continue;
    }
    $name = 'Twig\\'.str_replace('/', '\\', $relative);
    if (!class_exists($name) && !interface_exists($name) && !trait_exists($name) && !enum_exists($name)) {
        fail("$name does not exist");
    }
    $names[] = $name;
}
sort($names, \SORT_STRING);

// The PHP classes the Twig classes extend or implement that twig-value lacks.
$externals = [];
foreach ($names as $name) {
    $class = new ReflectionClass($name);
    $related = $class->getInterfaceNames();
    for ($parent = $class->getParentClass(); $parent; $parent = $parent->getParentClass()) {
        $related[] = $parent->getName();
    }
    foreach ($related as $other) {
        $reflection = new ReflectionClass($other);
        if (!str_starts_with($other, 'Twig\\') && $reflection->isInternal() && !in_array($other, TWIG_VALUE_BUILTINS, true)) {
            $externals[$other] = true;
            foreach ($reflection->getInterfaceNames() as $interface) {
                if (!in_array($interface, TWIG_VALUE_BUILTINS, true)) {
                    $externals[$interface] = true;
                }
            }
        }
    }
}
$externals = array_keys($externals);
sort($externals, \SORT_STRING);
$all = array_merge($externals, $names);

// Parents and interfaces first.
$ordered = [];
$visit = function (string $name) use (&$visit, &$ordered, $all): void {
    if (isset($ordered[$name]) || !in_array($name, $all, true)) {
        return;
    }
    $class = new ReflectionClass($name);
    if ($class->getParentClass()) {
        $visit($class->getParentClass()->getName());
    }
    foreach ($class->getInterfaceNames() as $interface) {
        $visit($interface);
    }
    $ordered[$name] = true;
};
foreach ($all as $name) {
    $visit($name);
}

$rows = [];
foreach (array_keys($ordered) as $name) {
    $class = new ReflectionClass($name);
    if ($class->getName() !== $name) {
        fail("$name is an alias of ".$class->getName());
    }
    $kind = match (true) {
        $class->isEnum() => match ((string) (new ReflectionEnum($name))->getBackingType()) {
            '' => 'Kind::Enum',
            'int' => 'Kind::IntEnum',
            'string' => 'Kind::StringEnum',
        },
        $class->isInterface() => 'Kind::Interface',
        $class->isTrait() => 'Kind::Trait',
        $class->isAbstract() => 'Kind::AbstractClass',
        $class->isFinal() => 'Kind::FinalClass',
        default => 'Kind::Class',
    };
    $parent = $class->getParentClass() ? $class->getParentClass()->getName() : null;
    $interfaces = $class->getInterfaceNames();
    $constants = [];
    $cases = [];
    foreach ($class->getReflectionConstants() as $constant) {
        $declaring = $constant->getDeclaringClass()->getName();
        if ($declaring !== $name) {
            if (!in_array($declaring, $all, true)) {
                fail("$name inherits the constant {$constant->getName()} from $declaring, which is not modelled");
            }
            continue;
        }
        if ($constant->isDeprecated()) {
            fail("$name::{$constant->getName()} is deprecated, which is not modelled");
        }
        if ($constant->isEnumCase()) {
            $case = $constant->getValue();
            $cases[] = '('.rust_str($constant->getName()).', '.($case instanceof BackedEnum ? 'Some('.rust_value($case->value).')' : 'None').')';
            continue;
        }
        $constants[] = '('.rust_str($constant->getName()).', '.visibility($constant).', '.rust_value($constant->getValue()).')';
    }
    $methods = [];
    foreach ($class->getMethods() as $method) {
        if ($method->getDeclaringClass()->getName() !== $name) {
            $declaring = $method->getDeclaringClass()->getName();
            if (!in_array($declaring, $all, true) && !in_array($declaring, TWIG_VALUE_BUILTINS, true) && !str_starts_with($declaring, 'PHPUnit\\')) {
                fail("$name inherits the method {$method->getName()} from $declaring, which is not modelled");
            }
            continue;
        }
        // the enum methods ClassBuilder::enumeration() adds
        if ($class->isEnum() && in_array($method->getName(), ['cases', 'from', 'tryFrom'], true)) {
            continue;
        }
        $methods[] = ($method->isStatic() ? '^' : '').($method->isAbstract() ? '!' : '')
            .($method->isProtected() ? '#' : ($method->isPrivate() ? '-' : '')).$method->getName();
    }
    $rows[] = sprintf("    TwigClass {\n        name: %s,\n        kind: %s,\n        parent: %s,\n        interfaces: &[%s],\n        constants: &[%s],\n        cases: &[%s],\n        methods: %s,\n    },\n",
        rust_str($name), $kind, null === $parent ? 'None' : 'Some('.rust_str($parent).')',
        implode(', ', array_map('rust_str', $interfaces)), implode(', ', $constants), implode(', ', $cases),
        rust_str(implode(' ', $methods)));
}

echo "//! The classes, interfaces, traits and enums of Twig ", Twig\Environment::VERSION, " (`TWIG/src`) and the PHP ", \PHP_VERSION, " classes they extend\n";
echo "//! or implement that `twig-value` lacks, as the oracle's reflection reports them (generated by `twig_classes.gen.php`;\n";
echo "//! do not edit).\n\n";
echo "/// A constant value (floats as IEEE-754 bits).\n";
echo "#[derive(Clone, Copy, Debug)]\n";
echo "#[allow(dead_code, reason = \"the value model of PHP constants; not every kind occurs\")]\n";
echo "pub(super) enum C {\n    Null,\n    Bool(bool),\n    Int(i64),\n    Float(u64),\n    Str(&'static [u8]),\n    Array(&'static [(K, C)]),\n}\n\n";
echo "/// An array key of a [`C::Array`].\n";
echo "#[derive(Clone, Copy, Debug)]\n";
echo "pub(super) enum K {\n    Int(i64),\n    Str(&'static [u8]),\n}\n\n";
echo "/// The visibility of a constant.\n";
echo "#[derive(Clone, Copy, Debug)]\n";
echo "#[allow(dead_code, reason = \"not every visibility occurs\")]\n";
echo "pub(super) enum V {\n    Public,\n    Protected,\n    Private,\n}\n\n";
echo "/// What a [`TwigClass`] is.\n";
echo "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n";
echo "#[allow(dead_code, reason = \"not every kind occurs\")]\n";
echo "pub(super) enum Kind {\n    Class,\n    AbstractClass,\n    FinalClass,\n    Interface,\n    Trait,\n    Enum,\n    IntEnum,\n    StringEnum,\n}\n\n";
echo "/// A class: its name, kind, parent and interfaces (all of them, inherited ones included), the constants and enum cases\n";
echo "/// it declares, and the methods it declares (those of its traits included) as one string of space-separated names,\n";
echo "/// each prefixed with its modifiers: `^` static, `!` abstract, `#` protected, `-` private.\n";
echo "#[derive(Clone, Copy, Debug)]\n";
echo "pub(super) struct TwigClass {\n    pub(super) name: &'static str,\n    pub(super) kind: Kind,\n    pub(super) parent: Option<&'static str>,\n    pub(super) interfaces: &'static [&'static str],\n    pub(super) constants: &'static [(&'static str, V, C)],\n    pub(super) cases: &'static [(&'static str, Option<C>)],\n    pub(super) methods: &'static str,\n}\n\n";
echo "/// The classes, parents and interfaces first.\n";
echo "pub(super) static CLASSES: &[TwigClass] = &[\n";
echo implode('', $rows);
echo "];\n";

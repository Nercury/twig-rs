<?php

/*
 * Oracle results of the scenarios of TWIG/tests/Extension/SandboxTest.php as ported in
 * tests/upstream/extensions/sandbox_test.rs: for every scenario (keyed "<upstream test>[/<case>]") the rendered output
 * or the exception (class, getMessage(), previous class), and every deprecation raised from the creation of the
 * environment on. The upstream assertions check parts of these results (violation names, message fragments); the
 * Rust ports compare the whole results.
 *
 * The scenarios use the test's own helper classes and getEnvironment() (through a subclass). PHPUnit converts warnings
 * and notices to exceptions; so does the error handler here.
 *
 * Regenerate (from the repository root; Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/upstream/extensions/sandbox.gen.php > tests/vectors/upstream/extensions/sandbox.json
 */

require '/oracle/vendor/autoload.php';
require_once '/oracle/vendor/twig/twig/tests/Extension/SandboxTest.php';

use Twig\Environment;
use Twig\Extension\SandboxExtension;
use Twig\Extension\StringLoaderExtension;
use Twig\Loader\ArrayLoader;
use Twig\Markup;
use Twig\Node\Expression\ConstantExpression;
use Twig\Node\Node;
use Twig\Node\Nodes;
use Twig\Parser;
use Twig\Sandbox\SecurityPolicy;
use Twig\Sandbox\SourcePolicyInterface;
use Twig\Source;
use Twig\Tests\Extension\AlwaysAllowedSandboxTokenParser;
use Twig\Tests\Extension\ColumnObject;
use Twig\Tests\Extension\CountableFooObject;
use Twig\Tests\Extension\CyclicTraversableObject;
use Twig\Tests\Extension\FooObject;
use Twig\Tests\Extension\GatedSandboxTokenParser;
use Twig\Tests\Extension\MagicCallObject;
use Twig\Tests\Extension\MagicObject;
use Twig\Tests\Extension\PlainBaseObject;
use Twig\Tests\Extension\StringablePlainObject;
use Twig\Tests\Extension\StringableTraversableObject;
use Twig\TwigFilter;
use Twig\TwigFunction;
use Twig\TwigTest;

final class Probe extends \Twig\Tests\Extension\SandboxTest
{
    public function __construct()
    {
        parent::__construct('probe');
    }

    public function env(...$args): Environment
    {
        return $this->getEnvironment(...$args);
    }

    /** self::$params of a fresh setUp(). */
    public function params(): array
    {
        $this->setUp();

        return self::$params;
    }

    /** self::$templates of a fresh setUp(). */
    public function templates(): array
    {
        $this->setUp();

        return self::$templates;
    }
}

final class AlwaysSourcePolicy implements SourcePolicyInterface
{
    public function __construct(private \Closure $enable)
    {
    }

    public function enableSandbox(Source $source): bool
    {
        return ($this->enable)($source);
    }
}

function always(bool $enabled): SourcePolicyInterface
{
    return new AlwaysSourcePolicy(static fn () => $enabled);
}

$results = [];

/** Runs a scenario: its return value is the output; an exception is recorded with its class and message. */
function scenario(string $id, \Closure $fn): void
{
    global $results;
    if (isset($results[$id])) {
        throw new \LogicException("Duplicate scenario $id");
    }
    $deprecations = [];
    set_error_handler(static function (int $type, string $message) use (&$deprecations) {
        if (\E_USER_DEPRECATED === $type || \E_DEPRECATED === $type) {
            $deprecations[] = $message;

            return true;
        }
        throw new \ErrorException($message, 0, $type);
    });
    try {
        $result = ['ok' => true, 'output' => $fn()];
    } catch (\Throwable $e) {
        $result = ['ok' => false, 'class' => $e::class, 'message' => $e->getMessage(), 'previous' => $e->getPrevious() ? $e->getPrevious()::class : null];
    } finally {
        restore_error_handler();
    }
    $result['deprecations'] = $deprecations;
    $results[$id] = $result;
}

$p = new Probe();
$foo = FooObject::class;

// ---- core tags, extends and use

foreach ([
    ['apply', '{% apply upper %}foo{% endapply %}'],
    ['autoescape', '{% autoescape %}foo{% endautoescape %}'],
    ['block', '{% block foo %}foo{% endblock %}'],
    ['deprecated', '{% deprecated "message" %}'],
    ['do', '{% do 1 + 2 %}'],
    ['embed', '{% embed "base.twig" %}{% endembed %}'],
    ['flush', '{% flush %}'],
    ['for', '{% for i in 1..2 %}{% endfor %}'],
    ['from', '{% from "macros" import foo %}'],
    ['guard', '{% guard function max %}{% endguard %}'],
    ['if', '{% if false %}{% endif %}'],
    ['import', '{% import "macros" as macros %}'],
    ['include', '{% include "macros" %}'],
    ['macro', '{% macro foo() %}{% endmacro %}'],
    ['set', '{% set foo = 1 %}'],
    ['with', '{% with foo %}{% endwith %}'],
] as [$tag, $template]) {
    scenario("testSandboxForCoreTags/$tag", fn () => $p->env(true, [], $p->templates(), [])->createTemplate($template, 'index')->render([]));
}

foreach ([['extends', '{% extends "1_empty" %}'], ['use', '{% use "1_empty" %}']] as [$tag, $template]) {
    scenario("testSandboxForExtendsAndUseTags/$tag", fn () => $p->env(true, [], $p->templates(), [])->createTemplate($template, 'index')->render([]));
}

$baseAndChild = [
    'base' => '{% block content %}PARENT{% endblock %}',
    'child' => '{% extends "base" %}{% block content %}{{ parent() }} CHILD{% endblock %}',
];
foreach ([
    'attribute on array' => ['index', ['index' => '{{ attribute(data, "secret") }}'], [], [], [], ['data' => ['secret' => 'LEAK']]],
    'attribute on object property' => ['index', ['index' => '{{ attribute(obj, "bar") }}'], [], [], [$foo => ['bar']], ['obj' => new FooObject()]],
    'attribute on object method' => ['index', ['index' => '{{ attribute(obj, "foo") }}'], [], [$foo => ['foo']], [], ['obj' => new FooObject()]],
    'block from same template' => ['index', ['index' => '{% block content %}B{% endblock %}{{ block("content") }}'], ['block'], [], [], []],
    'parent inside inherited block' => ['child', $baseAndChild, ['block'], [], [], []],
] as $name => [$templateName, $templates, $tags, $methods, $properties, $context]) {
    scenario("testSandboxForParserCallableFunctions/$name", fn () => $p->env(true, [], $templates, $tags, [], $methods, $properties, [])->load($templateName)->render($context));
}

foreach ([
    'attribute allowed' => ['index', ['index' => '{{ attribute(data, "x") }}'], [], ['attribute'], ['data' => ['x' => 'OK']]],
    'block allowed' => ['index', ['index' => '{% block content %}B{% endblock %}{{ block("content") }}'], ['block'], ['block'], []],
    'parent allowed' => ['child', $baseAndChild, ['block', 'extends'], ['parent'], []],
] as $name => [$templateName, $templates, $tags, $functions, $context]) {
    scenario("testSandboxWithAllowedParserCallableFunctions/$name", fn () => $p->env(true, [], $templates, $tags, [], [], [], $functions)->load($templateName)->render($context));
}

foreach ([
    'extends' => '{% extends "1_empty" %}',
    'use' => '{% use "1_empty" %}',
    'use of a missing template is rejected before the loader is reached' => '{% use "does_not_exist" %}',
    'use of a missing block is rejected before the trait is resolved' => '{% use "1_layout" with does_not_exist as alias %}',
    'use of a non-traitable template is rejected before the trait is resolved' => '{% use "1_child" %}',
] as $name => $template) {
    scenario("testStrictSandboxRejectsGrandfatheredTags/$name", fn () => $p->env(true, [], $p->templates(), [], [], [], [], [], null, true)->createTemplate($template, 'index')->render([]));
}

foreach ([
    'attribute' => ['index', ['index' => '{{ attribute(data, "secret") }}'], [], ['data' => ['secret' => 'LEAK']]],
    'block' => ['index', ['index' => '{% block content %}B{% endblock %}{{ block("content") }}'], ['block'], []],
    'parent' => ['child', $baseAndChild, ['extends', 'block'], []],
] as $name => [$templateName, $templates, $tags, $context]) {
    scenario("testStrictSandboxRejectsGrandfatheredFunctions/$name", fn () => $p->env(true, [], $templates, $tags, [], [], [], [], null, true)->load($templateName)->render($context));
}

scenario('testStrictSandboxStillAllowsExplicitlyAllowedGrandfatheredNames', fn () => $p->env(true, [], [
    'base' => '{% block content %}PARENT{% endblock %}',
    'child' => '{% extends "base" %}{% block content %}{{ parent() }} CHILD - {{ attribute(data, "x") }}{% endblock %}',
], ['extends', 'block'], [], [], [], ['parent', 'attribute'], null, true)->load('child')->render(['data' => ['x' => 'OK']]));

scenario('testStrictModeCanBeEnabledViaSetterAfterConstruction', function () {
    $policy = new SecurityPolicy([], [], [], [], []);
    $policy->setStrict(true);
    $policy->checkSecurity(['extends'], [], [], []);

    return '';
});

foreach ([
    '{{ "" is empty ? "y" }}', '{{ [] is iterable ? "y" }}', '{{ null is null ? "y" }}', '{{ null is none ? "y" }}',
    '{{ 1 is defined ? "y" }}', '{{ 2 is even ? "y" }}', '{{ 3 is odd ? "y" }}', '{{ true is true ? "y" }}',
    '{{ 1 is same as(1) ? "y" }}', '{{ 4 is divisible by(2) ? "y" }}', '{{ [] is sequence ? "y" }}', '{{ {"a": 1} is mapping ? "y" }}',
] as $template) {
    scenario("testSandboxAllowsAlwaysAllowedCoreTests/$template", fn () => $p->env(true, [], $p->templates(), [], [], [], [], [], null, true)->createTemplate($template, 'index')->render([]));
}

scenario('testSandboxForConstantTest', fn () => $p->env(true, [], ['index' => '{{ 1 is constant("PHP_INT_MAX") ? "y" }}'])->load('index')->render([]));

scenario('testSandboxForUserDefinedTest', function () use ($p) {
    $twig = $p->env(true, [], ['index' => '{{ "x" is unsafe ? "y" }}']);
    $twig->addTest(new TwigTest('unsafe', static fn ($value): bool => true));

    return $twig->load('index')->render([]);
});

scenario('testSandboxAllowsAllowListedTest', function () use ($p) {
    $twig = $p->env(true, [], ['index' => '{{ "x" is unsafe ? "y" }}'], [], [], [], [], [], null, false, ['unsafe']);
    $twig->addTest(new TwigTest('unsafe', static fn ($v): bool => true));

    return $twig->load('index')->render([]);
});

scenario('testStrictSandboxRejectsConstantTest', fn () => $p->env(true, [], ['index' => '{{ 1 is constant("PHP_INT_MAX") ? "y" }}'], [], [], [], [], [], null, true)->load('index')->render([]));

scenario('testStrictSandboxStillAllowsAllowListedTest', fn () => $p->env(true, [], ['index' => '{{ 1 is constant("PHP_INT_MAX") ? "y" : "n" }}'], [], [], [], [], [], null, true, ['constant'])->load('index')->render([]));

scenario('testStrictSandboxRejectsUserDefinedTest', function () use ($p) {
    $twig = $p->env(true, [], ['index' => '{{ "x" is unsafe ? "y" }}'], [], [], [], [], [], null, true);
    $twig->addTest(new TwigTest('unsafe', static fn ($v): bool => true));

    return $twig->load('index')->render([]);
});

scenario('testImplicitBooleanCoercionDoesNotRequireAllowListingTests', fn () => $p->env(true, [], ['index' => "{% if x %}a{% endif %}{{ y ? 'b' : 'c' }}{{ z ?: 'd' }}{{ w ?? 'e' }}{{ v|default('f') }}"], ['if'], ['default'], [], [], [], null, true)->load('index')->render(['x' => true, 'y' => false]));

scenario('testStrictSandboxRejectedTestCarriesSourceAndLine', fn () => $p->env(true, [], ['index' => "{{ 1 }}\n{{ 1 is constant('PHP_INT_MAX') ? 'y' }}"], [], [], [], [], [], null, true)->load('index')->render([]));

scenario('testStrictModeRejectsTestsViaSetter', function () {
    $policy = new SecurityPolicy([], [], [], [], []);
    $policy->setStrict(true);
    $policy->checkSecurity([], [], [], ['empty']);

    return '';
});

scenario('testAllowedTestsCanBeUpdatedViaSetter', function () {
    $policy = new SecurityPolicy([], [], [], [], []);
    $policy->setStrict(true);
    $policy->setAllowedTests(['empty']);
    $policy->checkSecurity([], [], [], ['empty']);
    $policy->checkSecurity([], [], [], ['null']);

    return '';
});

scenario('testSandboxWithInheritance', fn () => $p->env(true, [], $p->templates(), ['extends', 'block'])->load('1_child')->render([]));
scenario('testSandboxGloballySet', fn () => $p->env(false, [], $p->templates())->load('1_basic')->render($p->params()));
scenario('testSandboxUnallowedPropertyAccessor', fn () => $p->env(true, [], $p->templates())->load('1_basic1')->render(['obj' => new MagicObject()]));
scenario('testSandboxUnallowedArrayIndexAccessor/1', fn () => $p->env(true, [], $p->templates())->load('1_array_like')->render(['array_like' => new \ArrayObject(['foo' => 'bar'])]));
scenario('testSandboxUnallowedArrayIndexAccessor/2', fn () => $p->env(true, [], $p->templates())->load('1_array_like')->render($p->params()));

foreach (['legacy' => '1_syntax_error_wrapper_legacy', 'include' => '1_syntax_error_wrapper'] as $name => $wrapper) {
    $id = 'legacy' === $name ? 'testIfSandBoxIsDisabledAfterSyntaxErrorLegacy' : 'testIfSandBoxIsDisabledAfterSyntaxError';
    scenario($id, fn () => $p->env(false, [], $p->templates())->load($wrapper)->render($p->params()));
}

foreach ([
    'testSandboxGloballyFalseUnallowedFilterWithIncludeTemplateFromStringSandboxed' => [false, [], '1_basic2_include_template_from_string_sandboxed'],
    'testSandboxGloballyTrueUnallowedFilterWithIncludeTemplateFromStringSandboxed' => [true, ['include', 'template_from_string'], '1_basic2_include_template_from_string_sandboxed'],
    'testSandboxGloballyFalseUnallowedFilterWithIncludeTemplateFromStringNotSandboxed' => [false, [], '1_basic2_include_template_from_string'],
    'testSandboxGloballyTrueUnallowedFilterWithIncludeTemplateFromStringNotSandboxed' => [true, ['include', 'template_from_string'], '1_basic2_include_template_from_string'],
] as $id => [$sandboxed, $functions, $template]) {
    scenario($id, function () use ($p, $sandboxed, $functions, $template) {
        $twig = $p->env($sandboxed, [], $p->templates(), [], [], [], [], $functions);
        $twig->addExtension(new StringLoaderExtension());

        return $twig->load($template)->render($p->params());
    });
}

scenario('testSandboxUnallowedFilter', fn () => $p->env(true, [], $p->templates())->load('1_basic2')->render($p->params()));
scenario('testSandboxUnallowedTag', fn () => $p->env(true, [], $p->templates())->load('1_basic3')->render($p->params()));
scenario('testSandboxUnallowedProperty', fn () => $p->env(true, [], $p->templates())->load('1_basic4')->render($p->params()));
scenario('testSandboxAllowedMagicCallMethod', fn () => $p->env(true, [], ['index' => '{{ o.hello }}'], [], [], [MagicCallObject::class => 'hello'])->load('index')->render(['o' => new MagicCallObject()]));
scenario('testSandboxUnallowedMagicCallMethod', fn () => $p->env(true, [], ['index' => '{{ o.hello }}'])->load('index')->render(['o' => new MagicCallObject()]));
scenario('testSandboxUnallowedMagicCallMethodWithMethodSyntax', fn () => $p->env(true, [], ['index' => '{{ o.hello() }}'])->load('index')->render(['o' => new MagicCallObject()]));

foreach ([
    'context value' => '{{ m.__construct("<script>", "UTF-8") }}',
    'mixed case' => '{{ m.__CONSTRUCT("<script>", "UTF-8") }}',
    'captured value' => '{% set c %}safe{% endset %}{{ c.__construct("<script>", "UTF-8") }}',
] as $name => $template) {
    scenario("testSandboxDoesNotAllowCallingTheMarkupConstructor/$name", fn () => $p->env(true, [], ['index' => $template], ['set'])->load('index')->render(['m' => new Markup('<b>safe</b>', 'UTF-8')]));
}

scenario('testSandboxAllowsCallingMarkupMethods', fn () => $p->env(true, [], ['index' => '{{ m.count }}-{{ m.getCharset() }}'])->load('index')->render(['m' => new Markup('<b>', 'UTF-8')]));
scenario('testSandboxAllowingCallLiteralDoesNotAllowMagicCallMethod', fn () => $p->env(true, [], ['index' => '{{ o.hello }}'], [], [], [MagicCallObject::class => '__call'])->load('index')->render(['o' => new MagicCallObject()]));
scenario('testSandboxFallsBackToMagicCallMethodForUnallowedProperty', fn () => $p->env(true, [], ['index' => '{{ o.secret }}'], [], [], [MagicCallObject::class => 'secret'])->load('index')->render(['o' => new MagicCallObject()]));

// ---- __toString

foreach (\Twig\Tests\Extension\SandboxTest::getSandboxUnallowedToStringTests() as $name => [$template]) {
    if ('recursion' === $name) {
        continue; // not portable: twig-rs arrays cannot be recursive
    }
    scenario("testSandboxUnallowedToString/$name", function () use ($p, $template, $foo) {
        $twig = $p->env(true, [], ['index' => $template], ['if', 'do', 'for', 'set'], ['upper', 'join', 'replace', 'format', 'split'], [$foo => 'getAnotherFooObject'], [], ['random', 'range', 'my_func']);
        $twig->addFunction(new TwigFunction('my_func', static fn ($a) => (string) $a));

        return $twig->load('index')->render($p->params());
    });
}

scenario('testSandboxBlocksToStringOnFunctionReturn', function () use ($p) {
    $twig = $p->env(true, [], ['index' => '{{ make_obj() }}'], [], [], [], [], ['make_obj']);
    $twig->addFunction(new TwigFunction('make_obj', static fn () => new FooObject()));

    return $twig->load('index')->render([]);
});
scenario('testSandboxBlocksToStringOnFilterReturn', function () use ($p) {
    $twig = $p->env(true, [], ['index' => '{{ "x"|to_obj }}'], [], ['to_obj']);
    $twig->addFilter(new TwigFilter('to_obj', static fn () => new FooObject()));

    return $twig->load('index')->render([]);
});
scenario('testSandboxBlocksToStringOnDynamicAttributeName', fn () => $p->env(true, ['strict_variables' => true], ['index' => '{{ arr[obj] }}'], [], [], [$foo => 'getAnotherFooObject'])->load('index')->render($p->params()));
scenario('testSandboxBlocksToStringOnDynamicMacroName', fn () => $p->env(true, [], ['index' => "{% import _self as macros %}\n{% macro foo() %}foo{% endmacro %}\n{{ macros.(obj)() }}"], ['import', 'macro'])->load('index')->render($p->params()));
scenario('testSandboxBlocksToStringOnIncludeTemplateName', fn () => $p->env(true, [], ['index' => '{% include obj %}'], ['include'])->load('index')->render($p->params()));
scenario('testSandboxBlocksToStringOnExtendsTemplateName', fn () => $p->env(true, [], ['index' => '{% extends obj %}'], ['extends'])->load('index')->render($p->params()));
scenario('testSandboxBlocksToStringOnBlockFunctionTemplateName', fn () => $p->env(true, [], ['index' => '{{ block("content", obj) }}'], [], [], [], [], ['block'])->load('index')->render($p->params()));
scenario('testSandboxBlocksToStringOnEmbedTemplateName', fn () => $p->env(true, [], ['index' => '{% embed obj %}{% endembed %}'], ['embed', 'extends'])->load('index')->render($p->params()));
scenario('testSandboxBlocksToStringOnDynamicEmbedParent', fn () => $p->env(true, [], ['index' => '{% embed obj ~ ".twig" %}{% endembed %}', 'foo.twig' => 'LEAK'], ['embed', 'extends'])->load('index')->render($p->params()));

foreach ([
    'filter' => '{% embed "LEAK"|lower %}{% endembed %}',
    'function' => '{% embed leak() %}{% endembed %}',
    'nested function' => '{% embed "" ~ leak() %}{% endembed %}',
    'test' => '{% embed 1 is leak ? "leak" : "none" %}{% endembed %}',
] as $name => $template) {
    scenario("testSandboxChecksCallablesInDynamicEmbedParent/$name", function () use ($p, $template) {
        $twig = $p->env(true, [], ['index' => $template, 'leak' => 'LEAK'], ['embed', 'extends'], [], [], [], [], null, true);
        $twig->addFunction(new TwigFunction('leak', static fn () => 'leak'));
        $twig->addTest(new TwigTest('leak', static fn () => true));

        return $twig->load('index')->render([]);
    });
}

scenario('testSandboxBlocksToStringOnIsConstantTestArgument', fn () => $p->env(true, [], ['index' => '{% if "x" is constant(obj) %}LEAK{% endif %}'], ['if'], [], [], [], [], null, false, ['constant'])->load('index')->render($p->params()));
scenario('testSandboxBlocksToStringOnDeprecatedMessage', fn () => $p->env(true, [], ['index' => '{% deprecated obj %}'], ['deprecated'])->load('index')->render($p->params()));
scenario('testSandboxKeepsSelfImportShortcut', fn () => $p->env(true, [], ['index' => "{% macro local_lower(s) %}{{ s|lower }}{% endmacro %}{% from _self import local_lower %}{{ local_lower('A') }}"], ['from', 'macro', 'import'], ['lower'])->load('index')->render([]));

foreach (\Twig\Tests\Extension\SandboxTest::getSandboxAllowedToStringTests() as $name => [$template]) {
    scenario("testSandboxAllowedToString/$name", fn () => $p->env(true, [], ['index' => $template], ['set', 'do'], [], [$foo => ['foo', 'getAnotherFooObject']], [], [], null, false, ['constant'])->load('index')->render($p->params()));
}

scenario('testSandboxAllowMethodToString', fn () => $p->env(true, [], $p->templates(), [], [], [$foo => '__toString'])->load('1_basic5')->render($p->params()));
scenario('testSandboxAllowsArrayDynamicKeyWhenToStringAllowed', fn () => $p->env(true, [], ['index' => '{% set arr = {(obj): "v", (obj.anotherFooObject): "v2"} %}{{ arr|keys|join(",") }}'], ['set'], ['join', 'keys'], [$foo => ['__toString', 'getAnotherFooObject']])->load('index')->render($p->params()));
scenario('testSandboxAllowMethodToStringDisabled', fn () => $p->env(false, [], $p->templates())->load('1_basic5')->render($p->params()));
scenario('testSandboxUnallowedFunction', fn () => $p->env(true, [], $p->templates())->load('1_basic7')->render($p->params()));
scenario('testSandboxUnallowedRangeOperator', fn () => $p->env(true, [], $p->templates())->load('1_range_operator')->render($p->params()));
scenario('testSandboxAllowMethodFoo', fn () => $p->env(true, [], $p->templates(), [], [], [$foo => 'foo'])->load('1_basic1')->render($p->params()));
scenario('testSandboxAllowFilter', fn () => $p->env(true, [], $p->templates(), [], ['upper'])->load('1_basic2')->render($p->params()));
scenario('testSandboxAllowTag', fn () => $p->env(true, [], $p->templates(), ['if'])->load('1_basic3')->render($p->params()));
scenario('testSandboxAllowProperty', fn () => $p->env(true, [], $p->templates(), [], [], [], [$foo => 'bar'])->load('1_basic4')->render($p->params()));
scenario('testSandboxAllowDestructuring', fn () => $p->env(true, [], ['index' => '{% do {bar: x, foo: y} = obj %}{{ x }}-{{ y }}'], ['do'], [], [$foo => 'foo'], [$foo => 'bar'])->load('index')->render($p->params()));
scenario('testSandboxUnallowedDestructuringProperty', fn () => $p->env(true, [], ['index' => '{% do {bar: x} = obj %}{{ x }}'], ['do'])->load('index')->render($p->params()));
scenario('testSandboxUnallowedDestructuringMethod', fn () => $p->env(true, [], ['index' => '{% do {foo: y} = obj %}{{ y }}'], ['do'], [], [], [$foo => 'foo'])->load('index')->render($p->params()));
scenario('testSandboxAllowFunction', fn () => $p->env(true, [], $p->templates(), [], [], [], [], ['cycle'])->load('1_basic7')->render($p->params()));
scenario('testSandboxAllowRangeOperator', fn () => $p->env(true, [], $p->templates(), [], [], [], [], ['range'])->load('1_range_operator')->render($p->params()));

foreach (['getfoobar', 'getFoobar', 'getFooBar'] as $name) {
    foreach (['1_basic8', '1_basic9'] as $template) {
        scenario("testSandboxAllowMethodsCaseInsensitive/$name/$template", fn () => $p->env(true, [], $p->templates(), [], [], [$foo => $name])->load($template)->render($p->params()));
    }
}

scenario('testSandboxLocallySetForAnInclude/1', fn () => $p->env(false, [], [
    '2_basic' => '{{ obj.foo }}{% include "2_included" %}{{ obj.foo }}',
    '2_included' => '{% if obj.foo %}{{ obj.foo|upper }}{% endif %}',
])->load('2_basic')->render($p->params()));
scenario('testSandboxLocallySetForAnInclude/2', fn () => $p->env(true, [], [
    '3_basic' => '{{ include("3_included", sandboxed: true) }}',
    '3_included' => '{% if true %}{{ "foo"|upper }}{% endif %}',
], functions: ['include'])->load('3_basic')->render($p->params()));

scenario('testMacrosInASandbox', fn () => $p->env(true, ['autoescape' => 'html'], ['index' => "{%- import _self as macros %}\n\n{%- macro test(text) %}<p>{{ text }}</p>{% endmacro %}\n\n{{- macros.test('username') }}"], ['macro', 'import'], ['escape'])->load('index')->render([]));
scenario('testSelfMacroReferenceWithStringLiteralDoesNotInjectPhp', fn () => $p->env(true, [], ['index' => '{{ _self.(\'foo + 1; trigger_error("BAD-MACRO-REF") //\')() }}'])->load('index')->render([]));
scenario('testImportedTemplateMacroReferenceWithBadIdentifierDoesNotInjectPhp', fn () => $p->env(true, [], [
    'index' => '{% import "m" as m %}{{ m.(\'foo + 1; trigger_error("BAD-IMPORT-REF") //\')() }}',
    'm' => '{% macro greet() %}hi{% endmacro %}',
], ['import'])->load('index')->render([]));
scenario('testSelfMacroReferenceWithValidIdentifierStillWorks', fn () => $p->env(true, ['autoescape' => 'html'], ['index' => "{%- macro greet(n) %}Hi {{ n }}{% endmacro %}\n{{- _self.('greet')('World') }}"], ['macro'], ['escape'])->load('index')->render([]));
scenario('testSandboxDisabledAfterIncludeFunctionError', fn () => $p->env(false, [], $p->templates())->load('1_include')->render($p->params()));

foreach ([
    'testSandboxTagIncludeWithPreloadedTemplate' => '{% sandbox %}{% include "included" %}{% endsandbox %}',
    'testSandboxTagIncludeIgnoreMissingWithPreloadedTemplate' => '{% sandbox %}{% include "included" ignore missing %}{% endsandbox %}',
] as $id => $index) {
    scenario($id, function () use ($p, $index) {
        $twig = $p->env(false, [], ['index' => $index, 'included' => '{{ "hello"|upper }}']);
        $twig->load('included');

        return $twig->load('index')->render([]);
    });
}

scenario('testSandboxWithNoClosureFilter', fn () => $p->env(true, ['autoescape' => 'html'], ['index' => '{{ ["foo", "bar", ""]|filter("trim")|join(", ") }}'], [], ['escape', 'filter', 'join'])->load('index')->render([]));
scenario('testSandboxWithClosureFilter', fn () => $p->env(true, ['autoescape' => 'html'], ['index' => '{{ ["foo", "bar", ""]|filter(v => v != "")|join(", ") }}'], [], ['escape', 'filter', 'join'])->load('index')->render([]));

$child = 'Twig\Tests\Extension\ChildClass';
$parent = 'Twig\Tests\Extension\ParentClass';
foreach ([
    1 => [[$child => ['ChildMethod'], $parent => ['ParentMethod']], '1_childobj_childmethod'],
    2 => [[$parent => ['ParentMethod'], $child => ['ChildMethod']], '1_childobj_parentmethod'],
    3 => [[$parent => ['ParentMethod'], $child => ['ChildMethod']], '1_childobj_childmethod'],
    4 => [[$child => ['ChildMethod'], $parent => ['ParentMethod']], '1_childobj_parentmethod'],
] as $n => [$methods, $template]) {
    scenario("testMultipleClassMatchesViaInheritanceInAllowedMethods/$n", fn () => $p->env(true, [], $p->templates(), [], [], $methods)->load($template)->render($p->params()));
}

// ---- column

$columnBar = "{{ [obj]|column('bar')|first }}";
$columnBarFoo = "{{ [obj]|column('bar', 'foo')|keys|first }}";
$columnMagic = "{{ [magic]|column('anything')|first }}";
scenario('testSandboxAllowsColumnFilterOnAllowedProperty', fn () => $p->env(true, [], ['index' => $columnBar], [], ['column', 'first'], [], [ColumnObject::class => ['bar']])->load('index')->render(['obj' => new ColumnObject()]));
scenario('testSandboxBlocksColumnFilterOnDisallowedProperty', fn () => $p->env(true, [], ['index' => $columnBar], [], ['column', 'first'])->load('index')->render(['obj' => new ColumnObject()]));
scenario('testSandboxBlocksColumnFilterOnDisallowedIndex', fn () => $p->env(true, [], ['index' => $columnBarFoo], [], ['column', 'first', 'keys'], [], [ColumnObject::class => ['bar']])->load('index')->render(['obj' => new ColumnObject()]));
scenario('testSandboxBlocksColumnFilterOnMagicGetter', fn () => $p->env(true, [], ['index' => $columnMagic], [], ['column', 'first'])->load('index')->render(['magic' => new MagicObject()]));

// ---- Stringable + Traversable

$sto = StringableTraversableObject::class;
foreach (['join' => '{{ stringable_iterator|join(", ") }}', 'replace' => '{{ "__toString"|replace(stringable_iterator_map) }}'] as $name => $template) {
    scenario("testSandboxBlocksToStringInStringableTraversable/$name", fn () => $p->env(true, [], ['index' => $template], [], ['join', 'replace'], [$sto => ['__tostring']])->load('index')->render($p->params()));
    scenario("testSourcePolicySandboxBlocksToStringInStringableTraversable/$name", fn () => $p->env(false, [], ['index' => $template], [], ['join', 'replace'], [$sto => ['__tostring']], [], [], always(true))->load('index')->render($p->params()));
}
scenario('testSandboxAllowsPrintingStringableTraversableWhenToStringAllowed', fn () => $p->env(true, ['autoescape' => 'html'], ['index' => '{{ obj }}'], [], ['escape'], [$sto => ['__tostring']])->load('index')->render(['obj' => new StringableTraversableObject(['a', 'b'])]));
foreach (['join' => '{{ obj|join(",") }}', 'replace' => '{{ "x"|replace(obj) }}', 'spread' => '{{ ["a", ...obj]|join(",") }}'] as $name => $template) {
    scenario("testSandboxHandlesCyclicTraversableWithoutStackOverflow/$name", fn () => $p->env(true, [], ['index' => $template], [], ['join', 'replace'])->load('index')->render(['obj' => new CyclicTraversableObject()]));
}
scenario('testSourcePolicySandboxBlocksToStringInTraversableJoin', fn () => $p->env(false, [], ['index' => '{{ iterator|join(", ") }}'], [], ['join'], [], [], [], always(true))->load('index')->render($p->params()));
scenario('testSourcePolicySandboxBlocksToStringInTraversableReplace', fn () => $p->env(false, [], ['index' => '{{ "__toString"|replace(iterator_map) }}'], [], ['replace'], [], [], [], always(true))->load('index')->render($p->params()));

scenario('testSandboxPreservesTraversableArgumentIdentity', function () use ($p, $sto) {
    $twig = $p->env(true, [], ['index' => '{{ render_traversable(obj) }}'], [], [], [$sto => ['__tostring']]);
    $twig->addFunction(new TwigFunction('render_traversable', static function ($obj) {
        if (!$obj instanceof StringableTraversableObject) {
            throw new \RuntimeException(\sprintf('Expected a StringableTraversableObject, got "%s".', get_debug_type($obj)));
        }

        return (string) $obj;
    }));
    $twig->getExtension(SandboxExtension::class)->getSecurityPolicy()->setAllowedFunctions(['render_traversable']);

    return $twig->load('index')->render(['obj' => new StringableTraversableObject(['a', 'b'])]);
});
scenario('testSandboxStillBlocksDisallowedToStringInTraversableArgument', function () use ($p, $sto) {
    $twig = $p->env(true, [], ['index' => '{{ render_traversable(stringable_iterator) }}'], [], [], [$sto => ['__tostring']]);
    $twig->addFunction(new TwigFunction('render_traversable', static fn ($obj) => (string) $obj));
    $twig->getExtension(SandboxExtension::class)->getSecurityPolicy()->setAllowedFunctions(['render_traversable']);

    return $twig->load('index')->render($p->params());
});

// ---- PHP parameter types

/** getEnvironment(true, [], ['index' => $template]) with $callable registered and allowed. */
function with_function(Probe $p, string $template, string $name, callable $callable, array $context, array $options = []): string
{
    $twig = $p->env(true, [], ['index' => $template]);
    $twig->addFunction(new TwigFunction($name, $callable, $options));
    $twig->getExtension(SandboxExtension::class)->getSecurityPolicy()->setAllowedFunctions([$name]);

    return $twig->load('index')->render($context);
}

foreach ([
    'int param' => ['{{ safe_fn(n) }}', static fn (int $n) => 'ok', ['n' => 42]],
    'float param' => ['{{ safe_fn(n) }}', static fn (float $n) => 'ok', ['n' => 3.14]],
    'bool param' => ['{{ safe_fn(b) }}', static fn (bool $b) => 'ok', ['b' => true]],
    'non-stringable class param' => ['{{ safe_fn(obj) }}', static fn (ColumnObject $o) => 'ok', ['obj' => new ColumnObject()]],
    'nullable int param with null value' => ['{{ safe_fn(n) }}', static fn (?int $n) => 'ok', ['n' => null]],
    'int|float union param' => ['{{ safe_fn(n) }}', static fn (int|float $n) => 'ok', ['n' => 7]],
] as $name => [$template, $func, $context]) {
    scenario("testSafePhpParamTypesSkipToStringWrap/$name", fn () => with_function($p, $template, 'safe_fn', $func, $context));
}
foreach ([
    'untyped param' => static fn ($x) => (string) $x,
    'mixed param' => static fn (mixed $x) => (string) $x,
    'string param' => static fn (string $x) => $x,
    'object param' => static fn (object $x) => (string) $x,
    'Stringable param' => static fn (\Stringable $x) => (string) $x,
] as $name => $func) {
    scenario("testUnsafePhpParamTypesStillWrap/$name", fn () => with_function($p, '{{ unsafe_fn(obj) }}', 'unsafe_fn', $func, ['obj' => new FooObject()]));
}
foreach ([
    'interface param' => [static fn (\Countable $x) => (string) $x, new CountableFooObject()],
    'non-final class param' => [static fn (PlainBaseObject $x) => (string) $x, new StringablePlainObject()],
] as $name => [$func, $obj]) {
    scenario("testOpenPhpParamTypesStillWrap/$name", fn () => with_function($p, '{{ unsafe_fn(obj) }}', 'unsafe_fn', $func, ['obj' => $obj]));
}
scenario('testTestArgumentsMapAfterTheTestedValueParameter', function () use ($p) {
    $twig = $p->env(true, [], ['index' => '{{ 5 is my_test(obj) }}']);
    $twig->addTest(new TwigTest('my_test', static fn (int $value, $arg) => 'x' === (string) $arg));
    $twig->getExtension(SandboxExtension::class)->getSecurityPolicy()->setAllowedTests(['my_test']);

    return $twig->load('index')->render(['obj' => new FooObject()]);
});
scenario('testSafeVariadicPhpTypeSkipsToStringWrap', fn () => with_function($p, '{{ safe_fn(1, 2, 3) }}', 'safe_fn', static fn (int ...$x) => 'ok', []));
scenario('testSpreadIntoUnsafeVariadicStillWraps', fn () => with_function($p, '{{ unsafe_fn(...args) }}', 'unsafe_fn', static fn (...$x) => (string) $x[0], ['args' => [new FooObject()]]));
foreach ([
    'safe first param, string variadic' => ['{{ mixed_fn(...[1, obj]) }}', static fn (int $a, string ...$rest) => $rest[0]],
    'safe first param, untyped variadic' => ['{{ mixed_fn(...[1, obj]) }}', static fn (int $a, ...$rest) => (string) $rest[0]],
    'safe first param, optional untyped param' => ['{{ mixed_fn(...[1, obj]) }}', static fn (int $a, $b = null) => (string) $b],
    'spread of a variable' => ['{{ mixed_fn(...args) }}', static fn (int $a, string ...$rest) => $rest[0]],
    'spread of a mapping' => ['{{ mixed_fn(...{b: obj}) }}', static fn (int $a = 0, string $b = '') => $b],
    'after a positional argument' => ['{{ mixed_fn(1, ...[2, obj]) }}', static fn (int $a, int $b, string ...$rest) => $rest[0]],
    'safe variadic after an unsafe param' => ['{{ mixed_fn(...[obj, 1]) }}', static fn ($a, int ...$rest) => (string) $a],
] as $name => [$template, $func]) {
    scenario("testSpreadReachingUnsafeParamStillWraps/$name", fn () => with_function($p, $template, 'mixed_fn', $func, ['obj' => new FooObject(), 'args' => [1, new FooObject()]]));
}
scenario('testNormalizedNamedArgumentDoesNotFallThroughToSafeVariadic', fn () => with_function($p, '{{ unsafe_fn(foo_bar: obj) }}', 'unsafe_fn', static fn ($fooBar, int ...$rest) => (string) $fooBar, ['obj' => new FooObject()], ['is_variadic' => true]));
scenario('testNormalizedNamedFilterArgumentDoesNotFallThroughToSafeVariadic', function () use ($p) {
    $twig = $p->env(true, [], ['index' => '{{ 1|unsafe_filter(foo_bar: obj) }}']);
    $twig->addFilter(new TwigFilter('unsafe_filter', static fn ($value, $fooBar, int ...$rest) => (string) $fooBar, ['is_variadic' => true]));
    $twig->getExtension(SandboxExtension::class)->getSecurityPolicy()->setAllowedFilters(['unsafe_filter']);

    return $twig->load('index')->render(['obj' => new FooObject()]);
});
scenario('testNormalizedNamedTestArgumentDoesNotFallThroughToSafeVariadic', function () use ($p) {
    $twig = $p->env(true, [], ['index' => '{{ 1 is unsafe_test(foo_bar: obj) ? "yes" : "no" }}']);
    $twig->addTest(new TwigTest('unsafe_test', static fn ($value, $fooBar, int ...$rest) => 'x' === (string) $fooBar, ['is_variadic' => true]));
    $twig->getExtension(SandboxExtension::class)->getSecurityPolicy()->setAllowedTests(['unsafe_test']);

    return $twig->load('index')->render(['obj' => new FooObject()]);
});
scenario('testFilterInputTypeSkipsToStringWrap', function () use ($p) {
    $twig = $p->env(true, [], ['index' => '{{ n|safe_filter }}']);
    $twig->addFilter(new TwigFilter('safe_filter', static fn (int $n) => 'ok'));
    $twig->getExtension(SandboxExtension::class)->getSecurityPolicy()->setAllowedFilters(['safe_filter']);

    return $twig->load('index')->render(['n' => 42]);
});

/** getEnvironment(true, [], ['index' => $template], tests: $allowed, ...) with $tests registered. */
function with_tests(Probe $p, string $template, array $allowed, array $tests, array $context, array $methods = []): string
{
    $twig = $p->env(true, [], ['index' => $template], methods: $methods, tests: $allowed);
    foreach ($tests as $test) {
        $twig->addTest($test);
    }

    return $twig->load('index')->render($context);
}

foreach ([
    'string parameter' => static fn (string $value): bool => 'foo' === $value,
    'explicit cast' => static fn ($value): bool => 'foo' === (string) $value,
] as $name => $callable) {
    scenario("testSandboxBlocksToStringOnTestInput/$name", fn () => with_tests($p, '{{ obj is unsafe_test ? "yes" : "no" }}', ['unsafe_test'], [new TwigTest('unsafe_test', $callable)], ['obj' => new FooObject()]));
}
scenario('testSafeTestInputTypeSkipsToStringWrap', fn () => with_tests($p, '{{ obj is safe_test ? "yes" : "no" }}', ['safe_test'], [new TwigTest('safe_test', static fn (int $value): bool => true)], ['obj' => new FooObject()]));
scenario('testDynamicTestInputMapsAfterBoundArguments', fn () => with_tests($p, '{{ obj is unsafe_test_name ? "yes" : "no" }}', ['unsafe_test_name'], [new TwigTest('unsafe_test_*', static fn (string $name, string $value): bool => 'foo' === $value)], ['obj' => new FooObject()]));
scenario('testSafeDynamicTestInputTypeSkipsToStringWrap', fn () => with_tests($p, '{{ obj is safe_test_name ? "yes" : "no" }}', ['safe_test_name'], [new TwigTest('safe_test_*', static fn (string $name, int $value): bool => true)], ['obj' => new FooObject()]));
scenario('testSandboxAllowsToStringOnTestInputWhenMethodIsAllowed', fn () => with_tests($p, '{{ obj is safe_test ? "yes" : "no" }}', ['safe_test'], [new TwigTest('safe_test', static fn (string $value): bool => 'foo' === $value)], ['obj' => new FooObject()], [FooObject::class => ['__toString']]));
scenario('testSandboxBlocksToStringOnTestReturnUsedAsTestInput', fn () => with_tests($p, '{{ (obj is returns_object) is unsafe_test ? "yes" : "no" }}', ['returns_object', 'unsafe_test'], [new TwigTest('returns_object', static fn ($value) => new FooObject()), new TwigTest('unsafe_test', static fn (string $value): bool => 'foo' === $value)], ['obj' => new FooObject()]));
scenario('testSafeTestInputTypeDoesNotWrapNestedTestReturn', fn () => with_tests($p, '{{ (1 is returns_object) is safe_test ? "yes" : "no" }}', ['returns_object', 'safe_test'], [new TwigTest('returns_object', static fn ($value) => new FooObject()), new TwigTest('safe_test', static fn (int $value): bool => true)], []));

foreach ([
    'direct output' => ['{{ obj is true }}', []],
    'concatenation' => ['{{ (obj is true) ~ "" }}', []],
    'filter input' => ['{{ (obj is true)|upper }}', ['upper']],
] as $name => [$template, $filters]) {
    scenario("testSandboxBlocksToStringOnTrueTestResult/$name", fn () => $p->env(true, [], ['index' => $template], filters: $filters)->load('index')->render(['obj' => new FooObject()]));
}
scenario('testSandboxAllowsTrueTestInBooleanContextWithoutToString', fn () => $p->env(true, [], ['index' => '{{ obj is true ? "yes" : "no" }}'])->load('index')->render(['obj' => new FooObject()]));
foreach (['iterable', 'sequence', 'mapping'] as $test) {
    scenario("testSandboxAllowsNonStringCoercingBuiltInTestInput/$test", fn () => $p->env(true, [], ['index' => '{{ obj is '.$test.' ? "yes" : "no" }}'])->load('index')->render(['obj' => new FooObject()]));
}
scenario('testSourcePolicySandboxBlocksToStringOnTestInput', function () use ($p) {
    $twig = $p->env(false, [], ['index' => '{{ obj is unsafe_test ? "yes" : "no" }}'], sourcePolicy: always(true), tests: ['unsafe_test']);
    $twig->addTest(new TwigTest('unsafe_test', static fn (string $value): bool => 'foo' === $value));

    return $twig->load('index')->render(['obj' => new FooObject()]);
});

scenario('testColumnFilterUnaffectedOutsideSandbox', fn () => $p->env(false, [], ['index' => $columnBar])->load('index')->render(['obj' => new ColumnObject()]));
scenario('testSourcePolicySandboxBlocksColumnFilterOnDisallowedProperty', fn () => $p->env(false, [], ['index' => $columnBar], [], ['column', 'first'], [], [], [], always(true))->load('index')->render(['obj' => new ColumnObject()]));
scenario('testSourcePolicySandboxBlocksColumnFilterOnDisallowedIndex', fn () => $p->env(false, [], ['index' => $columnBarFoo], [], ['column', 'first', 'keys'], [], [ColumnObject::class => ['bar']], [], always(true))->load('index')->render(['obj' => new ColumnObject()]));
scenario('testSourcePolicySandboxAllowsColumnFilterOnAllowedProperty', fn () => $p->env(false, [], ['index' => $columnBar], [], ['column', 'first'], [], [ColumnObject::class => ['bar']], [], always(true))->load('index')->render(['obj' => new ColumnObject()]));
scenario('testSourcePolicySandboxBlocksColumnFilterOnMagicGetter', fn () => $p->env(false, [], ['index' => $columnMagic], [], ['column', 'first'], [], [], [], always(true))->load('index')->render(['magic' => new MagicObject()]));

scenario('testSandboxSourcePolicyEnableReturningFalse', fn () => $p->env(false, [], $p->templates(), [], [], [], [], [], new AlwaysSourcePolicy(static fn (Source $source) => '1_basic' != $source->getName()))->load('1_basic')->render($p->params()));
scenario('testSandboxSourcePolicyEnableReturningTrue', fn () => $p->env(false, [], $p->templates(), [], [], [], [], [], new AlwaysSourcePolicy(static fn (Source $source) => '1_basic' === $source->getName()))->load('1_basic')->render([]));
scenario('testSandboxSourcePolicyFalseDoesntOverrideOtherEnables', fn () => $p->env(true, [], $p->templates(), [], [], [], [], [], always(false))->load('1_basic')->render([]));

foreach (\Twig\Tests\Extension\SandboxTest::provideSourcePolicyArrowBlockedTemplates() as $name => [$template]) {
    scenario("testSourcePolicyBlocksNonClosureCallableInArrow/$name", fn () => $p->env(false, [], ['1_basic' => $template], [], ['sort', 'filter', 'map', 'reduce', 'find', 'join'], [], [], [], always(true))->load('1_basic')->render([]));
}
scenario('testSourcePolicyAllowsClosureInArrow', fn () => $p->env(false, [], ['1_basic' => '{{ ["b","a"]|sort((a, b) => a < b ? -1 : 1)|join(",") }}'], [], ['sort', 'join'], [], [], [], always(true))->load('1_basic')->render([]));

// ---- needs_is_sandboxed

$sandboxAware = static fn (bool $isSandboxed, string $value) => $value.':'.($isSandboxed ? 'on' : 'off');
scenario('testNeedsIsSandboxedFilterReceivesTrueWhenSandboxed', function () use ($p, $sandboxAware) {
    $twig = $p->env(true, [], ['index' => '{{ "foo"|sandbox_aware }}'], [], ['sandbox_aware']);
    $twig->addFilter(new TwigFilter('sandbox_aware', $sandboxAware, ['needs_is_sandboxed' => true]));

    return $twig->load('index')->render([]);
});
scenario('testNeedsIsSandboxedFilterReceivesFalseWhenNotSandboxed', function () use ($p, $sandboxAware) {
    $twig = $p->env(false, [], ['index' => '{{ "foo"|sandbox_aware }}']);
    $twig->addFilter(new TwigFilter('sandbox_aware', $sandboxAware, ['needs_is_sandboxed' => true]));

    return $twig->load('index')->render([]);
});
foreach (['in', 'out'] as $name) {
    scenario("testNeedsIsSandboxedFilterFollowsSourcePolicy/$name", function () use ($p, $sandboxAware, $name) {
        $twig = $p->env(false, [], ['in' => '{{ "foo"|sandbox_aware }}', 'out' => '{{ "foo"|sandbox_aware }}'], [], ['sandbox_aware'], [], [], [], new AlwaysSourcePolicy(static fn (Source $source) => 'in' === $source->getName()));
        $twig->addFilter(new TwigFilter('sandbox_aware', $sandboxAware, ['needs_is_sandboxed' => true]));

        return $twig->load($name)->render([]);
    });
}
scenario('testNeedsIsSandboxedFunctionWithoutSandboxExtension', function () use ($sandboxAware) {
    $twig = new Environment(new ArrayLoader(['index' => '{{ sandbox_aware("foo") }}']), ['debug' => true, 'cache' => false, 'autoescape' => false]);
    $twig->addFunction(new TwigFunction('sandbox_aware', $sandboxAware, ['needs_is_sandboxed' => true]));

    return $twig->load('index')->render([]);
});
scenario('testNeedsIsSandboxedTestReceivesTrueWhenSandboxed', function () use ($p) {
    $twig = $p->env(true, [], ['index' => '{{ "foo" is sandbox_aware ? "on" : "off" }}'], [], [], [], [], [], null, false, ['sandbox_aware']);
    $twig->addTest(new TwigTest('sandbox_aware', static fn (bool $isSandboxed, string $value) => $isSandboxed && 'foo' === $value, ['needs_is_sandboxed' => true]));

    return $twig->load('index')->render([]);
});
scenario('testNeedsIsSandboxedTestReceivesFalseWhenNotSandboxed', function () use ($p) {
    $twig = $p->env(false, [], ['index' => '{{ "foo" is sandbox_aware ? "on" : "off" }}']);
    $twig->addTest(new TwigTest('sandbox_aware', static fn (bool $isSandboxed, string $value) => !$isSandboxed && 'foo' === $value, ['needs_is_sandboxed' => true]));

    return $twig->load('index')->render([]);
});

// ---- always_allowed_in_sandbox

/** getEnvironment(true, [], ['index' => $template], ..., strict: $strict) after $register($twig). */
function with_env(Probe $p, string $template, \Closure $register, bool $strict = false, array $context = []): string
{
    $twig = $p->env(true, [], ['index' => $template], [], [], [], [], [], null, $strict);
    $register($twig);

    return $twig->load('index')->render($context);
}

scenario('testAlwaysAllowedInSandboxFilterBypassesAllowList', fn () => with_env($p, '{{ "fabien"|safe_upper }}', fn ($t) => $t->addFilter(new TwigFilter('safe_upper', 'strtoupper', ['always_allowed_in_sandbox' => true]))));
scenario('testAlwaysAllowedInSandboxFilterStillEnforcedWhenFlagNotSet', fn () => with_env($p, '{{ "fabien"|gated_upper }}', fn ($t) => $t->addFilter(new TwigFilter('gated_upper', 'strtoupper'))));
scenario('testAlwaysAllowedInSandboxFunctionBypassesAllowList', fn () => with_env($p, '{{ safe_greet("fabien") }}', fn ($t) => $t->addFunction(new TwigFunction('safe_greet', static fn (string $name) => "hi $name", ['always_allowed_in_sandbox' => true]))));
scenario('testAlwaysAllowedInSandboxFunctionStillEnforcedWhenFlagNotSet', fn () => with_env($p, '{{ gated_greet("fabien") }}', fn ($t) => $t->addFunction(new TwigFunction('gated_greet', static fn (string $name) => "hi $name"))));
scenario('testAlwaysAllowedInSandboxTestBypassesAllowList', fn () => with_env($p, '{{ 4 is safe_even ? "yes" : "no" }}', fn ($t) => $t->addTest(new TwigTest('safe_even', static fn ($value) => 0 === $value % 2, ['always_allowed_in_sandbox' => true])), true));
scenario('testAlwaysAllowedInSandboxTestStillEnforcedWhenFlagNotSet', fn () => with_env($p, '{{ 4 is gated_even ? "yes" : "no" }}', fn ($t) => $t->addTest(new TwigTest('gated_even', static fn ($value) => 0 === $value % 2)), true));
scenario('testAlwaysAllowedInSandboxFunctionAlsoCoversRangeOperator', fn () => with_env($p, '{{ (1..2)[0] }}', fn ($t) => $t->addFunction(new TwigFunction('range', 'range', ['always_allowed_in_sandbox' => true]))));
scenario('testAlwaysAllowedInSandboxFilterFromUndefinedCallbackUsesParsedCallable', fn () => with_env($p, '{{ "fabien"|callback_upper }}', function ($t) {
    $called = false;
    $t->registerUndefinedFilterCallback(static function (string $name) use (&$called) {
        if ($called || 'callback_upper' !== $name) {
            return false;
        }
        $called = true;

        return new TwigFilter('callback_upper', 'strtoupper', ['always_allowed_in_sandbox' => true]);
    });
}));
scenario('testAlwaysAllowedInSandboxFunctionFromUndefinedCallbackUsesParsedCallable', fn () => with_env($p, '{{ callback_upper("fabien") }}', function ($t) {
    $called = false;
    $t->registerUndefinedFunctionCallback(static function (string $name) use (&$called) {
        if ($called || 'callback_upper' !== $name) {
            return false;
        }
        $called = true;

        return new TwigFunction('callback_upper', 'strtoupper', ['always_allowed_in_sandbox' => true]);
    });
}));
scenario('testAlwaysAllowedInSandboxParserCallableFunctionFromUndefinedCallbackUsesParsedCallable', fn () => with_env($p, '{{ callback_literal() }}', function ($t) {
    $called = false;
    $t->registerUndefinedFunctionCallback(static function (string $name) use (&$called) {
        if ($called || 'callback_literal' !== $name) {
            return false;
        }
        $called = true;

        return new TwigFunction('callback_literal', null, [
            'always_allowed_in_sandbox' => true,
            'parser_callable' => static fn (Parser $parser, Node $node, Nodes $arguments, int $line): ConstantExpression => new ConstantExpression('literal', $line),
        ]);
    });
}));
scenario('testAlwaysAllowedInSandboxTagBypassesAllowList', fn () => with_env($p, '{% always_allowed_tag %}', fn ($t) => $t->addTokenParser(new AlwaysAllowedSandboxTokenParser())));
scenario('testAlwaysAllowedInSandboxTagStillEnforcedWhenFlagNotSet', fn () => with_env($p, '{% gated_tag %}', fn ($t) => $t->addTokenParser(new GatedSandboxTokenParser())));
scenario('testAlwaysAllowedInSandboxFilterStillEnforcesToStringPolicyOnArguments', fn () => with_env($p, '{{ obj|safe_upper }}', fn ($t) => $t->addFilter(new TwigFilter('safe_upper', static fn (string $s) => strtoupper($s), ['always_allowed_in_sandbox' => true])), false, ['obj' => new FooObject()]));
scenario('testAlwaysAllowedInSandboxFunctionStillEnforcesToStringPolicyOnArguments', fn () => with_env($p, '{{ safe_greet(obj) }}', fn ($t) => $t->addFunction(new TwigFunction('safe_greet', static fn (string $s) => "hi $s", ['always_allowed_in_sandbox' => true])), false, ['obj' => new FooObject()]));
scenario('testAlwaysAllowedInSandboxTestStillEnforcesToStringPolicyOnInput', fn () => with_env($p, '{{ obj is always_safe_test ? "yes" : "no" }}', fn ($t) => $t->addTest(new TwigTest('always_safe_test', static fn (string $value): bool => 'foo' === $value, ['always_allowed_in_sandbox' => true])), false, ['obj' => new FooObject()]));

ksort($results);
echo json_encode($results, \JSON_PRETTY_PRINT | \JSON_UNESCAPED_SLASHES | \JSON_UNESCAPED_UNICODE | \JSON_INVALID_UTF8_SUBSTITUTE | \JSON_PRESERVE_ZERO_FRACTION), "\n";

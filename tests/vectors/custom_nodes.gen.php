<?php

/*
 * Generator of tests/vectors/custom_nodes.json: node classes defined by an extension, rendered by Twig 3.31.1. The PHP
 * classes below are the Twig counterparts of the node classes of tests/custom_nodes.rs (same tags, same operators,
 * same callables, same compiled behaviour), which renders the same cases and compares outputs, errors and
 * deprecations.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/custom_nodes.gen.php > tests/vectors/custom_nodes.json
 *
 * Output: [{"name", "templates", "context", "objects", "strict_variables", "sandbox": null|{"tags", "filters",
 *           "functions", "tests"}, "output": "..."|null, "error": null|{"class", "message"}, "deprecations": [...]}]
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Attribute\YieldReady;
use Twig\Compiler;
use Twig\Environment;
use Twig\Error\RuntimeError;
use Twig\ExpressionParser\Infix\BinaryOperatorExpressionParser;
use Twig\ExpressionParser\Prefix\UnaryOperatorExpressionParser;
use Twig\Extension\AbstractExtension;
use Twig\Extension\SandboxExtension;
use Twig\Loader\ArrayLoader;
use Twig\Node\CaptureNode;
use Twig\Node\CoercesChildrenToStringInterface;
use Twig\Node\Expression\AbstractExpression;
use Twig\Error\SyntaxError;
use Twig\Node\Expression\Binary\AbstractBinary;
use Twig\Node\Expression\ConstantExpression;
use Twig\Node\Expression\FilterExpression;
use Twig\Node\Expression\FunctionExpression;
use Twig\Node\Expression\OperatorEscapeInterface;
use Twig\Node\Expression\TestExpression;
use Twig\Node\Expression\Unary\AbstractUnary;
use Twig\Node\Node;
use Twig\Sandbox\SecurityPolicy;
use Twig\Token;
use Twig\TokenParser\AbstractTokenParser;
use Twig\TwigFilter;
use Twig\TwigFunction;
use Twig\TwigTest;

// ---- {% unless cond %}...{% else %}...{% endunless %} ------------------------------------------------------------

#[YieldReady]
final class UnlessNode extends Node
{
    public function __construct(AbstractExpression $condition, Node $body, ?Node $else, int $lineno)
    {
        $nodes = ['condition' => $condition, 'body' => $body];
        if (null !== $else) {
            $nodes['else'] = $else;
        }
        parent::__construct($nodes, [], $lineno);
    }

    public function compile(Compiler $compiler): void
    {
        $compiler
            ->addDebugInfo($this)
            ->write('if (!(')
            ->subcompile($this->getNode('condition'))
            ->raw(")) {\n")
            ->indent()
            ->subcompile($this->getNode('body'))
            ->outdent()
        ;
        if ($this->hasNode('else')) {
            $compiler
                ->write("} else {\n")
                ->indent()
                ->subcompile($this->getNode('else'))
                ->outdent()
            ;
        }
        $compiler->write("}\n");
    }
}

final class UnlessTokenParser extends AbstractTokenParser
{
    public function parse(Token $token): Node
    {
        $stream = $this->parser->getStream();
        $condition = $this->parser->parseExpression();
        $stream->expect(Token::BLOCK_END_TYPE);
        $body = $this->parser->subparse([$this, 'decideFork']);
        $else = null;
        if ('else' === $stream->next()->getValue()) {
            $stream->expect(Token::BLOCK_END_TYPE);
            $else = $this->parser->subparse([$this, 'decideEnd'], true);
        }
        $stream->expect(Token::BLOCK_END_TYPE);

        return new UnlessNode($condition, $body, $else, $token->getLine());
    }

    public function decideFork(Token $token): bool
    {
        return $token->test(['else', 'endunless']);
    }

    public function decideEnd(Token $token): bool
    {
        return $token->test('endunless');
    }

    public function getTag(): string
    {
        return 'unless';
    }
}

// ---- {% shout_set name = expr %}: $context['name'] = strtoupper(expr) ---------------------------------------------

#[YieldReady]
final class ShoutSetNode extends Node
{
    public function __construct(string $name, AbstractExpression $value, int $lineno)
    {
        parent::__construct(['value' => $value], ['name' => $name], $lineno);
    }

    public function compile(Compiler $compiler): void
    {
        if ('_context' === $this->getAttribute('name')) {
            throw new \LogicException('Cannot assign to _context.');
        }
        $compiler
            ->addDebugInfo($this)
            ->write('$context[')
            ->string($this->getAttribute('name'))
            ->raw('] = strtoupper(')
            ->subcompile($this->getNode('value'))
            ->raw(");\n")
        ;
    }
}

final class ShoutSetTokenParser extends AbstractTokenParser
{
    public function parse(Token $token): Node
    {
        $stream = $this->parser->getStream();
        $name = $stream->expect(Token::NAME_TYPE)->getValue();
        $stream->expect(Token::OPERATOR_TYPE, '=');
        $value = $this->parser->parseExpression();
        $stream->expect(Token::BLOCK_END_TYPE);

        return new ShoutSetNode($name, $value, $token->getLine());
    }

    public function getTag(): string
    {
        return 'shout_set';
    }
}

// ---- {% shout_block %}...{% endshout_block %}: yield strtoupper(<captured body>) ----------------------------------

#[YieldReady]
final class ShoutBlockNode extends Node
{
    public function __construct(Node $body, int $lineno)
    {
        $capture = new CaptureNode($body, $lineno);
        $capture->setAttribute('raw', true);
        parent::__construct(['body' => $capture], [], $lineno);
    }

    public function compile(Compiler $compiler): void
    {
        $var = $compiler->getVarName();
        $compiler
            ->addDebugInfo($this)
            ->write("\$$var = ")
            ->subcompile($this->getNode('body'))
            ->raw("\n")
            ->write("yield strtoupper(\$$var);\n")
        ;
    }
}

final class ShoutBlockTokenParser extends AbstractTokenParser
{
    public function parse(Token $token): Node
    {
        $stream = $this->parser->getStream();
        $stream->expect(Token::BLOCK_END_TYPE);
        $body = $this->parser->subparse(fn (Token $token) => $token->test('endshout_block'), true);
        $stream->expect(Token::BLOCK_END_TYPE);

        return new ShoutBlockNode($body, $token->getLine());
    }

    public function getTag(): string
    {
        return 'shout_block';
    }
}

// ---- {% repeat count %}...{% endrepeat %}: for ($i = 0; $i < count; ++$i) { body } -------------------------------

#[YieldReady]
final class RepeatNode extends Node
{
    public function __construct(AbstractExpression $count, Node $body, int $lineno)
    {
        parent::__construct(['count' => $count, 'body' => $body], [], $lineno);
    }

    public function compile(Compiler $compiler): void
    {
        $count = $this->getNode('count');
        if ($count instanceof ConstantExpression && !is_numeric($count->getAttribute('value'))) {
            throw new SyntaxError('The count of the "repeat" tag must be a number.', $this->getTemplateLine());
        }
        $i = $compiler->getVarName();
        $compiler
            ->addDebugInfo($this)
            ->write("for (\$$i = 0; \$$i < ")
            ->subcompile($this->getNode('count'))
            ->raw("; \$$i = \$$i + 1) {\n")
            ->indent()
            ->subcompile($this->getNode('body'))
            ->outdent()
            ->write("}\n")
        ;
    }
}

final class RepeatTokenParser extends AbstractTokenParser
{
    public function parse(Token $token): Node
    {
        $stream = $this->parser->getStream();
        $count = $this->parser->parseExpression();
        $stream->expect(Token::BLOCK_END_TYPE);
        $body = $this->parser->subparse(fn (Token $token) => $token->test('endrepeat'), true);
        $stream->expect(Token::BLOCK_END_TYPE);

        return new RepeatNode($count, $body, $token->getLine());
    }

    public function getTag(): string
    {
        return 'repeat';
    }
}

// ---- expression classes -------------------------------------------------------------------------------------------

/** The node class of the `shout`, `shout_safe` and `shout2` functions: `strtoupper(<first argument>)`. */
final class ShoutFunction extends FunctionExpression
{
    public function compile(Compiler $compiler): void
    {
        $compiler
            ->raw('strtoupper(')
            ->subcompile($this->getNode('arguments')->getNode('0'))
            ->raw(')')
        ;
    }
}

/** The node class of the `fail` and `fail_logic` functions: a runtime check of the first argument. */
final class FailFunction extends FunctionExpression
{
    public function compile(Compiler $compiler): void
    {
        $compiler
            ->raw('\FailRuntime::'.$this->getAttribute('name').'(')
            ->subcompile($this->getNode('arguments')->getNode('0'))
            ->raw(')')
        ;
    }
}

final class FailRuntime
{
    public static function fail($value)
    {
        if (!$value) {
            throw new RuntimeError('Value is empty.');
        }

        return $value;
    }

    public static function fail_logic($value)
    {
        if (!$value) {
            throw new \LogicException('Value is empty.');
        }

        return $value;
    }
}

/** The node class of the `wrap` and `wrap2` filters: `'<p>' . <input> . '</p>'`. */
final class WrapFilter extends FilterExpression
{
    public function compile(Compiler $compiler): void
    {
        $compiler
            ->raw('(\'<p>\' . ')
            ->subcompile($this->getNode('node'))
            ->raw(' . \'</p>\')')
        ;
    }
}

/** The node class of the `empty_string` test: `('' === <input>)`. */
final class EmptyStringTest extends TestExpression
{
    public function compile(Compiler $compiler): void
    {
        $compiler
            ->raw('(\'\' === ')
            ->subcompile($this->getNode('node'))
            ->raw(')')
        ;
    }
}

/** `a orelse b`: `((a) ?: (b))`, an operator whose value is one of its operands. */
final class OrElseBinary extends AbstractBinary implements OperatorEscapeInterface
{
    public function compile(Compiler $compiler): void
    {
        $compiler
            ->raw('((')
            ->subcompile($this->getNode('left'))
            ->raw(') ?: (')
            ->subcompile($this->getNode('right'))
            ->raw('))')
        ;
    }

    public function operator(Compiler $compiler): Compiler
    {
        return $compiler->raw('?:');
    }

    public function getOperandNamesToEscape(): array
    {
        return ['left', 'right'];
    }
}

/** `a three b`: `a orelse b` naming three operands (the left one twice). */
final class ThreeBinary extends AbstractBinary implements OperatorEscapeInterface
{
    public function compile(Compiler $compiler): void
    {
        $compiler
            ->raw('((')
            ->subcompile($this->getNode('left'))
            ->raw(') ?: (')
            ->subcompile($this->getNode('right'))
            ->raw('))')
        ;
    }

    public function operator(Compiler $compiler): Compiler
    {
        return $compiler->raw('?:');
    }

    public function getOperandNamesToEscape(): array
    {
        return ['left', 'right', 'left'];
    }
}

/** `up x`: `strtoupper(x)`. */
final class UpUnary extends AbstractUnary
{
    public function compile(Compiler $compiler): void
    {
        $compiler->raw('strtoupper(')->subcompile($this->getNode('node'))->raw(')');
    }

    public function operator(Compiler $compiler): Compiler
    {
        return $compiler->raw('strtoupper');
    }
}

/** `upc x`: `up x` that declares its string coercion. */
final class UpCoercingUnary extends AbstractUnary implements CoercesChildrenToStringInterface
{
    public function compile(Compiler $compiler): void
    {
        $compiler->raw('strtoupper(')->subcompile($this->getNode('node'))->raw(')');
    }

    public function operator(Compiler $compiler): Compiler
    {
        return $compiler->raw('strtoupper');
    }

    public function getStringCoercedChildNames(): array
    {
        return ['node'];
    }
}

final class CustomNodesExtension extends AbstractExtension
{
    public function getTokenParsers(): array
    {
        return [new UnlessTokenParser(), new ShoutSetTokenParser(), new ShoutBlockTokenParser(), new RepeatTokenParser()];
    }

    public function getFunctions(): array
    {
        return [
            new TwigFunction('shout', null, ['node_class' => ShoutFunction::class]),
            new TwigFunction('shout_safe', null, ['node_class' => ShoutFunction::class, 'is_safe' => ['html']]),
            new TwigFunction('shout2', static fn (string $s, int $n) => '', ['node_class' => ShoutFunction::class]),
            new TwigFunction('fail', null, ['node_class' => FailFunction::class]),
            new TwigFunction('fail_logic', null, ['node_class' => FailFunction::class]),
        ];
    }

    public function getFilters(): array
    {
        return [
            new TwigFilter('wrap', null, ['node_class' => WrapFilter::class, 'pre_escape' => 'html', 'is_safe' => ['html']]),
            new TwigFilter('wrap2', null, ['node_class' => WrapFilter::class, 'preserves_safety' => ['html']]),
        ];
    }

    public function getTests(): array
    {
        return [new TwigTest('empty_string', null, ['node_class' => EmptyStringTest::class])];
    }

    public function getExpressionParsers(): array
    {
        return [
            new BinaryOperatorExpressionParser(OrElseBinary::class, 'orelse', 15),
            new BinaryOperatorExpressionParser(ThreeBinary::class, 'three', 15),
            new UnaryOperatorExpressionParser(UpUnary::class, 'up', 500),
            new UnaryOperatorExpressionParser(UpCoercingUnary::class, 'upc', 500),
        ];
    }
}

/** A Stringable object (`obj` in the contexts). */
final class Obj
{
    public function __toString(): string
    {
        return 'obj';
    }
}

// ---- the cases ----------------------------------------------------------------------------------------------------

function c(string $name, $templates, array $context = [], array $options = []): array
{
    return [
        'name' => $name,
        'templates' => is_string($templates) ? ['index.twig' => $templates] : $templates,
        'context' => (object) $context,
        'objects' => $options['objects'] ?? [],
        'strict_variables' => $options['strict_variables'] ?? false,
        'sandbox' => $options['sandbox'] ?? null,
    ];
}

$sandbox = static fn (array $policy) => ['sandbox' => $policy + ['tags' => [], 'filters' => [], 'functions' => [], 'tests' => []]];

$cases = [
    // a tag compiling itself
    c('unless_true', '{% unless x %}no{% else %}yes{% endunless %}|{% unless y %}<{{ y }}>{% endunless %}', ['x' => true, 'y' => 0]),
    c('unless_false', '{% unless x %}no{% else %}yes{% endunless %}|{% unless y %}<{{ y }}>{% endunless %}', ['x' => false, 'y' => 'a']),
    c('unless_loop', '{% for i in 1..3 %}{% unless loop.first %}, {% endunless %}{{ i }}{% endfor %}'),
    c('unless_loop_last', '{% for i in [1, 2, 3] %}{{ i }}{% unless loop.last %}+{% else %}={% endunless %}{% endfor %}'),
    c('unless_block', ['index.twig' => "{% extends 'base.twig' %}{% block b %}{% unless x %}[{{ parent() }}]{% endunless %}{% endblock %}", 'base.twig' => '<{% block b %}base{% endblock %}>'], ['x' => 0]),
    c('unless_macro', "{% macro m(v) %}{% unless v %}empty{% else %}{{ v }}{% endunless %}{% endmacro %}{{ _self.m('') }}|{{ _self.m('a') }}"),
    c('unless_escape', '{% unless x %}{{ y }}{% endunless %}', ['y' => '<b>']),
    c('unless_apply', '{% apply upper %}{% unless x %}a{% else %}b{% endunless %}{% endapply %}', ['x' => 1]),
    c('unless_whitespace', "a {%- unless x -%} b {%- endunless -%} c\n{% unless x %}\n  d\n{% endunless %}\n"),
    c('unless_nested', '{% unless a %}{% unless b %}none{% else %}b{% endunless %}{% else %}a{% endunless %}', ['a' => 0, 'b' => 0]),
    c('unless_strict_condition', "line 1\n{% unless foo.bar %}\nx{% endunless %}", [], ['strict_variables' => true]),
    c('unless_body_error', "{% unless x %}\n\n{{ 1 / 0 }}{% endunless %}"),
    c('unless_else_error', "{% unless x %}a{% else %}\n{{ missing.attr }}\n{% endunless %}", ['x' => 1], ['strict_variables' => true]),
    c('unless_syntax', '{% unless x %}a{% endif %}'),
    c('unless_sandbox_denied', 'a{% unless x %}b{% endunless %}', [], $sandbox([])),
    c('unless_sandbox_allowed', '{% unless x %}<{{ y }}>{% endunless %}', ['y' => '&'], $sandbox(['tags' => ['unless'], 'filters' => ['escape']])),
    c('shout_set', "{% shout_set v = 'abc' ~ x %}{{ v }}", ['x' => 'd']),
    c('shout_block', '{% shout_block %}a{{ x }}<b>{% endshout_block %}|{{ x }}', ['x' => '<i>']),
    c('repeat', '{% repeat n %}x{% endrepeat %}|{% repeat 0 %}y{% endrepeat %}|{% repeat 2.5 %}z{% endrepeat %}', ['n' => 3]),
    c('repeat_nested', '{% repeat 2 %}[{% repeat 3 %}.{% endrepeat %}]{% endrepeat %}'),
    c('repeat_syntax_error', "a\n{% repeat 'x' %}y{% endrepeat %}"),
    c('shout_set_logic_error', "a\n{% shout_set _context = 'x' %}"),

    // a FunctionExpression subclass
    c('shout_escape', '{{ shout(x) }}|{{ shout(x)|raw }}', ['x' => '<b>a']),
    c('shout_safe', '{{ shout_safe(x) }}', ['x' => '<b>a']),
    c('shout_concat', "{{ 'x' ~ shout(x) }}", ['x' => '<b>']),
    c('shout_sandbox_denied', '{{ shout(x) }}', ['x' => 'a'], $sandbox(['filters' => ['escape']])),
    c('shout_sandbox_wrap', '{{ shout(obj) }}', [], $sandbox(['functions' => ['shout'], 'filters' => ['escape']]) + ['objects' => ['obj']]),
    c('shout2_typed_parameter', "{{ shout2('a', obj) }}", [], $sandbox(['functions' => ['shout2'], 'filters' => ['escape']]) + ['objects' => ['obj']]),
    c('shout2_untyped_parameter', '{{ shout2(obj, 1) }}', [], $sandbox(['functions' => ['shout2'], 'filters' => ['escape']]) + ['objects' => ['obj']]),
    c('fail_line', "{{ 'a' }}\n{{ fail(\n  x\n) }}", ['x' => '']),
    c('fail_logic_line', "{{ 'a' }}\n{{ fail_logic(\n  x\n) }}", ['x' => 0]),
    c('fail_passes', '{{ fail(x) }}', ['x' => 'ok']),

    // a FilterExpression subclass
    c('wrap_pre_escape', "{{ x|wrap }}|{{ '<i>'|wrap }}", ['x' => '<b>']),
    c('wrap_preserves_safety', "{{ '<i>'|wrap2 }}|{{ x|wrap2 }}", ['x' => '<b>']),
    c('wrap_sandbox_denied', '{{ x|wrap }}', ['x' => 'a'], $sandbox(['filters' => ['escape']])),
    c('wrap_sandbox_wrap', '{{ obj|wrap }}', [], $sandbox(['filters' => ['escape', 'wrap']]) + ['objects' => ['obj']]),

    // a TestExpression subclass
    c('empty_string', "{{ x is empty_string ? 'empty' : 'full' }}|{{ y is empty_string ? 'empty' : 'full' }}", ['x' => '', 'y' => 0]),
    c('empty_string_sandbox_denied', "{{ x is empty_string ? 'e' }}", ['x' => ''], $sandbox(['filters' => ['escape']])),

    // an OperatorEscapeInterface
    c('orelse_escape', "{{ '<i>a</i>' orelse x }}|{{ '' orelse x }}", ['x' => '<b>']),
    c('orelse_nested', "{{ x orelse ('' orelse '<i>') }}", ['x' => '']),
    c('orelse_concat', "{{ (x orelse '<i>') ~ '' }}", ['x' => '']),
    c('orelse_preserved', "{{ ('<i>' orelse '<b>')|wrap2 }}|{{ (x orelse '<b>')|wrap2 }}", ['x' => '<u>']),
    c('orelse_sandbox', "{{ obj orelse 'x' }}", [], $sandbox(['filters' => ['escape']]) + ['objects' => ['obj']]),
    c('three_printed', '{{ a three b }}|{{ c three b }}', ['a' => '<', 'b' => '>', 'c' => '']),
    c('three_nested', "{{ (a three b) ~ '' }}", ['a' => '<', 'b' => '>']),

    // a plain expression class, and one declaring its string coercion
    c('up_escape', '{{ up x }}', ['x' => '<b>a']),
    c('up_sandbox', '{{ up obj }}', [], $sandbox(['filters' => ['escape']]) + ['objects' => ['obj']]),
    c('upc_sandbox', '{{ upc obj }}', [], $sandbox(['filters' => ['escape']]) + ['objects' => ['obj']]),
];

// ---- rendering ----------------------------------------------------------------------------------------------------

$deprecations = [];
set_error_handler(static function (int $level, string $message) use (&$deprecations) {
    if (E_USER_DEPRECATED === $level || E_DEPRECATED === $level) {
        $deprecations[] = $message;

        return true;
    }
    throw new \ErrorException($message, 0, $level);
});

$results = [];
foreach ($cases as $case) {
    $deprecations = [];
    $result = $case;
    $result['output'] = null;
    $result['error'] = null;
    try {
        $twig = new Environment(new ArrayLoader($case['templates']), ['cache' => false, 'strict_variables' => $case['strict_variables']]);
        $twig->addExtension(new CustomNodesExtension());
        if (null !== $case['sandbox']) {
            $policy = new SecurityPolicy($case['sandbox']['tags'], $case['sandbox']['filters'], [], [], $case['sandbox']['functions'], $case['sandbox']['tests']);
            $policy->setStrict(true);
            $twig->addExtension(new SandboxExtension($policy, true));
        }
        $context = (array) $case['context'];
        foreach ($case['objects'] as $name) {
            $context[$name] = new Obj();
        }
        $result['output'] = $twig->render('index.twig', $context);
    } catch (\Throwable $e) {
        $result['error'] = ['class' => get_class($e), 'message' => $e->getMessage()];
    }
    $result['deprecations'] = $deprecations;
    $results[] = $result;
}

echo json_encode($results, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";

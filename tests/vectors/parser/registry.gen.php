<?php

/*
 * Generator of tests/vectors/parser/registry.json: the registry of a Twig 3.31.1 environment with the core
 * extensions plus the Debug, StringLoader, Sandbox and SandboxBridge extensions: every filter, function and test
 * (options, PHP reflection of the callable), the token parser tags, the expression parsers in iteration order with
 * their precedence changes, and the lexer's operator tokens.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/parser/registry.gen.php > tests/vectors/parser/registry.json
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\ExpressionParser\ExpressionParsers;
use Twig\ExpressionParser\InfixExpressionParserInterface;
use Twig\ExpressionParser\PrefixExpressionParserInterface;
use Twig\Loader\ArrayLoader;
use Twig\Util\ReflectionCallable;

$env = new Environment(new ArrayLoader([]));
$env->addExtension(new \Twig\Extension\DebugExtension());
$env->addExtension(new \Twig\Extension\StringLoaderExtension());
$env->addExtension(new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy()));
$env->addExtension(new \Twig\Extension\SandboxBridgeExtension());

function short_class(string $class): string
{
    return substr($class, strrpos($class, '\\') + 1);
}

function describe_callable($c): array
{
    $options = (fn () => $this->options)->call($c);
    $out = [
        'name' => $c->getName(),
        'node_class' => short_class($options['node_class']),
        'needs_environment' => $options['needs_environment'],
        'needs_context' => $options['needs_context'],
        'needs_charset' => $options['needs_charset'],
        'needs_is_sandboxed' => $options['needs_is_sandboxed'],
        'is_variadic' => $options['is_variadic'],
        'always_allowed_in_sandbox' => $options['always_allowed_in_sandbox'],
        'deprecation' => $options['deprecation_info'] ? (function () {
            return [$this->package, $this->version, $this->altName, $this->altPackage, $this->altVersion];
        })->call($options['deprecation_info']) : null,
        'is_safe' => $options['is_safe'] ?? null,
        'has_is_safe_callback' => null !== ($options['is_safe_callback'] ?? null),
        'pre_escape' => $options['pre_escape'] ?? null,
        'preserves_safety' => $options['preserves_safety'] ?? null,
        'one_mandatory_argument' => $options['one_mandatory_argument'] ?? null,
        'has_parser_callable' => null !== ($options['parser_callable'] ?? null),
        'min_required' => $c->getMinimalNumberOfRequiredArguments(),
        'callable' => null,
    ];
    if (null !== $c->getCallable()) {
        $rc = new ReflectionCallable($c);
        $params = [];
        foreach ($rc->getReflector()->getParameters() as $p) {
            $params[] = [
                'name' => $p->getName(),
                'optional' => $p->isOptional(),
                'default' => $p->isDefaultValueAvailable() ? str_replace("\n", '', var_export($p->getDefaultValue(), true)) : null,
                'variadic' => $p->isVariadic(),
                'array' => $p->hasType() && $p->getType() instanceof \ReflectionNamedType && 'array' === $p->getType()->getName(),
            ];
        }
        $out['callable'] = [
            'reflection_name' => $rc->getName(),
            'required' => $rc->getReflector()->getNumberOfRequiredParameters(),
            'params' => $params,
            'string' => is_string($c->getCallable()) ? $c->getCallable() : null,
        ];
    }

    return $out;
}

$out = ['filters' => [], 'functions' => [], 'tests' => []];
foreach ($env->getFilters() as $filter) {
    $out['filters'][] = describe_callable($filter);
}
foreach ($env->getFunctions() as $function) {
    $out['functions'][] = describe_callable($function);
}
foreach ($env->getTests() as $test) {
    $out['tests'][] = describe_callable($test);
}
$out['tags'] = array_keys($env->getTokenParsers());
$out['expression_parsers'] = [];
foreach ($env->getExpressionParsers() as $parser) {
    $change = $parser->getPrecedenceChange();
    $out['expression_parsers'][] = [
        'string' => (string) $parser,
        'class' => short_class(get_class($parser)),
        'name' => $parser->getName(),
        'precedence' => $parser->getPrecedence(),
        'associativity' => $parser instanceof InfixExpressionParserInterface ? $parser->getAssociativity()->name : null,
        'precedence_change' => $change ? [$change->getPackage(), $change->getVersion(), $change->getNewPrecedence()] : null,
        'aliases' => $parser->getAliases(),
        'tokens' => ExpressionParsers::getOperatorTokensFor($parser),
        'description' => method_exists($parser, 'getDescription') ? $parser->getDescription() : null,
    ];
}
$changes = $env->getExpressionParsers()->getPrecedenceChanges();
$out['precedence_changes'] = [];
foreach ($env->getExpressionParsers() as $parser) {
    if (isset($changes[$parser])) {
        $out['precedence_changes'][] = [(string) $parser, array_map(fn ($e) => (string) $e, $changes[$parser])];
    }
}
$out['operator_tokens'] = [];
foreach ($env->getExpressionParsers() as $parser) {
    foreach (ExpressionParsers::getOperatorTokensFor($parser) as $token) {
        $out['operator_tokens'][] = $token;
    }
}
$out['visitors'] = array_map(fn ($v) => [short_class(get_class($v)), $v->getPriority()], $env->getNodeVisitors());

echo json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";

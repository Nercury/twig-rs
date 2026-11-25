<?php

/*
 * Generator of tests/vectors/parser/fixtures.json: the parse result of every template of Twig 3.31.1's integration
 * fixtures (vendor/twig/twig/tests/Fixtures, read like IntegrationTestCase::assembleTests() does), in the
 * environment of Twig's integration tests (Twig\Tests\IntegrationTest: Debug, Sandbox, StringLoader and the test
 * extensions, the undefined-callable callbacks), with the parser's own node visitors only (see
 * expressions.gen.php): the node tree, or the error, plus the deprecations. Each template is parsed on its own, in a
 * fresh environment, without the spaces the harness appends for the second and later DATA/EXPECT pairs.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/parser/fixtures.gen.php > tests/vectors/parser/fixtures.json
 *
 * Output: a list of {file, template, source (or source_b64), dump (or error: {class, message}), deprecations}, in
 * file order (sorted paths) and template order. The dump format is the one of expressions.gen.php.
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';
require '/oracle/lib.php';

use Twig\Environment;
use Twig\Node\Expression\MacroReferenceExpression;
use Twig\Node\Expression\SupportDefinedTestInterface;
use Twig\Node\ModuleNode;
use Twig\Node\Node;
use Twig\NodeVisitor\CorrectnessNodeVisitor;
use Twig\NodeVisitor\NodeVisitorInterface;
use Twig\Source;

use function TwigOracle\integration_environment;
use function TwigOracle\parse_fixture;

final class DumpVisitor implements NodeVisitorInterface
{
    public ?string $dump = null;

    public function enterNode(Node $node, Environment $env): Node
    {
        if ($node instanceof ModuleNode) {
            // embedded templates are parsed (and visited) first; the outermost module is the last one
            $this->dump = rs_dump($node);
        }

        return $node;
    }

    public function leaveNode(Node $node, Environment $env): ?Node
    {
        return $node;
    }

    public function getPriority(): int
    {
        return -100000;
    }
}

function rs_quote(string $s): string
{
    $out = '"';
    $length = strlen($s);
    for ($i = 0; $i < $length; ++$i) {
        $c = $s[$i];
        $b = ord($c);
        if ('"' === $c || '\\' === $c) {
            $out .= '\\'.$c;
        } elseif ($b < 0x20 || 0x7F === $b) {
            $out .= sprintf('\\x%02x', $b);
        } else {
            $out .= $c;
        }
    }

    return $out.'"';
}

function rs_value($value): string
{
    if (null === $value) {
        return 'NULL';
    }
    if (is_bool($value)) {
        return $value ? 'true' : 'false';
    }
    if (is_int($value)) {
        return (string) $value;
    }
    if (is_float($value)) {
        return 'float('.var_export($value, true).')';
    }
    if (is_string($value)) {
        return rs_quote($value);
    }
    if (is_array($value)) {
        $parts = [];
        foreach ($value as $k => $v) {
            $parts[] = rs_value($k).' => '.rs_value($v);
        }

        return '['.implode(', ', $parts).']';
    }
    if ($value instanceof \Closure) {
        return '\Closure';
    }
    if ($value instanceof \Stringable) {
        return (string) $value;
    }

    return 'object('.get_class($value).')';
}

function rs_dump(Node $node, string $prefix = '', int $depth = 0): string
{
    $pad = str_repeat('  ', $depth);
    $class = get_class($node);
    if (str_starts_with($class, 'Twig\\Node\\')) {
        $class = substr($class, strlen('Twig\\Node\\'));
    }
    $line = $pad.$prefix.$class.' @'.$node->getTemplateLine();
    if (null !== $tag = $node->getNodeTag()) {
        $line .= ' #'.$tag;
    }
    if (null !== $doc = $node->getDocumentation()) {
        $line .= ' doc='.rs_quote($doc);
    }
    $lines = [$line];
    $nodeAttributes = [];
    foreach ((fn () => $this->attributes)->call($node) as $name => $value) {
        if ($value instanceof Node) {
            $nodeAttributes[$name] = $value;
            continue;
        }
        $lines[] = $pad.'  .'.$name.' = '.rs_value($value);
    }
    if ($node instanceof SupportDefinedTestInterface && $node->isDefinedTestEnabled()) {
        $lines[] = $pad.'  .(defined_test) = true';
    }
    if ($node instanceof MacroReferenceExpression) {
        $lines[] = $pad.'  .(call_parentheses) = '.($node->hasCallParentheses() ? 'true' : 'false');
    }
    foreach ($node->getNodes() as $key => $child) {
        $lines[] = rs_dump($child, '['.$key.'] ', $depth + 1);
    }
    foreach ($nodeAttributes as $name => $child) {
        $lines[] = rs_dump($child, '[.'.$name.'] ', $depth + 1);
    }

    return implode("\n", $lines);
}

/**
 * Initializes the extensions and keeps only the parser's own node visitors (the CorrectnessNodeVisitor and the
 * dump visitor): the escaper, optimizer, yield-ready and sandbox visitors belong to later stages.
 */
function keep_parser_visitors(Environment $env): void
{
    $env->getNodeVisitors();
    $set = (fn () => $this->extensionSet)->call($env);
    (function (): void {
        $this->visitors = array_values(array_filter(
            $this->visitors,
            static fn (NodeVisitorInterface $v): bool => $v instanceof CorrectnessNodeVisitor || $v instanceof DumpVisitor,
        ));
    })->call($set);
}

function enc_str(?string $s)
{
    if (null === $s) {
        return null;
    }

    return mb_check_encoding($s, 'UTF-8') ? $s : ['b64' => base64_encode($s)];
}


$root = '/oracle/vendor/twig/twig/tests/Fixtures';
$files = [];
foreach (new \RecursiveIteratorIterator(new \RecursiveDirectoryIterator($root, \FilesystemIterator::SKIP_DOTS)) as $file) {
    if (str_ends_with($file->getPathname(), '.test')) {
        $files[] = substr($file->getPathname(), strlen($root) + 1);
    }
}
sort($files, \SORT_STRING);

$out = [];
foreach ($files as $rel) {
    $fixture = parse_fixture(file_get_contents($root.'/'.$rel));
    foreach ($fixture['templates'] as $name => $code) {
        $name = (string) $name;
        $deprecations = [];
        set_error_handler(function (int $type, string $message) use (&$deprecations): bool {
            if (E_USER_DEPRECATED === $type) {
                $deprecations[] = $message;

                return true;
            }

            throw new \ErrorException($message, 0, $type);
        });
        $res = ['file' => $rel, 'template' => $name];
        if (mb_check_encoding($code, 'UTF-8')) {
            $res['source'] = $code;
        } else {
            $res['source_b64'] = base64_encode($code);
        }
        $visitor = new DumpVisitor();
        $error = null;
        try {
            $env = integration_environment($fixture['templates'], []);
            $env->addNodeVisitor($visitor);
            keep_parser_visitors($env);
            $env->parse($env->tokenize(new Source($code, $name)));
        } catch (\Throwable $e) {
            $error = ['class' => get_class($e), 'message' => enc_str($e->getMessage())];
        }
        restore_error_handler();
        if (null !== $error) {
            $res['error'] = $error;
        } else {
            $res['dump'] = enc_str($visitor->dump);
        }
        $res['deprecations'] = array_map('enc_str', $deprecations);
        $out[] = $res;
    }
}

echo json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION | JSON_THROW_ON_ERROR), "\n";

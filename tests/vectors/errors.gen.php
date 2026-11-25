<?php

/*
 * Generator of tests/vectors/errors.json: Twig 3.31.1's error message formatting (Error::updateRepr()), PHP's
 * levenshtein() and SyntaxError::addSuggestions() with the registries of a default Environment.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/errors.gen.php > tests/vectors/errors.json
 *
 * Output: {"repr": [[class, raw message, line, column, source name|null, message]],
 *          "levenshtein": [[a, b, distance]],
 *          "registries": {"tags": [...], "filters": [...], "functions": [...], "tests": [...]},
 *          "suggestions": [[registry, name, raw message after addSuggestions]]}
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\Error\LoaderError;
use Twig\Error\RuntimeError;
use Twig\Error\SyntaxError;
use Twig\Loader\ArrayLoader;
use Twig\Source;

function j($value): string
{
    return json_encode($value, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR);
}

$messages = ['', '.', '?', 'a', 'a.', 'a?', 'a!', 'Unknown "x" function. Did you mean "y"?', 'x..', 'x?.', "multi\nline.", 'é.', 'Unclosed "(".'];
$lines = [-1, 0, 1, 42, PHP_INT_MAX];
$columns = [null, 9];
$names = [null, '', '0', '00', 'index', 'a "b"', 'dir/t.twig'];
$classes = [SyntaxError::class, LoaderError::class, RuntimeError::class];

$repr = [];
$i = 0;
foreach ($messages as $message) {
    foreach ($lines as $line) {
        foreach ($columns as $column) {
            foreach ($names as $name) {
                $class = $classes[$i++ % 3];
                $source = null === $name ? null : new Source('', $name);
                $e = new $class($message, $line, $source, null, $column);
                $repr[] = [$class, $message, $line, $column, $name, $e->getMessage()];
            }
        }
    }
}

$pairs = [
    ['', ''], ['', 'abc'], ['abc', ''], ['kitten', 'sitting'], ['sitting', 'kitten'], ['flaw', 'lawn'],
    ['cycl', 'cycle'], ['é', 'e'], ['日本', '日'], ['abc', 'abc'], ['abcdef', 'azced'], ['a', 'b'], ['ab', 'ba'],
    ['include', 'includ'], ['date_modify', 'datemodify'], ['number_format', 'format'],
    [str_repeat('ab', 40), str_repeat('ba', 35)], ['Upper', 'upper'], ['same as', 'same'], ['divisible by', 'divisible'],
];
$levenshtein = array_map(fn ($p) => [$p[0], $p[1], levenshtein($p[0], $p[1])], $pairs);

$env = new Environment(new ArrayLoader());
// The lists the parser passes to addSuggestions(): array_keys() of each registry.
$registries = [
    'tags' => array_map('strval', array_keys($env->getTokenParsers())),
    'filters' => array_map('strval', array_keys($env->getFilters())),
    'functions' => array_map('strval', array_keys($env->getFunctions())),
    'tests' => array_map('strval', array_keys($env->getTests())),
];

$names = ['e', 'f', 'fo', 'includ', 'cycl', 'cl', 'n', '', 'date', 'xyz', 'lenght', 'ab', 'datemodify', 'same', 'divisible', 'odd foo', 'Upper', 'é', 'r', 'ma', 'ra', 'en', 'defined', 'nul', 'ever', 'mapping', 'escap', 'json', 'url', 'filter'];
$suggestions = [];
foreach ($registries as $registry => $items) {
    foreach ($names as $name) {
        $e = new SyntaxError(sprintf('Unknown "%s" %s.', $name, $registry), 1, new Source('', 'index'));
        $e->addSuggestions($name, $items);
        $suggestions[] = [$registry, $name, $e->getRawMessage(), $e->getMessage()];
    }
}
// Repeated items keep their first position (an ordered map in PHP).
$e = new SyntaxError('Unknown "fo" thing.', 1);
$e->addSuggestions('fo', ['for', 'fox', 'for', 'f']);
$suggestions[] = ['custom', 'fo', $e->getRawMessage(), $e->getMessage()];

echo "{\n";
echo "\"repr\": [\n", implode(",\n", array_map('j', $repr)), "\n],\n";
echo "\"levenshtein\": [\n", implode(",\n", array_map('j', $levenshtein)), "\n],\n";
echo "\"registries\": ", j($registries), ",\n";
echo "\"suggestions\": [\n", implode(",\n", array_map('j', $suggestions)), "\n]\n";
echo "}\n";

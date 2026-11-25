<?php

/*
 * Generator of tests/vectors/loader/scenarios.json: Twig 3.31.1's FilesystemLoader and ChainLoader
 * (TWIG/src/Loader/) on PHP 8.5.13, run on a real directory tree: paths and namespaces, name normalisation,
 * security checks, the positive and negative lookup caches, cache keys, isFresh(), the root path, file:// paths,
 * symlinks and ChainLoader's error aggregation and exists() memo.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/loader/scenarios.gen.php > tests/vectors/loader/scenarios.json
 *
 * Output: {"tree": {"dirs": [...], "files": {path: content}},
 *          "scenarios": [{"name", "unix_only"?, "steps": [step]}]}
 * Every scenario starts from a fresh copy of the tree under <ROOT> (here /tmp/twig-loader-vectors). Steps:
 *   {"fs": "write"|"unlink"|"mkdir"|"touch"|"symlink", "path", "content"?|"mtime"?|"target"?}
 *   {"new": id, "class": "filesystem", "paths": [...]|string, "root": string|null, "expect"}
 *   {"new": id, "class": "array", "templates": {...}, "expect"}
 *   {"new": id, "class": "chain", "loaders": [id...], "expect"}
 *   {"new": id, "class": "throwing", "label", "exists": bool, "throws": "loader"|"loader_with_line"|"runtime", "expect"}
 *   {"call": method, "on": id, "args": [...], "loader"?: id (addLoader), "unix_only"?: true, "expect"}
 * "expect" is {"ok": value} or {"error": {"class", "message"}}, plus "warnings": [...] for PHP warnings and
 * notices (collected; PHP then continues with its default behaviour). Values: null for void methods, booleans,
 * strings (cache keys), lists of strings (getPaths(), getNamespaces() with numeric keys cast to strings) and
 * {"code", "name", "path"} for Source objects. "<ROOT>" stands for the tree root in inputs and outputs.
 * PHP's stat and realpath caches are cleared after every filesystem step (twig-rs has no such caches).
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Error\LoaderError;
use Twig\Error\RuntimeError;
use Twig\Loader\ArrayLoader;
use Twig\Loader\ChainLoader;
use Twig\Loader\FilesystemLoader;
use Twig\Loader\LoaderInterface;
use Twig\Source;

const ROOT = '/tmp/twig-loader-vectors';

/** A loader whose exists() answer is fixed and whose other methods always throw. */
final class ThrowingLoader implements LoaderInterface
{
    public function __construct(private string $label, private bool $exists, private string $throws)
    {
    }

    private function fail(string $name): never
    {
        $message = sprintf('%s cannot load "%s".', $this->label, $name);
        throw match ($this->throws) {
            'loader' => new LoaderError($message),
            'loader_with_line' => new LoaderError($message, 3, new Source('', 'origin.twig')),
            'runtime' => new RuntimeError($message),
        };
    }

    public function getSourceContext(string $name): Source
    {
        $this->fail($name);
    }

    public function getCacheKey(string $name): string
    {
        $this->fail($name);
    }

    public function isFresh(string $name, int $time): bool
    {
        $this->fail($name);
    }

    public function exists(string $name)
    {
        return $this->exists;
    }
}

$tree = [
    'dirs' => ['tpl', 'tpl/sub', 'tpl/sub/deep', 'tpl2', 'admin', 'admin/sub', 'empty'],
    'files' => [
        'tpl/index.twig' => 'index',
        'tpl/a.twig' => 'tpl a',
        'tpl/sub/a.twig' => 'sub a',
        'tpl/sub/deep/b.twig' => 'deep b',
        'tpl/@at.twig' => 'at',
        'tpl/é.twig' => 'e acute',
        'tpl/sp ace.twig' => 'space',
        'tpl2/index.twig' => 'tpl2 index',
        'tpl2/only2.twig' => 'only2',
        'admin/x.twig' => 'admin x',
        'admin/index.twig' => 'admin index',
        'admin/sub/y.twig' => 'admin y',
        'outside.twig' => 'outside',
        'file.txt' => 'not a directory',
    ],
];

function subst($value)
{
    if (is_string($value)) {
        return str_replace('<ROOT>', ROOT, $value);
    }
    if (is_array($value)) {
        return array_map('subst', $value);
    }

    return $value;
}

function unsubst($value)
{
    if (is_string($value)) {
        return str_replace(ROOT, '<ROOT>', $value);
    }
    if (is_array($value)) {
        return array_map('unsubst', $value);
    }

    return $value;
}

function rm_rf(string $path): void
{
    if (is_link($path) || is_file($path)) {
        unlink($path);

        return;
    }
    if (!is_dir($path)) {
        return;
    }
    foreach (scandir($path) as $entry) {
        if ('.' !== $entry && '..' !== $entry) {
            rm_rf($path.'/'.$entry);
        }
    }
    rmdir($path);
}

function build_tree(array $tree): void
{
    rm_rf(ROOT);
    mkdir(ROOT, 0777, true);
    foreach ($tree['dirs'] as $dir) {
        mkdir(ROOT.'/'.$dir, 0777, true);
    }
    foreach ($tree['files'] as $file => $content) {
        file_put_contents(ROOT.'/'.$file, $content);
    }
    clearstatcache(true);
}

function fs_step(array $step): void
{
    $path = ROOT.'/'.$step['path'];
    $ok = match ($step['fs']) {
        'write' => false !== file_put_contents($path, $step['content']),
        'unlink' => unlink($path),
        'mkdir' => mkdir($path, 0777, true),
        'touch' => touch($path, $step['mtime']),
        'symlink' => symlink(subst($step['target']), $path),
    };
    if (!$ok) {
        throw new \RuntimeException('filesystem step failed: '.json_encode($step));
    }
    clearstatcache(true);
}

function export($value)
{
    if ($value instanceof Source) {
        return ['code' => $value->getCode(), 'name' => $value->getName(), 'path' => $value->getPath()];
    }
    if (is_array($value)) {
        return array_map('strval', array_values($value));
    }

    return $value;
}

function run_step(array &$loaders, array $step): ?array
{
    if (isset($step['fs'])) {
        fs_step($step);

        return null;
    }
    $warnings = [];
    set_error_handler(static function (int $no, string $message) use (&$warnings): bool {
        $warnings[] = $message;

        return true;
    });
    try {
        if (isset($step['new'])) {
            $loaders[$step['new']] = match ($step['class']) {
                'filesystem' => new FilesystemLoader(subst($step['paths'] ?? []), subst($step['root'] ?? null)),
                'array' => new ArrayLoader($step['templates']),
                'chain' => new ChainLoader(array_map(static fn ($id) => $loaders[$id], $step['loaders'])),
                'throwing' => new ThrowingLoader($step['label'], $step['exists'], $step['throws']),
            };
            $expect = ['ok' => null];
        } elseif ('addLoader' === $step['call']) {
            $expect = ['ok' => $loaders[$step['on']]->addLoader($loaders[$step['loader']])];
        } else {
            $expect = ['ok' => export($loaders[$step['on']]->{$step['call']}(...subst($step['args'])))];
        }
    } catch (\Throwable $e) {
        $expect = ['error' => ['class' => get_class($e), 'message' => $e->getMessage()]];
    } finally {
        restore_error_handler();
    }
    if ($warnings) {
        $expect['warnings'] = $warnings;
    }

    return unsubst($expect);
}

/** Shorthands: a FilesystemLoader, a method call, a filesystem step. */
function loader(string $id, $paths, ?string $root = '<ROOT>'): array
{
    return ['new' => $id, 'class' => 'filesystem', 'paths' => $paths, 'root' => $root];
}

function call(string $on, string $method, ...$args): array
{
    return ['call' => $method, 'on' => $on, 'args' => $args];
}

function unix(array $step): array
{
    return $step + ['unix_only' => true];
}

$scenarios = [];

$scenarios[] = ['name' => 'constructor and path inspection', 'steps' => [
    loader('none', []),
    call('none', 'getPaths', '__main__'),
    call('none', 'getNamespaces'),
    call('none', 'getSourceContext', 'index.twig'),
    call('none', 'exists', 'index.twig'),
    call('none', 'getSourceContext', '@__main__/index.twig'),
    loader('abs', ['<ROOT>/tpl/', '<ROOT>/tpl2//']),
    call('abs', 'getPaths', '__main__'),
    call('abs', 'getNamespaces'),
    call('abs', 'getSourceContext', 'index.twig'),
    call('abs', 'getSourceContext', 'only2.twig'),
    loader('rel', ['tpl', 'tpl2/']),
    call('rel', 'getPaths', '__main__'),
    call('rel', 'getSourceContext', 'only2.twig'),
    call('rel', 'getCacheKey', 'only2.twig'),
    call('rel', 'getSourceContext', 'missing.twig'),
    loader('single', 'tpl'),
    call('single', 'getPaths', '__main__'),
    loader('missing_rel', ['missing']),
    loader('missing_abs', ['<ROOT>/missing']),
    loader('not_a_dir', ['file.txt']),
    loader('second_fails', ['tpl', 'missing']),
    loader('nul_root', [], "<ROOT>\0x"),
    loader('nul_path', ["tpl\0x"]),
]];

$scenarios[] = ['name' => 'addPath, prependPath and setPaths', 'steps' => [
    loader('l', []),
    call('l', 'addPath', 'tpl'),
    call('l', 'addPath', 'tpl2/', '__main__'),
    call('l', 'getPaths', '__main__'),
    call('l', 'prependPath', 'admin', 'ns'),
    call('l', 'getNamespaces'),
    call('l', 'prependPath', 'tpl', 'ns'),
    call('l', 'getPaths', 'ns'),
    call('l', 'addPath', 'missing', 'ns2'),
    call('l', 'getNamespaces'),
    call('l', 'prependPath', 'file.txt', 'ns2'),
    call('l', 'addPath', '<ROOT>/missing/', 'ns2', true),
    call('l', 'getNamespaces'),
    call('l', 'addPath', 'missing', 'ns2', false),
    call('l', 'getNamespaces'),
    call('l', 'prependPath', 'missing2//', 'ns2', false),
    call('l', 'addPath', 'tpl\\\\/', 'ns2', false),
    call('l', 'getPaths', 'ns2'),
    call('l', 'prependPath', 'missing3', 'ns2', true),
    call('l', 'getSourceContext', '@ns2/x.twig'),
    call('l', 'getSourceContext', '@ns2/index.twig'),
    call('l', 'setPaths', ['tpl2', 'admin'], 'ns'),
    call('l', 'getPaths', 'ns'),
    call('l', 'getNamespaces'),
    call('l', 'setPaths', 'tpl', 'ns3'),
    call('l', 'getPaths', 'ns3'),
    call('l', 'setPaths', ['tpl', 'missing', 'tpl2'], 'ns4'),
    call('l', 'getPaths', 'ns4'),
    call('l', 'setPaths', [], 'ns5'),
    call('l', 'getPaths', 'ns5'),
    call('l', 'getSourceContext', '@ns5/index.twig'),
    call('l', 'getPaths', 'unknown'),
    call('l', 'getNamespaces'),
    call('l', 'addPath', '/', 'slash'),
    call('l', 'getPaths', 'slash'),
    call('l', 'getSourceContext', '@slash/tpl/index.twig'),
    call('l', 'getCacheKey', '@slash/tpl/index.twig'),
    call('l', 'addPath', '<ROOT>/tpl///', 'trim'),
    call('l', 'getPaths', 'trim'),
    call('l', 'addPath', '<ROOT>', '123'),
    call('l', 'addPath', '<ROOT>', '0123'),
    call('l', 'addPath', '<ROOT>', ''),
    call('l', 'getNamespaces'),
    call('l', 'getSourceContext', '@123/outside.twig'),
    call('l', 'getSourceContext', '@/outside.twig'),
    call('l', 'setPaths', ['tpl'], '__main__'),
    call('l', 'getPaths', '__main__'),
    call('l', 'getNamespaces'),
]];

$scenarios[] = ['name' => 'name normalisation', 'steps' => array_merge(
    [loader('l', ['<ROOT>/tpl']), call('l', 'addPath', '<ROOT>/admin', 'admin')],
    array_map(static fn ($name) => call('l', 'getSourceContext', $name), [
        'index.twig', 'sub\\a.twig', 'sub//a.twig', '/sub/a.twig', '//sub/a.twig', './sub/a.twig', 'sub/./a.twig',
        'sub/deep/../a.twig', 'sub/deep/.././/../sub/a.twig', 'sub/deep/../../index.twig', '@__main__/sub/a.twig',
        '@__main__/./sub/a.twig', '@__main__//index.twig', '@__main__\\index.twig', '@admin/x.twig', '@admin//x.twig',
        '@admin/./x.twig', '@admin/sub/../x.twig', '@admin\\x.twig', '@admin\\sub\\y.twig', '\\@admin\\x.twig',
        './@at.twig', '@__main__/@at.twig', '/@at.twig', '\\@at.twig', '@__main__/./@at.twig', 'sub/../@at.twig',
        '@at.twig', 'é.twig', 'sp ace.twig', './index.twig/', 'index.twig/.', 'sub/a.twig/../a.twig', '@admin/',
        '@admin', '@', '@@x/y', '@/index.twig', 'a/@admin/x.twig', '@__main__/@admin/x.twig', '/@admin/x.twig',
    ]),
    [
        call('l', 'exists', '@admin/x.twig'),
        call('l', 'exists', '/@admin/x.twig'),
        call('l', 'exists', './@admin/x.twig'),
        call('l', 'exists', '@__main__/@admin/x.twig'),
        call('l', 'exists', '@admin'),
        call('l', 'exists', 'sub\\a.twig'),
    ]
)];

$securityNames = [
    "AutoloaderTest\0.php", '..\\AutoloaderTest.php', '..\\\\\\AutoloaderTest.php', '../AutoloaderTest.php',
    '..////AutoloaderTest.php', './../AutoloaderTest.php', '.\\..\\AutoloaderTest.php', '././././././../AutoloaderTest.php',
    '.\\./.\\./.\\./../AutoloaderTest.php', 'foo/../../AutoloaderTest.php', 'foo\\..\\..\\AutoloaderTest.php',
    'foo/../bar/../../AutoloaderTest.php', 'foo/bar/../../../AutoloaderTest.php', 'filters/../../AutoloaderTest.php',
    'filters//..//..//AutoloaderTest.php', 'filters\\..\\..\\AutoloaderTest.php', 'filters\\\\..\\\\..\\\\AutoloaderTest.php',
    'filters\\//../\\/\\..\\AutoloaderTest.php', '/../AutoloaderTest.php', '@__main__/../AutoloaderTest.php',
    '@foo/../AutoloaderTest.php', '@__main__/../../AutoloaderTest.php', '@foo/../../AutoloaderTest.php',
    '..', '../', './..', 'sub/../..', 'sub/../../outside.twig', '@foo/..', '@foo/../outside.twig', '@__main__/..',
    "\0", "a\0b.twig", "sub/a.twig\0", "@foo/a\0", "@fo\0o/a.twig", '..\\outside.twig', 'sub\\..\\..\\outside.twig',
    'errors/../../../etc/passwd',
];
$scenarios[] = ['name' => 'security checks', 'steps' => array_merge(
    [loader('l', ['<ROOT>/tpl']), call('l', 'addPath', '<ROOT>/tpl', 'foo')],
    array_map(static fn ($name) => call('l', 'getCacheKey', $name), $securityNames),
    array_map(static fn ($name) => call('l', 'getSourceContext', $name), $securityNames),
    array_map(static fn ($name) => call('l', 'exists', $name), $securityNames),
    array_map(static fn ($name) => call('l', 'isFresh', $name, 1), ['../outside.twig', "a\0", '@fo/../x']),
    [call('l', 'getSourceContext', '../outside.twig')]
)];

$scenarios[] = ['name' => 'negative lookup cache', 'steps' => [
    loader('l', ['<ROOT>/tpl']),
    call('l', 'getSourceContext', 'new.twig'),
    ['fs' => 'write', 'path' => 'tpl/new.twig', 'content' => 'new'],
    call('l', 'getSourceContext', 'new.twig'),
    call('l', 'exists', 'new.twig'),
    call('l', 'getCacheKey', 'new.twig'),
    call('l', 'isFresh', 'new.twig', 1),
    call('l', 'getSourceContext', './new.twig'),
    call('l', 'getSourceContext', 'new.twig'),
    call('l', 'exists', 'new.twig'),
    call('l', 'exists', 'sub\\new2.twig'),
    call('l', 'getSourceContext', 'sub/new2.twig'),
    call('l', 'getSourceContext', 'sub\\new2.twig'),
    ['fs' => 'write', 'path' => 'tpl/sub/new2.twig', 'content' => 'new2'],
    call('l', 'getSourceContext', 'sub\\new2.twig'),
    call('l', 'exists', 'sub/new2.twig'),
    call('l', 'getSourceContext', 'sub/./new2.twig'),
    call('l', 'exists', 'sub\\new2.twig'),
    call('l', 'getSourceContext', 'sub\\new2.twig'),
    call('l', 'getSourceContext', 'late.twig'),
    ['fs' => 'write', 'path' => 'tpl2/late.twig', 'content' => 'late'],
    call('l', 'addPath', '<ROOT>/tpl2'),
    call('l', 'getSourceContext', 'late.twig'),
    call('l', 'getSourceContext', 'later.twig'),
    ['fs' => 'write', 'path' => 'tpl2/later.twig', 'content' => 'later'],
    call('l', 'addPath', '<ROOT>/missing'),
    call('l', 'getSourceContext', 'later.twig'),
    call('l', 'getSourceContext', 'latest.twig'),
    ['fs' => 'write', 'path' => 'tpl2/latest.twig', 'content' => 'latest'],
    call('l', 'addPath', '<ROOT>/missing', 'other', false),
    call('l', 'getSourceContext', 'latest.twig'),
    // exists() remembers its failure under the normalised name
    call('l', 'exists', 'sub\\late2.twig'),
    ['fs' => 'write', 'path' => 'tpl/sub/late2.twig', 'content' => 'late2'],
    call('l', 'getSourceContext', 'sub/late2.twig'),
    call('l', 'exists', 'sub/late2.twig'),
    call('l', 'exists', 'sub//late2.twig'),
    call('l', 'getSourceContext', 'sub\\late2.twig'),
    call('l', 'exists', 'sub/late2.twig'),
    call('l', 'getSourceContext', 'sub/late2.twig'),
    // prependPath() clears both caches too
    call('l', 'getSourceContext', 'late3.twig'),
    ['fs' => 'write', 'path' => 'tpl/late3.twig', 'content' => 'late3'],
    call('l', 'prependPath', '<ROOT>/admin', 'other2'),
    call('l', 'getSourceContext', 'late3.twig'),
    call('l', 'getSourceContext', 'late4.twig'),
    ['fs' => 'write', 'path' => 'tpl/late4.twig', 'content' => 'late4'],
    call('l', 'prependPath', '<ROOT>/missing', 'other2', false),
    call('l', 'getSourceContext', 'late4.twig'),
]];

$scenarios[] = ['name' => 'positive lookup cache', 'steps' => [
    loader('l', ['<ROOT>/tpl', '<ROOT>/tpl2']),
    call('l', 'getSourceContext', 'index.twig'),
    ['fs' => 'unlink', 'path' => 'tpl/index.twig'],
    call('l', 'exists', 'index.twig'),
    call('l', 'getCacheKey', 'index.twig'),
    call('l', 'getSourceContext', 'index.twig'),
    call('l', 'isFresh', 'index.twig', 0),
    call('l', 'isFresh', 'index.twig', 5),
    call('l', 'isFresh', 'index.twig', -5),
    call('l', 'exists', './index.twig'),
    call('l', 'addPath', '<ROOT>/admin', 'x'),
    call('l', 'getSourceContext', 'index.twig'),
    call('l', 'setPaths', [], 'y'),
    ['fs' => 'unlink', 'path' => 'tpl2/index.twig'],
    call('l', 'exists', 'index.twig'),
    call('l', 'getCacheKey', 'index.twig'),
    call('l', 'setPaths', ['<ROOT>/admin'], 'y'),
    call('l', 'exists', 'index.twig'),
    call('l', 'getSourceContext', 'index.twig'),
    call('l', 'getSourceContext', 'a.twig'),
    ['fs' => 'unlink', 'path' => 'tpl/a.twig'],
    ['fs' => 'mkdir', 'path' => 'tpl/a.twig'],
    unix(call('l', 'getSourceContext', 'a.twig')),
    call('l', 'exists', 'a.twig'),
]];

$scenarios[] = ['name' => 'unregistered namespace cache', 'steps' => [
    loader('l', ['<ROOT>/tpl']),
    call('l', 'getSourceContext', '@ns/x.twig'),
    call('l', 'setPaths', [], 'ns'),
    call('l', 'getSourceContext', '@ns/x.twig'),
    call('l', 'getSourceContext', '@ns/./x.twig'),
    call('l', 'exists', '@ns/x.twig'),
    call('l', 'getNamespaces'),
    call('l', 'addPath', '<ROOT>/admin', 'ns'),
    call('l', 'getSourceContext', '@ns/x.twig'),
    call('l', 'getSourceContext', '@nope/x.twig'),
    call('l', 'exists', '@nope/x.twig'),
    call('l', 'getSourceContext', '@nope/x.twig'),
]];

$scenarios[] = ['name' => 'cache keys and root paths', 'steps' => [
    loader('l', ['<ROOT>/tpl', 'tpl2']),
    call('l', 'addPath', '<ROOT>/admin/../tpl', 'dots'),
    call('l', 'addPath', '<ROOT>', 'rootns'),
    call('l', 'getCacheKey', 'index.twig'),
    call('l', 'getCacheKey', 'only2.twig'),
    call('l', 'getCacheKey', '@dots/sub/a.twig'),
    call('l', 'getSourceContext', '@dots/sub/a.twig'),
    call('l', 'getCacheKey', '@rootns/outside.twig'),
    call('l', 'getCacheKey', '@rootns/tpl/sub/../a.twig'),
    loader('own_root', ['<ROOT>/tpl'], '<ROOT>/tpl/'),
    call('own_root', 'getCacheKey', 'index.twig'),
    loader('other_root', ['<ROOT>/tpl'], '<ROOT>/tpl2'),
    call('other_root', 'getCacheKey', 'index.twig'),
    loader('missing_root', ['<ROOT>/tpl'], '<ROOT>/nope'),
    call('missing_root', 'getCacheKey', 'index.twig'),
    call('missing_root', 'addPath', 'tpl'),
    loader('dotdot_path', ['../tpl'], '<ROOT>/admin'),
    call('dotdot_path', 'getCacheKey', 'index.twig'),
    call('dotdot_path', 'getSourceContext', 'index.twig'),
    loader('dotdot_root', ['tpl'], '<ROOT>/admin/..'),
    call('dotdot_root', 'getCacheKey', 'index.twig'),
    loader('slashes_root', ['tpl'], '<ROOT>//'),
    call('slashes_root', 'getCacheKey', 'index.twig'),
    loader('dot_root', ['tpl'], '<ROOT>/./'),
    call('dot_root', 'getCacheKey', 'index.twig'),
    loader('missing_root_slash', ['x'], '<ROOT>/nope/'),
    loader('file_root', ['tpl'], '<ROOT>/file.txt'),
    loader('file_root_abs', ['<ROOT>/tpl'], '<ROOT>/file.txt'),
    call('file_root_abs', 'getCacheKey', 'index.twig'),
    call('file_root_abs', 'addPath', '.'),
]];

$scenarios[] = ['name' => 'isFresh', 'steps' => [
    ['fs' => 'touch', 'path' => 'tpl/a.twig', 'mtime' => 1700000000],
    ['fs' => 'touch', 'path' => 'tpl/sub/a.twig', 'mtime' => 86400],
    loader('l', ['<ROOT>/tpl']),
    call('l', 'isFresh', 'a.twig', 1700000000),
    call('l', 'isFresh', 'a.twig', 1700000001),
    call('l', 'isFresh', 'a.twig', 1699999999),
    call('l', 'isFresh', 'a.twig', 0),
    call('l', 'isFresh', 'a.twig', -1),
    call('l', 'isFresh', 'a.twig', PHP_INT_MAX),
    call('l', 'isFresh', 'a.twig', PHP_INT_MIN),
    call('l', 'isFresh', 'sub/a.twig', 86400),
    call('l', 'isFresh', 'sub/a.twig', 86401),
    call('l', 'isFresh', 'missing.twig', 1),
    ['fs' => 'touch', 'path' => 'tpl/a.twig', 'mtime' => 1800000000],
    call('l', 'isFresh', 'a.twig', 1700000001),
    call('l', 'isFresh', './a.twig', 1800000001),
]];

$scenarios[] = ['name' => 'empty and unusual names', 'steps' => [
    loader('l', ['<ROOT>/tpl', '<ROOT>/tpl2']),
    call('l', 'getSourceContext', ''),
    call('l', 'getSourceContext', '/'),
    call('l', 'getSourceContext', '.'),
    call('l', 'getSourceContext', '@__main__/'),
    call('l', 'getSourceContext', 'sub'),
    call('l', 'getSourceContext', 'sub/'),
    call('l', 'exists', ''),
    call('l', 'exists', 'sub'),
    unix(call('l', 'getSourceContext', 'index.twig/')),
    call('l', 'getSourceContext', 'index.twig/..'),
    unix(call('l', 'getSourceContext', '...')),
    call('l', 'getSourceContext', 'sub/deep/b.twig'),
    call('l', 'getSourceContext', 'sub/deep/../../a.twig'),
    call('l', 'getCacheKey', ''),
]];

$scenarios[] = ['name' => 'isAbsolutePath seen through addPath', 'steps' => [
    loader('l', []),
    call('l', 'addPath', 'a:b'),
    call('l', 'addPath', 'ab'),
    call('l', 'addPath', 'a:80'),
    call('l', 'addPath', 'a:99999'),
    call('l', 'addPath', 'a:123456'),
    call('l', 'addPath', ':'),
    call('l', 'addPath', ':x'),
    call('l', 'addPath', 'a b:c'),
    call('l', 'addPath', 'tpl:80/x'),
    call('l', 'addPath', 'http://example.com/templates'),
    call('l', 'addPath', 'x', 'ns', false),
    call('l', 'addPath', 'a:b', 'ns', false),
    call('l', 'getSourceContext', '@ns/index.twig'),
    call('l', 'getPaths', 'ns'),
]];

$scenarios[] = ['name' => 'file:// paths', 'steps' => [
    loader('l', []),
    call('l', 'addPath', 'file://<ROOT>/tpl'),
    call('l', 'getPaths', '__main__'),
    call('l', 'getSourceContext', 'index.twig'),
    call('l', 'getCacheKey', 'index.twig'),
    call('l', 'isFresh', 'index.twig', PHP_INT_MAX),
    call('l', 'exists', 'sub/a.twig'),
    call('l', 'addPath', 'FILE://<ROOT>/tpl2/', 'upper'),
    call('l', 'getSourceContext', '@upper/only2.twig'),
    call('l', 'addPath', 'file://remote<ROOT>/tpl', 'remote'),
    call('l', 'addPath', 'file://remote<ROOT>/tpl', 'remote', false),
    call('l', 'getSourceContext', '@remote/index.twig'),
    call('l', 'addPath', 'file://', 'bare'),
    call('l', 'getPaths', 'bare'),
    call('l', 'getSourceContext', '@bare/index.twig'),
    call('l', 'addPath', 'file:', 'colon'),
    loader('root_url', ['tpl'], 'file://<ROOT>'),
    loader('root_url_abs', ['<ROOT>/tpl'], 'file://<ROOT>'),
    call('root_url_abs', 'getCacheKey', 'index.twig'),
]];

$scenarios[] = ['name' => 'file://localhost paths', 'unix_only' => true, 'steps' => [
    loader('l', []),
    call('l', 'addPath', 'file://localhost<ROOT>/admin', 'lh'),
    call('l', 'getSourceContext', '@lh/x.twig'),
    call('l', 'getCacheKey', '@lh/x.twig'),
    call('l', 'addPath', 'file:////<ROOT>/tpl2', 'many'),
    call('l', 'getSourceContext', '@many/only2.twig'),
]];

$scenarios[] = ['name' => 'backslash paths on POSIX', 'unix_only' => true, 'steps' => [
    loader('l', []),
    call('l', 'addPath', '<ROOT>/tpl\\'),
    call('l', 'addPath', 'tpl\\sub'),
    call('l', 'addPath', '\\'),
    call('l', 'addPath', '<ROOT>\\tpl', 'ns', false),
    call('l', 'getSourceContext', '@ns/index.twig'),
    call('l', 'getSourceContext', 'INDEX.twig'),
]];

$scenarios[] = ['name' => 'symlinks', 'unix_only' => true, 'steps' => [
    ['fs' => 'symlink', 'path' => 'tpl/link.twig', 'target' => '../outside.twig'],
    ['fs' => 'symlink', 'path' => 'tpl/abslink.twig', 'target' => '<ROOT>/outside.twig'],
    ['fs' => 'symlink', 'path' => 'tpl/broken.twig', 'target' => 'nowhere.twig'],
    ['fs' => 'symlink', 'path' => 'tpl/linkdir', 'target' => '../admin'],
    ['fs' => 'symlink', 'path' => 'tpl/loop.twig', 'target' => 'loop.twig'],
    ['fs' => 'symlink', 'path' => 'linkroot', 'target' => 'tpl'],
    loader('l', ['<ROOT>/tpl']),
    call('l', 'getSourceContext', 'link.twig'),
    call('l', 'getCacheKey', 'link.twig'),
    call('l', 'getSourceContext', 'abslink.twig'),
    call('l', 'getSourceContext', 'broken.twig'),
    call('l', 'getSourceContext', 'linkdir/x.twig'),
    call('l', 'getSourceContext', 'linkdir/../index.twig'),
    call('l', 'getSourceContext', 'loop.twig'),
    call('l', 'exists', 'linkdir'),
    loader('via_link', ['linkroot']),
    call('via_link', 'getCacheKey', 'index.twig'),
    call('via_link', 'getPaths', '__main__'),
    loader('link_root', ['.'], '<ROOT>/linkroot'),
    call('link_root', 'getCacheKey', 'index.twig'),
    call('link_root', 'addPath', 'sub'),
    call('link_root', 'getCacheKey', 'deep/b.twig'),
    call('link_root', 'addPath', 'missing'),
]];

$scenarios[] = ['name' => 'chain basics', 'steps' => [
    ['new' => 'a1', 'class' => 'array', 'templates' => ['foo' => 'bar']],
    ['new' => 'a2', 'class' => 'array', 'templates' => ['errors/index.html' => 'baz', 'index.twig' => 'array index']],
    loader('fs', ['<ROOT>/tpl']),
    ['new' => 'c', 'class' => 'chain', 'loaders' => ['a1', 'a2', 'fs']],
    call('c', 'getSourceContext', 'foo'),
    call('c', 'getSourceContext', 'errors/index.html'),
    call('c', 'getSourceContext', 'index.twig'),
    call('c', 'getSourceContext', 'sub/a.twig'),
    call('c', 'getSourceContext', 'sub\\a.twig'),
    call('c', 'getSourceContext', 'zzz'),
    call('c', 'getCacheKey', 'foo'),
    call('c', 'getCacheKey', 'sub/a.twig'),
    call('c', 'getCacheKey', 'zzz'),
    call('c', 'isFresh', 'foo', 0),
    call('c', 'isFresh', 'zzz', 0),
    call('c', 'exists', 'foo'),
    call('c', 'exists', 'sub/a.twig'),
    call('c', 'exists', 'zzz'),
    call('c', 'exists', '../outside.twig'),
    call('c', 'getSourceContext', '../outside.twig'),
    ['new' => 'empty', 'class' => 'chain', 'loaders' => []],
    call('empty', 'getSourceContext', 'foo'),
    call('empty', 'getCacheKey', 'foo'),
    call('empty', 'isFresh', 'foo', 0),
    call('empty', 'exists', 'foo'),
    call('empty', 'getSourceContext', ''),
]];

$scenarios[] = ['name' => 'chain error aggregation', 'steps' => [
    ['new' => 't1', 'class' => 'throwing', 'label' => 'First', 'exists' => true, 'throws' => 'loader'],
    ['new' => 't2', 'class' => 'throwing', 'label' => 'Second', 'exists' => true, 'throws' => 'loader_with_line'],
    ['new' => 'skip', 'class' => 'throwing', 'label' => 'Skipped', 'exists' => false, 'throws' => 'loader'],
    ['new' => 'rt', 'class' => 'throwing', 'label' => 'Runtime', 'exists' => true, 'throws' => 'runtime'],
    ['new' => 'a', 'class' => 'array', 'templates' => ['x' => 'X']],
    ['new' => 'c', 'class' => 'chain', 'loaders' => ['skip', 't1', 't2']],
    call('c', 'getSourceContext', 'x'),
    call('c', 'getCacheKey', 'x'),
    call('c', 'isFresh', 'x', 0),
    call('c', 'exists', 'x'),
    ['call' => 'addLoader', 'on' => 'c', 'loader' => 'a'],
    call('c', 'getSourceContext', 'x'),
    call('c', 'getCacheKey', 'x'),
    call('c', 'isFresh', 'x', 0),
    call('c', 'getSourceContext', 'y'),
    ['new' => 'c2', 'class' => 'chain', 'loaders' => ['t1', 'rt', 'a']],
    call('c2', 'getSourceContext', 'x'),
    call('c2', 'getCacheKey', 'x'),
    call('c2', 'isFresh', 'x', 0),
    ['new' => 'c3', 'class' => 'chain', 'loaders' => ['skip']],
    call('c3', 'getSourceContext', 'x'),
    call('c3', 'exists', 'x'),
    ['new' => 'nested', 'class' => 'chain', 'loaders' => ['c', 't1']],
    call('nested', 'getSourceContext', 'y'),
    call('nested', 'getCacheKey', 'y'),
]];

$scenarios[] = ['name' => 'chain exists memo', 'steps' => [
    ['new' => 'a', 'class' => 'array', 'templates' => []],
    ['new' => 'c', 'class' => 'chain', 'loaders' => ['a']],
    call('c', 'exists', 'x'),
    call('a', 'setTemplate', 'x', 'X'),
    call('a', 'exists', 'x'),
    call('c', 'exists', 'x'),
    call('c', 'getSourceContext', 'x'),
    call('c', 'getCacheKey', 'x'),
    ['new' => 'b', 'class' => 'array', 'templates' => []],
    ['call' => 'addLoader', 'on' => 'c', 'loader' => 'b'],
    call('c', 'exists', 'x'),
    call('c', 'exists', 'y'),
    call('b', 'setTemplate', 'y', 'Y'),
    call('c', 'exists', 'y'),
    call('c', 'isFresh', 'y', 0),
]];

$scenarios[] = ['name' => 'nested chains', 'steps' => [
    loader('fs', ['<ROOT>/tpl']),
    ['new' => 'inner', 'class' => 'chain', 'loaders' => ['fs']],
    ['new' => 'outer', 'class' => 'chain', 'loaders' => ['inner']],
    call('outer', 'exists', 'a.twig'),
    call('fs', 'setPaths', ['<ROOT>/empty']),
    call('outer', 'getCacheKey', 'a.twig'),
    call('outer', 'getSourceContext', 'a.twig'),
    call('outer', 'isFresh', 'a.twig', 0),
    call('outer', 'exists', 'a.twig'),
    call('inner', 'exists', 'a.twig'),
    call('fs', 'exists', 'a.twig'),
    call('inner', 'getSourceContext', 'a.twig'),
]];

$scenarios[] = ['name' => 'chain over a deleted file', 'steps' => [
    loader('fs', ['<ROOT>/tpl']),
    ['new' => 'a', 'class' => 'array', 'templates' => ['a.twig' => 'array a']],
    ['new' => 'c', 'class' => 'chain', 'loaders' => ['fs', 'a']],
    call('c', 'getSourceContext', 'a.twig'),
    ['fs' => 'unlink', 'path' => 'tpl/a.twig'],
    call('c', 'getSourceContext', 'a.twig'),
    call('c', 'getCacheKey', 'a.twig'),
    call('c', 'isFresh', 'a.twig', 0),
]];

$output = ['tree' => $tree, 'scenarios' => []];
foreach ($scenarios as $scenario) {
    build_tree($tree);
    $loaders = [];
    foreach ($scenario['steps'] as $i => $step) {
        $expect = run_step($loaders, $step);
        if (null !== $expect) {
            $scenario['steps'][$i]['expect'] = $expect;
        }
    }
    $output['scenarios'][] = $scenario;
}
rm_rf(ROOT);

echo json_encode($output, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR), "\n";

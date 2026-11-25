<?php

/*
 * Generator of tests/vectors/loader/names.json: the pure string functions of Twig 3.31.1's FilesystemLoader
 * (TWIG/src/Loader/FilesystemLoader.php) on PHP 8.5.13, called through reflection:
 *
 *   - isAbsolutePath($file), including the parse_url($file, PHP_URL_SCHEME) part (PHPSRC/ext/standard/url.c);
 *   - normalizeName($name), then parseName() and validateName() as findTemplate() chains them.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/loader/names.gen.php > tests/vectors/loader/names.json
 *
 * Output: {"is_absolute_path": [[path, bool]],
 *          "names": [[name, normalized name, namespace|null, short name|null, LoaderError message|null]]}
 * namespace/short name are null when parseName() throws; the message is the first error of parseName() or
 * validateName($shortname, $name). Every string is valid UTF-8 (names are Rust strings in twig-rs).
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Error\LoaderError;
use Twig\Loader\FilesystemLoader;

/** Every string of length 0..$max over $alphabet, shortest first. */
function words(array $alphabet, int $max): array
{
    $all = [''];
    $level = [''];
    for ($len = 1; $len <= $max; ++$len) {
        $next = [];
        foreach ($level as $prefix) {
            foreach ($alphabet as $char) {
                $next[] = $prefix.$char;
            }
        }
        array_push($all, ...$next);
        $level = $next;
    }

    return $all;
}

$loader = new FilesystemLoader();
$isAbsolute = new ReflectionMethod(FilesystemLoader::class, 'isAbsolutePath');
$normalize = new ReflectionMethod(FilesystemLoader::class, 'normalizeName');
$parse = new ReflectionMethod(FilesystemLoader::class, 'parseName');
$validate = new ReflectionMethod(FilesystemLoader::class, 'validateName');

// isAbsolutePath(): exhaustive short strings over the characters its three rules and parse_url() look at,
// hand-picked URL shapes (ports, IPv6 hosts, user info, file:// and Windows drives) and seeded random strings.
$paths = words(['/', '\\', ':', 'a', 'C', '1', '0', '?', '#', '@', '[', ']', '.', ' '], 4);
$handPicked = [
    'C:/x', 'C:\\x', 'c:/', 'C:', 'C:x', 'CC:/x', '1:/x', 'é:/x', 'é:x', 'aé:x', 'a:é',
    'a:0', 'a:00', 'a:1', 'a:80', 'a:65535', 'a:65536', 'a:99999', 'a:100000', 'a:123456', 'a:1234567', 'a:80/x',
    'a:80?x', 'a:80#x', 'a:8x', 'a:-1', 'a:+1', 'a: 1', 'a:0x1', ':', ':x', ':80', ':80/x', ':99999', '::', ':/',
    '//', '//x', '//x:80', '//x:', '//:80', '//x:65536', '//x:123456', '//x: 1', '//x:+1', '//x:-1', '//x:1a',
    '//u@x', '//u@', '//u:p@x:1', '//[::1]', '//[::1]:80', '//[::1', 'u@:80', 'u@x:80', 'x@y:80/z',
    'http://example.com', 'http://example.com:80/x?y#z', 'http:///x', 'http://', 'http:', 'http:/x',
    'file:', 'file:/x', 'file://', 'file:///x', 'file:///c:/x', 'file://localhost/x', 'FILE:///x', 'phar://x.phar/y',
    'data:text/plain,x', 'mailto:a@b', 'zlib:x', 'a+b.c-d:x', 'a_b:x', 'a b:x', 'a?b:c', 'a#b:c', 'a/b:c', 'a\\b:c',
    'templates', 'templates/', './templates', '../templates', 'templates:', 'tpl:80', 'tpl:8080/x', 'x:/y',
    "a\0:x", "\0", "a:\0", "a:1\0", "//x:\0", "//x:1\0",
];
array_push($paths, ...$handPicked);
mt_srand(20261125);
$alphabet = str_split('a1:/?#@[]. -+9%\\0_');
for ($i = 0; $i < 20000; ++$i) {
    $len = mt_rand(1, 16);
    $s = '';
    for ($j = 0; $j < $len; ++$j) {
        $s .= $alphabet[mt_rand(0, count($alphabet) - 1)];
    }
    $paths[] = $s;
}
$absolute = [];
foreach (array_values(array_unique($paths)) as $path) {
    $absolute[] = [$path, $isAbsolute->invoke($loader, $path)];
}

// Names: exhaustive short names over the characters normalizeName(), parseName() and validateName() look at,
// the same with namespace prefixes, and hand-picked names (Twig's own test names among them).
$names = words(['/', '\\', '.', '@', 'a', "\0"], 5);
foreach (['@__main__/', '@__main__\\', '@ns/', '@ns\\', '/@ns/', './', '../'] as $prefix) {
    foreach (words(['/', '\\', '.', '@', 'a'], 4) as $rest) {
        $names[] = $prefix.$rest;
    }
}
$handPicked = [
    'errors\\index.html', 'errors//index.html', '/errors/index.html', './errors/index.html', 'errors/./index.html',
    'filters/../errors/index.html', 'filters/./..//errors/index.html', '@__main__/errors/index.html',
    '@__main__/./errors/index.html', '@foo/errors/index.html', '@foo//errors/index.html', '@foo/./errors/index.html',
    '@foo/filters/../errors/index.html', '@named/index.html', '/@named/index.html', './@named/index.html',
    '@__main__/@named/index.html', 'errors/../../../etc/passwd', "AutoloaderTest\0.php", '..\\AutoloaderTest.php',
    '..\\\\\\AutoloaderTest.php', '../AutoloaderTest.php', '..////AutoloaderTest.php', './../AutoloaderTest.php',
    '.\\..\\AutoloaderTest.php', '././././././../AutoloaderTest.php', '.\\./.\\./.\\./../AutoloaderTest.php',
    'foo/../../AutoloaderTest.php', 'foo\\..\\..\\AutoloaderTest.php', 'foo/../bar/../../AutoloaderTest.php',
    'foo/bar/../../../AutoloaderTest.php', 'filters/../../AutoloaderTest.php', 'filters//..//..//AutoloaderTest.php',
    'filters\\..\\..\\AutoloaderTest.php', 'filters\\\\..\\\\..\\\\AutoloaderTest.php',
    'filters\\//../\\/\\..\\AutoloaderTest.php', '/../AutoloaderTest.php', '@__main__/../AutoloaderTest.php',
    '@foo/../AutoloaderTest.php', '@__main__/../../AutoloaderTest.php', '@foo/../../AutoloaderTest.php',
    '', '@', '@/', '@/x', '@@x/y', '@a.b/x', '@ /x', "@n\0s/x", "x\0", '@__main__', '@__main__/', '@__main__//',
    '@__main__/@', '@__main__/@x', '@__main__/@x/y', '@__main__/../@x', '@__MAIN__/x', '@__main__x/y',
    '...', '..../x', 'a/.../b', '.hidden', 'a/.hidden', 'a/..hidden', 'a..b', 'index.twig/', './index.twig/',
    'index.twig/.', 'index.twig/..', 'sub/a.twig/../a.twig', 'é.twig', './é/../é.twig', 'sp ace.twig',
    'a/b/c/../../../../d', 'a/b/../../c', 'a/../../c', '@ns/a/../../c', '@ns/a/../b', "\\\\server\\share\\x",
    'C:\\x\\y', 'C:/x', 'file:///x', 'a:b', '@ns/@x', '@ns//@x', '@ns/./@x',
];
array_push($names, ...$handPicked);
$results = [];
foreach (array_values(array_unique($names)) as $name) {
    $normalized = $normalize->invoke($loader, $name);
    $namespace = $short = $error = null;
    try {
        [$namespace, $short] = $parse->invoke($loader, $normalized);
        $validate->invoke($loader, $short, $name);
    } catch (LoaderError $e) {
        $error = $e->getMessage();
    }
    $results[] = [$name, $normalized, $namespace, $short, $error];
}

echo json_encode(
    ['is_absolute_path' => $absolute, 'names' => $results],
    JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR
), "\n";

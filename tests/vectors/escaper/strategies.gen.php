<?php

/*
 * Generator of tests/vectors/escaper/strategies.json: what Twig 3.31.1's EscaperRuntime::escape($string, $strategy,
 * $charset) returns for strings (single bytes, characters, invalid UTF-8, random byte strings) with every built-in
 * strategy and a range of charsets (UTF-8 spellings, the charsets htmlspecialchars() knows, iconv-only and unknown
 * ones), plus FileExtensionEscapingStrategy::guess() for template names.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/escaper/strategies.gen.php > tests/vectors/escaper/strategies.json
 *
 * Output: {"escape": [{s, strategy, charset, out (string, {"b64": ...} or false) | error {class, message},
 * diagnostics: ["Warning: ...", "Notice: ..."]}], "guess": [{name, strategy (string or false)}]}. Warnings and notices
 * are collected (not thrown).
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\FileExtensionEscapingStrategy;
use Twig\Runtime\EscaperRuntime;

function enc(string $s)
{
    return mb_check_encoding($s, 'UTF-8') ? $s : ['b64' => base64_encode($s)];
}

mt_srand(20261125);

function random_string(): string
{
    $pieces = ['a', 'Z', '0', ' ', '<', '>', '&', '"', "'", '/', '\\', ',', '.', '-', '_', ':', '@', '[', ']', '~', '+', '%',
        "\n", "\r", "\t", "\x00", "\x08", "\x0C", "\x1F", "\x7F", 'é', 'Ā', '€', '😀', "\u{2028}", "\u{FEFF}", "\u{0085}",
        "\u{00A0}", "\xC3", "\xE2\x82", "\xFF", "\x80", "\xC0\xAF", "\xED\xA0\x80", 'ü', 'Ж', '中'];
    $out = '';
    $n = mt_rand(1, 12);
    for ($i = 0; $i < $n; ++$i) {
        $out .= $pieces[mt_rand(0, count($pieces) - 1)];
    }

    return $out;
}

$strategies = ['html', 'js', 'css', 'html_attr', 'html_attr_relaxed', 'url'];

$singles = [];
for ($b = 0; $b < 256; ++$b) {
    $singles[] = chr($b);
}
$characters = ['é', 'Ā', '😀', "\u{FFFD}", "\u{2028}", "\u{FEFF}", "\u{10FFFF}", "\u{0080}", "\u{009F}", "\u{00A0}", "\u{07FF}", "\u{0800}",
    "\u{FFFF}", "\u{10000}", '€', 'Ж', '中', "\xC3", "\xE2\x82", "\xF0\x9F\x98", "\xC0\xAF", "\xED\xA0\x80", "\xF4\x90\x80\x80", "a\xFFb",
    "<a href='x'>&é", 'foo bar', "\x00\x01\x1F\x7F", '&amp; &lt;', "\"'<>&", '123', 'abcXYZ', '', "&#039; &quot;", "&nbsp;"];
$random = [];
for ($i = 0; $i < 110; ++$i) {
    $random[] = random_string();
}

$cases = [];
foreach (['UTF-8', 'utf-8', 'ISO-8859-1', 'CP1252', 'BOGUS'] as $charset) {
    foreach ($singles as $s) {
        foreach ($strategies as $strategy) {
            $cases[] = [$s, $strategy, $charset];
        }
    }
}
foreach (['UTF-8', 'utf-8', 'UTF8', 'ISO-8859-1', 'iso-8859-1', 'latin1', 'ISO-8859-15', 'CP1252', 'windows-1252', 'WINDOWS-1251', '1251', 'KOI8-R', 'koi8-ru', 'MACROMAN', 'CP866', 'ISO-8859-5', 'ASCII', 'ISO-8859-2', 'UTF-16', 'UTF-16LE', 'UTF-32', 'BIG5', 'SJIS', 'EUC-JP', 'GB2312', 'BOGUS', 'UTF-8//IGNORE'] as $charset) {
    foreach (array_merge($characters, $random) as $s) {
        foreach ($strategies as $strategy) {
            $cases[] = [$s, $strategy, $charset];
        }
    }
}

$out = ['escape' => [], 'guess' => []];
foreach ($cases as [$s, $strategy, $charset]) {
    $diagnostics = [];
    set_error_handler(function (int $type, string $message) use (&$diagnostics): bool {
        $label = match ($type) {
            E_WARNING => 'Warning',
            E_NOTICE => 'Notice',
            E_DEPRECATED, E_USER_DEPRECATED => 'Deprecated',
            default => 'Error '.$type,
        };
        $diagnostics[] = $label.': '.$message;

        return true;
    });
    $result = ['s' => enc($s), 'strategy' => $strategy, 'charset' => $charset];
    try {
        $value = (new EscaperRuntime('UTF-8'))->escape($s, $strategy, $charset);
        $result['out'] = false === $value ? false : enc($value);
    } catch (\Throwable $e) {
        $result['error'] = ['class' => get_class($e), 'message' => $e->getMessage()];
    }
    restore_error_handler();
    $result['diagnostics'] = array_map('enc', $diagnostics);
    $out['escape'][] = $result;
}

$names = ['foo.html', 'foo.html.twig', 'foo', 'foo.bar.twig', 'foo.txt/foo', 'foo.txt/foo.js/', 'foo.css', 'foo.css.twig',
    'foo.twig.css', 'foo.js.css', 'foo.js.css.twig', 'foo.js', 'foo.js.twig', 'foo.txt/foo.js', 'foo.txt.twig/foo.js', 'foo.txt',
    'foo.txt.twig', 'foo.json', 'foo.JS', 'foo.Txt', '', '.twig', '.js', 'a/.js', 'a\\b.js', 'a\\', 'a/', '/', '///', 'dir/sub/x.css.twig',
    'x.json.twig', 'x.twig.twig', 'x.txt.js', 'x.', 'x..js', '@ns/foo.txt', 'foo.txt//', 'a.b/c', 'twig', 'x.TWIG', 'x.js ', ' .js',
    "x.js\n", 'x.htm', 'x.xml', 'x.css.json'];
foreach ($names as $name) {
    $out['guess'][] = ['name' => $name, 'strategy' => FileExtensionEscapingStrategy::guess($name)];
}

echo json_encode($out, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";

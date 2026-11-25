<?php

/*
 * Oracle vectors for twig_pcre::preg::preg_quote(): PHP 8.5.13's preg_quote(). Output: preg_quote.json next to
 * this file. Regenerate from the repository root (Git Bash):
 *
 *   MSYS_NO_PATHCONV=1 docker run --rm -v "$(cygpath -wa crates/twig-pcre/tests/vectors):/v:ro" \
 *       twig-oracle:3.31.1 php /v/preg_quote.gen.php > crates/twig-pcre/tests/vectors/preg_quote.json
 *
 * Format: {"php": VERSION, "cases": [{"s": STR, "d": null | STR, "out": STR}, ...]}, one case per line; byte
 * strings are JSON strings when valid UTF-8, else {"hex": "..."}.
 */

declare(strict_types=1);

function enc(string $s): string|array
{
    return mb_check_encoding($s, 'UTF-8') ? $s : ['hex' => bin2hex($s)];
}

$cases = [];
$delims = [null, '', '/', '#', '~', 'ab', "\x00", "\xFF", 'a', '.'];
for ($i = 0; $i < 256; ++$i) {
    foreach ([null, '/', chr($i)] as $d) {
        $cases[] = [chr($i), $d];
    }
}
foreach (['', 'Hello.World?', 'a+b*c', '1-2=3', '[x]{y}(z)', 'a#b', 'a/b', 'path/to/file.twig', "a\x00b",
    "\x00\x00", 'Ünïcødé ✓', '*', 'foo_*_bar', 'foo*', '*_*', "\xFF\xFE", '$^|!<>:', 'aaa'] as $s) {
    foreach ($delims as $d) {
        $cases[] = [$s, $d];
    }
}

$lines = [];
foreach ($cases as [$s, $d]) {
    $out = $d === null ? preg_quote($s) : preg_quote($s, $d);
    $lines[] = json_encode(['s' => enc($s), 'd' => $d === null ? null : enc($d), 'out' => enc($out)], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR);
}
echo '{"generator":"preg_quote.gen.php","php":'.json_encode(PHP_VERSION).',"cases":['."\n".implode(",\n", $lines)."\n]}\n";

<?php
// Generates mb_encodings.json: what twig-rs's encoding table for the mbstring callables
// (src/extension/runtime/strings/mb.rs) must contain, observed black-box in PHP 8.5.13:
//
// - "encodings": mb_list_encodings() in its order, each with mb_preferred_mime_name() (null when PHP has none and
//   warns), mb_encoding_aliases(), and "single_byte": whether mb_strlen() and mb_substr() count and cut bytes for it,
//   checked on every byte value, every pair of bytes and seeded random strings;
// - "names": the encoding a name designates, read back with mb_internal_encoding() (null: PHP's ValueError): every
//   listed name, MIME name and alias as listed, lower-cased and upper-cased, and names mbstring does not know;
// - "arguments": whether mb_strlen() accepts a name as its encoding argument (names with NUL bytes, "pass", ...).
//
// Regenerate from the repository root (Git Bash on Windows):
//
//   MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php \
//       < tests/vectors/callables/strings/mb_encodings.gen.php > tests/vectors/callables/strings/mb_encodings.json

declare(strict_types=1);

error_reporting(E_ALL & ~E_DEPRECATED);

/** Byte strings on which a single-byte encoding counts and cuts bytes. */
function samples(): array
{
    $all = '';
    for ($b = 0; $b < 256; $b++) {
        $all .= chr($b);
    }
    $samples = [$all, strrev($all)];
    for ($a = 0; $a < 256; $a++) {
        $pairs = '';
        for ($b = 0; $b < 256; $b++) {
            $pairs .= chr($a).chr($b);
        }
        $samples[] = $pairs;
    }
    mt_srand(2297);
    for ($i = 0; $i < 300; $i++) {
        $s = '';
        $n = mt_rand(0, 40);
        for ($j = 0; $j < $n; $j++) {
            $s .= chr(mt_rand(0, 255));
        }
        $samples[] = $s;
    }
    return $samples;
}

function counts_bytes(string $encoding, array $samples): bool
{
    foreach ($samples as $s) {
        if (mb_strlen($s, $encoding) !== strlen($s)
            || mb_substr($s, 1, 5, $encoding) !== substr($s, 1, 5)
            || mb_substr($s, -7, -2, $encoding) !== substr($s, -7, -2)) {
            return false;
        }
    }
    return true;
}

/** The encoding mb_internal_encoding() reads back after being given $name, null when it rejects the name. */
function resolve(string $name): ?string
{
    try {
        mb_internal_encoding($name);
    } catch (ValueError) {
        return null;
    }
    $resolved = mb_internal_encoding();
    mb_internal_encoding('UTF-8');
    return $resolved;
}

function accepted(string $name): bool
{
    try {
        mb_strlen('a', $name);
        return true;
    } catch (ValueError) {
        return false;
    }
}

$samples = samples();
$encodings = [];
$candidates = [];
foreach (mb_list_encodings() as $name) {
    $mime = @mb_preferred_mime_name($name);
    $aliases = mb_encoding_aliases($name);
    $encodings[] = [
        'name' => $name,
        'mime' => $mime === false ? null : $mime,
        'aliases' => $aliases,
        'single_byte' => counts_bytes($name, $samples),
    ];
    foreach (array_merge([$name], $mime === false ? [] : [$mime], $aliases) as $candidate) {
        array_push($candidates, $candidate, strtolower($candidate), strtoupper($candidate));
    }
}
array_push($candidates, '', ' ', 'UTF_8', 'UTF 8', 'utf8mb4', 'UTF-8 ', ' UTF-8', 'auto', 'pass', 'none', 'x',
    'Latin-1', 'ISO8859-1', 'SJIS-WIN', 'big5-hkscs', 'cp51932', 'eucjp-win', 'MS932', 'GB18030-2022');
$names = [];
foreach (array_values(array_unique($candidates)) as $candidate) {
    $names[] = [bin2hex($candidate), resolve($candidate)];
}
$arguments = [];
foreach (["UTF-8\0junk", "utf8\0", "\0UTF-8", "latin1\0UTF-8", "BASE64\0x", "x\0UTF-8", 'pass', 'auto', '',
    'UTF-8', 'utf_8'] as $candidate) {
    $arguments[] = [bin2hex($candidate), accepted($candidate)];
}
echo json_encode([
    'generator' => 'mb_encodings.gen.php',
    'php' => PHP_VERSION,
    'encodings' => $encodings,
    'names' => $names,
    'arguments' => $arguments,
], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), "\n";

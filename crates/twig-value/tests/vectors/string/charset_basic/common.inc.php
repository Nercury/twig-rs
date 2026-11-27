<?php
// Shared by the charset_basic generators: compact cases for PHP's iconv() (PHP 8.5.13 on glibc 2.41, LC_CTYPE=C.UTF-8
// in the oracle image twig-oracle:3.31.1). Everything here is black-box observation of PHP.
//
// A file is {"generator", "php", "names": [hex], "messages": [hex], "cases": [...]}. A case is
// [from, to, input, result]: from/to index "names", input is the hex input (or an input recipe, see recipe()),
// result is the hex output, or [level, message index] for PHP's false with its one diagnostic ("W" warning, "N"
// notice), or ["E", class, message index] for an exception. Long outputs are [length, fnv1a64 hex].

declare(strict_types=1);
ini_set('memory_limit', '-1');

if (setlocale(LC_CTYPE, '0') !== 'C.UTF-8') {
    fwrite(STDERR, "expected LC_CTYPE=C.UTF-8\n");
    exit(1);
}

$GLOBALS['__diags'] = [];
set_error_handler(static function (int $no, string $msg): bool {
    $GLOBALS['__diags'][] = [$no === E_WARNING ? 'W' : ($no === E_NOTICE ? 'N' : ($no === E_DEPRECATED ? 'D' : (string) $no)), $msg];
    return true;
});

$GLOBALS['__names'] = [];
$GLOBALS['__messages'] = [];

function name_index(string $name): int
{
    $hex = bin2hex($name);
    if (!isset($GLOBALS['__names'][$hex])) {
        $GLOBALS['__names'][$hex] = count($GLOBALS['__names']);
    }
    return $GLOBALS['__names'][$hex];
}

function message_index(string $message): int
{
    $hex = bin2hex($message);
    if (!isset($GLOBALS['__messages'][$hex])) {
        $GLOBALS['__messages'][$hex] = count($GLOBALS['__messages']);
    }
    return $GLOBALS['__messages'][$hex];
}

/** iconv() with its result encoded; $long: outputs as [length, fnv1a64]. */
function convert(string $from, string $to, string $input, bool $long = false): string|array
{
    $GLOBALS['__diags'] = [];
    try {
        $r = iconv($from, $to, $input);
    } catch (\Throwable $e) {
        return ['E', get_class($e), message_index($e->getMessage())];
    }
    $d = $GLOBALS['__diags'];
    if ($r === false && count($d) === 1) {
        return [$d[0][0], message_index($d[0][1])];
    }
    if (is_string($r) && $d === []) {
        return $long ? [strlen($r), hash('fnv1a64', $r)] : bin2hex($r);
    }
    throw new RuntimeException('unexpected iconv() outcome: ' . var_export([$r, $d], true));
}

/** A case for iconv($from, $to, $input). */
function conversion(string $from, string $to, string $input): array
{
    return [name_index($from), name_index($to), bin2hex($input), convert($from, $to, $input)];
}

/**
 * Builds an input from a recipe, a list of segments: ["hex", "e282ac"] (literal bytes) or ["rep", "61", n] (the bytes
 * repeated n times). The Rust test builds the same bytes.
 */
function recipe(array $segments): string
{
    $s = '';
    foreach ($segments as $seg) {
        $s .= match ($seg[0]) {
            'hex' => hex2bin($seg[1]),
            'rep' => str_repeat(hex2bin($seg[1]), $seg[2]),
        };
    }
    return $s;
}

function emit(string $generator, array $cases, array $extra = []): void
{
    echo '{"generator":"', $generator, '","php":"', PHP_VERSION, '"';
    foreach ($extra as $k => $v) {
        echo ',"', $k, '":', json_encode($v, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR);
    }
    // hex keys such as "4153434949" turned into integers in the PHP arrays
    echo ",\n\"names\":", json_encode(array_map('strval', array_keys($GLOBALS['__names'])), JSON_THROW_ON_ERROR);
    echo ",\n\"messages\":", json_encode(array_map('strval', array_keys($GLOBALS['__messages'])), JSON_THROW_ON_ERROR);
    echo ",\n\"cases\":[\n";
    $n = count($cases);
    foreach ($cases as $k => $c) {
        echo json_encode($c, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), $k + 1 < $n ? ",\n" : "\n";
    }
    echo "]}\n";
}

/** Canonical names of the charsets charset_basic converts (without the codepages). */
const CHARSETS = ['UTF-8', 'UTF-16', 'UTF-16BE', 'UTF-16LE', 'UTF-32', 'UTF-32BE', 'UTF-32LE', 'UCS-2', 'UCS-2BE',
    'UCS-2LE', 'UCS-4', 'UCS-4BE', 'UCS-4LE', 'ASCII', 'ISO-8859-1'];

/** The single-byte charsets charset_basic converts with tables (codepage.rs), under one of glibc's names each. */
const CODEPAGES = ['ISO-8859-2', 'ISO-8859-3', 'ISO-8859-4', 'ISO-8859-5', 'ISO-8859-6', 'ISO-8859-7', 'ISO-8859-8',
    'ISO-8859-9', 'ISO-8859-10', 'ISO-8859-11', 'ISO-8859-13', 'ISO-8859-14', 'ISO-8859-15', 'ISO-8859-16', 'CP1250',
    'CP1251', 'CP1252', 'CP1253', 'CP1254', 'CP1255', 'CP1256', 'CP1257', 'CP1258', 'KOI8-R', 'KOI8-U', 'KOI8-RU',
    'CP437', 'CP850', 'CP852', 'CP866', 'MACINTOSH'];

/** The codepages whose decoders combine characters. */
const COMPOSING = ['CP1255', 'CP1258'];

/** The multi-byte charsets charset_basic converts with tables (multibyte.rs), under one of glibc's names each. */
const MULTIBYTES = ['SJIS', 'CP932', 'EUC-JP', 'EUC-JP-MS', 'GBK', 'GB18030', 'EUC-CN', 'BIG5', 'BIG5-HKSCS', 'EUC-KR',
    'UHC', 'JOHAB', 'EUC-TW'];

/** The UTF-8 form of a code point, also beyond U+10FFFF (five- and six-byte forms) and for surrogates. */
function utf8(int $cp): string
{
    if ($cp < 0x80) {
        return chr($cp);
    }
    $len = $cp < 0x800 ? 2 : ($cp < 0x10000 ? 3 : ($cp < 0x200000 ? 4 : ($cp < 0x4000000 ? 5 : 6)));
    $lead = [2 => 0xC0, 3 => 0xE0, 4 => 0xF0, 5 => 0xF8, 6 => 0xFC][$len];
    $s = '';
    for ($i = 1; $i < $len; $i++) {
        $s = chr(0x80 | ($cp & 0x3F)) . $s;
        $cp >>= 6;
    }
    return chr($lead | $cp) . $s;
}

/** Code points in UCS-4 big endian (any value, the raw way to feed glibc beyond Unicode and surrogates). */
function ucs4(int ...$cps): string
{
    return implode('', array_map(static fn (int $c): string => pack('N', $c), $cps));
}

/**
 * One conversion as a token for block hashes (sweep.json, codepoints.json):
 *   "S" . strlen(out) . ":" . out          converted
 *   level . strlen(msg) . ":" . msg         false with one diagnostic (level W or N)
 *   "E" . class . ":" . msg . "\n"         an exception
 */
function token(string $from, string $to, string $input): string
{
    $GLOBALS['__diags'] = [];
    try {
        $r = iconv($from, $to, $input);
    } catch (\Throwable $e) {
        return 'E' . get_class($e) . ':' . $e->getMessage() . "\n";
    }
    $d = $GLOBALS['__diags'];
    if (is_string($r) && $d === []) {
        return 'S' . strlen($r) . ':' . $r;
    }
    if ($r === false && count($d) === 1) {
        return $d[0][0] . strlen($d[0][1]) . ':' . $d[0][1];
    }
    throw new RuntimeException('unexpected iconv() outcome');
}

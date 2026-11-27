<?php
// iso2022_fuzz.json: seeded random conversions from and into the ISO-2022 charsets (iso2022.rs): names in random
// case, suffix variants; into them, characters of every set (also those several sets have), control characters, line
// feeds, G2 characters, language tags, characters to transliterate or skip, long runs (so that designations meet
// E2BIG and the rounds); from them, byte strings built from designations, shifts, pairs of every set, unknown and
// cut-off escape sequences and invalid bytes. The characters whose replacement twig-rs refuses are left out (see
// iso2022_cases.json). Inputs are recipes (see common.inc.php); outputs longer than 256 bytes are [length, fnv1a64].
// Mount the crate like iso2022.gen.php.

require __DIR__ . '/common.inc.php';

mt_srand((int) ($argv[1] ?? 1129));
$count = (int) ($argv[2] ?? 8000);

$iso = ['ISO-2022-JP', 'ISO-2022-JP-2', 'ISO-2022-KR'];
$aliases = ['ISO-2022-JP' => ['iso2022jp', 'csISO2022JP'], 'ISO-2022-JP-2' => ['iso2022jp2', 'CSISO2022JP2'],
    'ISO-2022-KR' => ['iso2022kr', 'csiso2022kr']];
$others = ['UTF-8', 'UTF-16', 'UCS-4', 'ISO-8859-1', 'SJIS', 'EUC-JP', 'EUC-KR', 'GBK', 'ASCII'];
$suffixes = ['', '', '//TRANSLIT', '//IGNORE', '//TRANSLIT//IGNORE', '//ignore', '//translit', '//IGNORE//TRANSLIT'];
$table = file_get_contents(__DIR__ . '/../../../../src/builtins/string/charset_basic/translit_table.rs');
$refused = [];
foreach ($iso as $cs) {
    preg_match('/static ' . strtoupper(str_replace('-', '_', $cs)) . '_UNMODELLED: \[u32; \d+\] = \[(.*?)\];/s', (string) $table, $m);
    preg_match_all('/0x[0-9A-F]+/', $m[1] ?? '', $cps);
    foreach ($cps[0] as $h) {
        $refused[(int) hexdec($h)] = true;
    }
}
$chars = [0x61, 0x7A, 0x20, 0x0A, 0x0D, 0x09, 0x1B, 0x0E, 0x0F, 0x5C, 0x7E, 0xA5, 0x203E, 0xA0, 0xE9, 0xB5, 0x3B1, 0x386,
    0x37A, 0x20AC, 0x3042, 0x30A2, 0x4E00, 0x4E02, 0x554A, 0x2C9, 0xAC00, 0x3131, 0xFF61, 0xFF71, 0x2460, 0x3231, 0xFF5E,
    0x2015, 0xFFE5, 0x80, 0x9F, 0xFFFF, 0x1F600, 0xFB01, 0x2026, 0x2014, 0x100, 0x2C7, 0x2160, 0xE0041, 0xE0001, 0xE007F];
$chars = array_values(array_filter($chars, static fn (int $c): bool => !isset($refused[$c])));
$tagLetters = array_map(static fn (string $c): int => 0xE0000 + ord($c), str_split('jakozhenx-'));
$byteBits = ["\x1b\$B", "\x1b\$@", "\x1b(B", "\x1b(J", "\x1b\$A", "\x1b\$(C", "\x1b\$(D", "\x1b(I", "\x1b.A", "\x1b.F", "\x1bN",
    "\x1b\$)C", "\x0e", "\x0f", "\x1b", "\x1b\$", "\x1b\$(", "\x1bZ", "\x80", "\xff", "\n", "\$\"", "0!", "!!", "a", "\x21\x7f",
    "\x1bNA", "\x1bN\xc1", "+*", "\x7e\x7e"];

$cases = [];
for ($i = 0; $i < $count; $i++) {
    $encoding = mt_rand(0, 1) === 0;
    $isoCs = $iso[mt_rand(0, 2)];
    $name = mt_rand(0, 2) === 0 ? $aliases[$isoCs][mt_rand(0, 1)] : $isoCs;
    if (mt_rand(0, 3) === 0) {
        $name = strtolower($name);
    }
    $long = mt_rand(0, 9) === 0;
    $segments = [];
    if ($encoding) {
        $from = ['UTF-8', 'UCS-4BE', 'UTF-16LE'][mt_rand(0, 2)];
        $to = $name . $suffixes[mt_rand(0, count($suffixes) - 1)];
        for ($n = mt_rand(1, $long ? 8 : 5); $n > 0; $n--) {
            $cps = [];
            for ($k = mt_rand(1, 6); $k > 0; $k--) {
                $cps[] = mt_rand(0, 4) === 0 ? $tagLetters[mt_rand(0, count($tagLetters) - 1)] : $chars[mt_rand(0, count($chars) - 1)];
            }
            $s = match ($from) {
                'UTF-8' => implode('', array_map('utf8', $cps)),
                'UCS-4BE' => ucs4(...$cps),
                'UTF-16LE' => (string) iconv('UCS-4BE', 'UTF-16LE//IGNORE', ucs4(...$cps)),
            };
            if ($s === '') {
                continue;
            }
            $segments[] = $long && mt_rand(0, 2) === 0 ? ['rep', bin2hex($s), mt_rand(100, 4000)] : ['hex', bin2hex($s)];
        }
    } else {
        $from = $name;
        $to = ($others[mt_rand(0, count($others) - 1)]) . $suffixes[mt_rand(0, count($suffixes) - 1)];
        for ($n = mt_rand(1, $long ? 10 : 6); $n > 0; $n--) {
            $s = '';
            for ($k = mt_rand(1, 5); $k > 0; $k--) {
                $s .= mt_rand(0, 5) === 0 ? chr(mt_rand(0, 255)) : $byteBits[mt_rand(0, count($byteBits) - 1)];
            }
            $segments[] = $long && mt_rand(0, 2) === 0 ? ['rep', bin2hex($s), mt_rand(100, 4000)] : ['hex', bin2hex($s)];
        }
    }
    $input = recipe($segments);
    $cases[] = [name_index($from), name_index($to), $segments, convert($from, $to, $input, strlen($input) > 256)];
}
emit('iso2022_fuzz.gen.php', $cases);

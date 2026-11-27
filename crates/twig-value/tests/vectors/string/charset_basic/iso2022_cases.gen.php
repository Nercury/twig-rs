<?php
// iso2022_cases.json: conversions from and into the ISO-2022 charsets (iso2022.rs):
//   - designations and shifts around E2BIG: inputs that switch sets at every character, so that PHP's output buffer
//     (the input length + 32 bytes, growing by the input length) fills up in front of an escape sequence, between it
//     and its character, and before the flush's return to ASCII (ESC ( B, SI);
//   - ISO-2022-KR's header (also of an empty input, and again before a //TRANSLIT replacement during the encoder's
//     first run); ISO-2022-JP-2's G2 sets after line feeds; language tags;
//   - //TRANSLIT replacements and //IGNORE skips in every state; the characters whose replacement twig-rs refuses
//     (translit_table.rs), each on its own (they must give twig-rs' error);
//   - decoding: escape sequences cut off by the end of the input, unknown ones, ESC N without G2, invalid bytes in
//     every set, long inputs with designations around the rounds of 8160 characters and where //IGNORE ends a call
//     early (PHP then skips one input byte, which may fall inside an escape sequence);
//   - conversions between the ISO-2022 charsets and the others.
// Inputs are recipes (see common.inc.php); the outputs of inputs longer than 64 bytes are [length, fnv1a64]. Mount the
// crate like iso2022.gen.php (the refused characters come from src/builtins/string/charset_basic/translit_table.rs).

require __DIR__ . '/common.inc.php';

$cases = [];
$add = static function (string $from, string $to, array $segments) use (&$cases): void {
    $input = recipe($segments);
    $cases[] = [name_index($from), name_index($to), $segments, convert($from, $to, $input, strlen($input) > 64)];
};
$hex = static fn (string $s): array => [['hex', bin2hex($s)]];
$u8 = static fn (int ...$cps): string => implode('', array_map('utf8', $cps));
$tag = static function (string $lang): string {
    $s = pack('N', 0xE0001);
    foreach (str_split($lang) as $ch) {
        $s .= pack('N', 0xE0000 + ord($ch));
    }
    return $s;
};

mt_srand(20261128);
$iso = ['ISO-2022-JP', 'ISO-2022-JP-2', 'ISO-2022-KR'];

// encoding around E2BIG: units that switch sets, repeated n times, from UTF-8, ISO-8859-1 and UCS-2LE
$units = [
    'ISO-2022-JP' => [[0x61, 0x3042], [0xA5, 0x61], [0xA5, 0x5C], [0x3042], [0xA5], [0x3042, 0xA, 0x3042], [0x203E, 0x7E]],
    'ISO-2022-JP-2' => [[0x61, 0x3042], [0xA0], [0xE9, 0x3042], [0xAC00, 0x554A], [0x4E02, 0x61], [0xA0, 0xA], [0x37A, 0xA0],
        [0xFF61, 0x61], [0x3B1, 0x386], [0xA5, 0xA0]],
    'ISO-2022-KR' => [[0x61, 0xAC00], [0xAC00], [0xAC00, 0x20], [0xAC00, 0xA, 0xAC00]],
];
foreach ($units as $cs => $list) {
    foreach ($list as $unit) {
        foreach ([0, 1, 2, 3, 5, 8, 10, 13, 16, 20, 31, 32, 33, 40, 64, 100] as $n) {
            foreach (['UTF-8', 'UCS-2LE'] as $from) {
                $s = $from === 'UTF-8' ? $u8(...$unit) : implode('', array_map(static fn (int $c): string => pack('v', $c), $unit));
                foreach (['', '//IGNORE', '//TRANSLIT'] as $suffix) {
                    $add($from, $cs . $suffix, [['rep', bin2hex($s), $n]]);
                }
            }
            if (max($unit) < 0x100) {
                $add('ISO-8859-1', $cs, [['rep', bin2hex(implode('', array_map('chr', $unit))), $n]]);
                $add('ISO-8859-1', $cs, [['rep', bin2hex(implode('', array_map('chr', $unit))), $n], ['hex', 'a5']]);
            }
        }
    }
}
// ... with a //TRANSLIT replacement or an //IGNORE skip at every position of a short run
$special = [0xE9, 0x20AC, 0xFFFF, 0x2460, 0x3231, 0x3300, 0xFB01, 0xE0041, 0xC0];
foreach ($iso as $cs) {
    foreach ($special as $sp) {
        foreach ([[0x61], [0x3042], [0xAC00], [0xA0], [0xA5], [0x4E02], [0x3042, 0x61]] as $pre) {
            foreach (['', '//TRANSLIT', '//IGNORE', '//TRANSLIT//IGNORE', '//ignore'] as $suffix) {
                $add('UTF-8', $cs . $suffix, $hex($u8(...[...$pre, $sp])));
                $add('UTF-8', $cs . $suffix, $hex($u8($sp, ...$pre)));
                $add('UTF-8', $cs . $suffix, $hex($u8(...[...$pre, $sp, ...$pre])));
            }
        }
        foreach ([1, 2, 10, 33] as $n) {
            $add('UTF-8', "$cs//TRANSLIT", [['rep', bin2hex($u8(0x3042, $sp)), $n]]);
            $add('UTF-8', "$cs//TRANSLIT", [['rep', bin2hex($u8(0xAC00, 0xA, $sp)), $n]]);
        }
    }
    // empty input and only tags (the KR header), every suffix
    foreach (['', '//TRANSLIT', '//IGNORE', '//ignore'] as $suffix) {
        $add('UTF-8', $cs . $suffix, $hex(''));
        $add('UCS-4BE', $cs . $suffix, $hex(pack('N', 0xE0041)));
        $add('UCS-4BE', $cs . $suffix, $hex(pack('N', 0xE0041) . pack('N', 0x61)));
    }
}
// the KR header and the encoder's first run: replacements before and after a round of 8160 characters
foreach ([8158, 8159, 8160, 8161, 16319, 16320] as $n) {
    foreach (['ISO-2022-KR//TRANSLIT', 'ISO-2022-JP//TRANSLIT', 'ISO-2022-JP-2//TRANSLIT'] as $to) {
        $add('UTF-8', $to, [['rep', '61', $n], ['hex', bin2hex($u8(0xE9, 0xAC00, 0xE9))], ['rep', '62', 5]]);
        $add('UTF-8', $to, [['rep', bin2hex($u8(0xAC00)), $n], ['hex', bin2hex($u8(0xE9))]]);
    }
}
// language tags in long runs and around designations
foreach (['zh', 'ko', 'ja', 'en', 'zh-TW', 'jax'] as $lang) {
    foreach ([[0x4E00, 0xE9, 0x3B1], [0xAC00, 0x4E00], [0x3042, 0x4E00, 0x61, 0x4E00], [0xA5, 0x61, 0xA0, 0xA, 0xA0]] as $cps) {
        $in = $tag($lang) . implode('', array_map(static fn (int $c): string => pack('N', $c), $cps));
        $add('UCS-4BE', 'ISO-2022-JP-2', $hex($in));
        $add('UCS-4BE', 'ISO-2022-JP-2', $hex(pack('N', 0xAC00) . $in . pack('N', 0xE007F) . pack('N', 0x4E00)));
        $add('UCS-4BE', 'ISO-2022-JP-2', [['rep', bin2hex($in), 50]]);
    }
}

// the characters whose replacement twig-rs refuses
$table = file_get_contents(__DIR__ . '/../../../../src/builtins/string/charset_basic/translit_table.rs');
foreach ($iso as $cs) {
    $name = strtoupper(str_replace('-', '_', $cs)) . '_UNMODELLED';
    if ($table === false || !preg_match('/static ' . $name . ': \[u32; \d+\] = \[(.*?)\];/s', $table, $m)) {
        throw new RuntimeException("no $name in translit_table.rs");
    }
    preg_match_all('/0x[0-9A-F]+/', $m[1], $cps);
    foreach ($cps[0] as $h) {
        $add('UTF-8', "$cs//TRANSLIT", $hex(utf8((int) hexdec($h))));
    }
}

// decoding: escape sequences cut off, unknown, in every state, invalid bytes
$states = ['ISO-2022-JP' => ['', "\x1b\$B", "\x1b(J", "\x1b\$@"],
    'ISO-2022-JP-2' => ['', "\x1b\$B", "\x1b\$A", "\x1b\$(C", "\x1b\$(D", "\x1b(I", "\x1b.A", "\x1b.F", "\x1b\$B\x1b.A"],
    'ISO-2022-KR' => ['', "\x0e", "\x1b\$)C", "\x1b\$)C\x0e"]];
$tails = ["\x1b", "\x1b\$", "\x1b(", "\x1b\$(", "\x1b.", "\x1bN", "\x1b\$)", "\x1b\$)C", "\x1bNa", "\x1bN\xc1", "\x1bN!", "\x1bN.",
    "\x1b\$Z", "\x1b(Z", "\x1bZZ", "\x1b\x1b\$B", "!", "!!", "!\x80", "\x80", "\xff", "\x0e", "\x0f", "\x0e!!\x0f", "a\x0a!",
    "\$\"", "\$\"a", "\x7f", "\x20", "\x1b\$B\x0a\$\""];
foreach ($states as $cs => $prefixes) {
    foreach ($prefixes as $prefix) {
        foreach ($tails as $tail) {
            foreach (['UTF-8', 'UTF-8//IGNORE', 'UTF-16LE//TRANSLIT', 'UTF-8//ignore'] as $to) {
                $add($cs, $to, $hex($prefix . $tail));
                $add($cs, $to, $hex($prefix . $tail . 'ab'));
            }
        }
    }
}
// long inputs: designations around the rounds, invalid bytes there under //IGNORE
$round = 8160;
foreach (['ISO-2022-JP' => ["\x1b\$B\$\"\x1b(B", "a", "\x1b\$B\$\"", "\$\""], 'ISO-2022-JP-2' => ["\x1b\$A0!\x1b(B", "\x1b.A\x1bN!", "\x1b\$B\$\"", "a"],
    'ISO-2022-KR' => ["\x0e0!\x0f", "a", "\x0e0!0!\x0fa"]] as $cs => $fillers) {
    foreach ($fillers as $filler) {
        foreach ([$round - 2, $round - 1, $round, $round + 1, 2 * $round] as $n) {
            foreach (["\x80", "\x1b\$Z", "\x1b(B\xff", "\x0e\x80\x0f"] as $bad) {
                foreach (['UTF-8//IGNORE', 'ISO-8859-1//IGNORE', 'UTF-16//TRANSLIT', 'UTF-8//ignore'] as $to) {
                    $add($cs, $to, [['rep', bin2hex($filler), intdiv($n, max(1, mb_strlen((string) iconv($cs, 'UTF-8', $filler), 'UTF-8')))],
                        ['hex', bin2hex($bad)], ['rep', bin2hex($filler), 20]]);
                }
            }
        }
        for ($i = 0; $i < 8; $i++) {
            $segments = [];
            for ($k = mt_rand(2, 6); $k > 0; $k--) {
                $segments[] = ['rep', bin2hex($filler), mt_rand(0, 9000)];
                $segments[] = ['hex', bin2hex(["\x80", "\x1b\$B", "\x1b(B", "\x0e", "\x0f", "\x1b(J", "\x1b"][mt_rand(0, 6)])];
            }
            $add($cs, ['UTF-8', 'UTF-8//IGNORE', 'UTF-16', 'EUC-JP//TRANSLIT'][mt_rand(0, 3)], $segments);
        }
    }
}

// between the ISO-2022 charsets and the others
$samples = ["a\u{3042}\u{4e00}b", "\u{ac00}\u{554a}\u{e9}", "\u{a5}\u{203e}\\~", "\u{ff71}\u{a0}\u{386}", "\u{20ac}\u{2460}", "\n\u{3042}\r\n"];
foreach ($iso as $cs) {
    foreach (array_merge($iso, ['SJIS', 'EUC-JP', 'EUC-KR', 'GBK', 'UTF-16', 'ISO-8859-1']) as $to) {
        foreach ($samples as $s) {
            $enc = iconv('UTF-8', "$cs//IGNORE", $s);
            if ($enc === false) {
                continue;
            }
            foreach (['', '//TRANSLIT', '//IGNORE'] as $suffix) {
                $add($cs, $to . $suffix, $hex($enc));
            }
        }
    }
}

emit('iso2022_cases.gen.php', $cases);

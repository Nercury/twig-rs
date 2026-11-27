<?php
// multibytes_cases.json: conversions from and into the multi-byte charsets (multibyte.rs):
//   - every multi-byte charset into every multi-byte charset, UTF-8, UTF-16, ISO-8859-1, ASCII and CP1252, plain,
//     //TRANSLIT and //IGNORE, on a pool of inputs per source (characters of every kind of sequence, invalid and cut
//     off sequences, seeded random strings);
//   - UTF-8 into every multi-byte charset on a pool of characters, with every suffix;
//   - the irregular sequences: UHC's A2 E8 and EUC-JP-MS's 8E E0..FF under //IGNORE, where the input ends and in the
//     middle; sequences cut off by the end of the input (GB18030's and EUC-TW's four bytes, EUC-JP's three);
//   - BIG5-HKSCS: the characters its encoder holds back (Ê, ê) before what may combine with them, before characters
//     the target cannot represent, at the end (PHP's flush) and around E2BIG; the cells that decode to two characters
//     around the rounds and where PHP's output buffer is full between the two;
//   - long inputs: characters of every length around the rounds of 8160 characters and around the points where
//     PHP's output buffer grows (E2BIG), invalid and cut-off sequences there with //IGNORE (where a call ends early
//     and PHP skips an input byte), multi-byte targets of //TRANSLIT replacements.
// Inputs are recipes (see common.inc.php); the outputs of inputs longer than 64 bytes are [length, fnv1a64].

require __DIR__ . '/common.inc.php';

$cases = [];
$add = static function (string $from, string $to, array $segments) use (&$cases): void {
    $input = recipe($segments);
    $cases[] = [name_index($from), name_index($to), $segments, convert($from, $to, $input, strlen($input) > 64)];
};
$hex = static fn (string $s): array => [['hex', bin2hex($s)]];

mt_srand(20261127);

/** Valid sequences of a charset: every single-byte character and a seeded sample of the longer ones. */
function valid_sequences(string $cs, int $sample): array
{
    static $cache = [];
    if (!isset($cache[$cs])) {
        $single = [];
        $long = [];
        for ($b = 0; $b < 256; $b++) {
            $GLOBALS['__diags'] = [];
            if (iconv($cs, 'UCS-4BE', chr($b)) !== false) {
                $single[] = chr($b);
            }
        }
        // the code points the charset encodes, sampled: their sequences
        for ($cp = 0x80; $cp < 0x30000; $cp += $cp < 0x10000 ? 7 : 211) {
            $GLOBALS['__diags'] = [];
            $r = iconv('UCS-4BE', $cs, pack('N', $cp));
            if ($r !== false && strlen($r) > 1) {
                $long[] = $r;
            }
        }
        $cache[$cs] = [$single, $long];
    }
    [$single, $long] = $cache[$cs];
    $out = [];
    for ($i = 0; $i < $sample; $i++) {
        $out[] = $long[mt_rand(0, count($long) - 1)];
    }
    return [$single, $out];
}

/** Invalid and cut-off sequences of a charset (as the probe found them; a few per kind). */
const INVALID = [
    'SJIS' => ["\x80", "\xa0", "\xff", "\x81\x20", "\x81\x7f", "\x85\x40", "\x81"],
    'CP932' => ["\x80", "\xfd", "\x81\x20", "\x85\x40", "\x84\xc0", "\x87\x5e", "\x81"],
    'EUC-JP' => ["\xff", "\xa1\x41", "\xa9\xa1", "\x8e\xe0", "\x8f\xa1\xa1", "\x8f\xb0\x41", "\x8f\xb0", "\x8f", "\xa1"],
    'EUC-JP-MS' => ["\xff", "\xa1\x41", "\xa9\xa1", "\x8e\xe0", "\x8e\xff", "\x8f\xa1\xa1", "\x8f\xf3\xa1", "\x8f\xb0", "\xa1"],
    'GBK' => ["\xff", "\x81\x20", "\x81\x7f", "\xa2\xa0", "\xfe\xa1", "\x81"],
    'GB18030' => ["\x80", "\xff", "\x81\x20", "\x81\x7f", "\x81\x30\x20\x30", "\x81\x30\x81\x20", "\x84\x31\xa5\x30",
        "\xe3\x32\x9a\x36", "\x81\x30\x81", "\x81\x30", "\x81"],
    'EUC-CN' => ["\x80", "\xff", "\xa1\x41", "\xa2\xa1", "\x8e\xa1", "\xa1"],
    'BIG5' => ["\x81", "\xff", "\xa1\x20", "\xa1\x80", "\xa3\xc0", "\xa1"],
    'BIG5-HKSCS' => ["\xff", "\x81\x40", "\x87\x66", "\x88\xab", "\xa1\x20", "\x88"],
    'EUC-KR' => ["\xa0", "\xa1\x41", "\xad\xa1", "\xa2\xe9", "\xff\xa1", "\xa1"],
    'UHC' => ["\x80", "\xc9", "\x81\x20", "\xa2\xe9", "\xc7\x41", "\xa2\xe8", "\x81"],
    'JOHAB' => ["\x80", "\xd4", "\x84\x42", "\x88\x72", "\xd9\x30", "\xda\xa1", "\x84"],
    'EUC-TW' => ["\x80", "\xff", "\xa1\x41", "\xa8\xa1", "\x8e\xa1\xa1\x41", "\x8e\xa8\xa1\xa1", "\x8e\xb1", "\x8e\xa2\xa1", "\x8e"],
];

// every multi-byte charset into every multi-byte charset and the main others
$targets = array_merge(MULTIBYTES, ['UTF-8', 'UTF-16', 'ISO-8859-1', 'ASCII', 'CP1252']);
foreach (MULTIBYTES as $from) {
    [$single, $long] = valid_sequences($from, 60);
    $pool = [implode('', $single), implode('', $long)];
    for ($i = 0; $i < 3; $i++) {
        $s = '';
        for ($n = mt_rand(2, 12); $n > 0; $n--) {
            $s .= mt_rand(0, 2) === 0 ? $single[mt_rand(0, count($single) - 1)] : $long[mt_rand(0, count($long) - 1)];
        }
        $pool[] = $s;
    }
    $bad = 'a';
    foreach (INVALID[$from] as $k => $x) {
        if ($k < count(INVALID[$from]) - 1) {
            $bad .= $x . $long[$k] . 'b';
        }
    }
    $pool[] = $bad;
    $pool[] = $long[0] . INVALID[$from][count(INVALID[$from]) - 1];
    foreach ($targets as $to) {
        foreach (['', '//TRANSLIT', '//IGNORE'] as $suffix) {
            foreach ($pool as $input) {
                $add($from, $to . $suffix, $hex($input));
            }
        }
    }
    // every invalid sequence alone, before a character and at the end, with every suffix
    foreach (INVALID[$from] as $x) {
        foreach (['UTF-8', 'UTF-8//IGNORE', 'UTF-8//ignore', 'UTF-16//TRANSLIT', 'UCS-4BE//IGNORE//TRANSLIT'] as $to) {
            $add($from, $to, $hex($x));
            $add($from, $to, $hex($x . 'ab'));
            $add($from, $to, $hex('a' . $x));
            $add($from, $to, $hex($long[1] . $x . $long[2] . $x));
        }
    }
}

// UTF-8 into every multi-byte charset
$samples = [[0x61, 0xE9, 0x62], [0x20AC], [0xA5, 0x5C, 0x203E, 0x7E], [0xA0], [0xBD], [0x2026], [0x2014, 0x2015, 0x2212, 0xFF0D],
    [0x301C, 0xFF5E], [0x2016, 0x2225], [0xA2, 0xA3, 0xAC, 0xFFE0, 0xFFE1, 0xFFE2], [0x3042, 0x30A2, 0xFF71], [0x4E00, 0x4E02],
    [0xAC00, 0xD7A3, 0x3131], [0x20A9, 0xFFE6], [0x2460, 0x2160, 0x3231], [0x3300, 0x339F, 0x2116], [0xF900, 0xFA6C], [0x5341, 0x5345],
    [0xCA, 0x304], [0xEA, 0x30C], [0xE000, 0xE757, 0xF8F0], [0x80, 0x10000, 0x1F600, 0x20089, 0x2A6D6], [0xE5E5, 0x1E3F, 0xFFFD],
    [0x9FA6, 0x9FBB, 0xFE10, 0xFE19], [0xE0041, 0x61], [0xFEFF, 0x61], [0x200B], [0x0300, 0x0301], [0xFB01], [0x10FFFF], [0x110000], [0xD800]];
foreach (MULTIBYTES as $to) {
    foreach (['', '//TRANSLIT', '//IGNORE', '//TRANSLIT//IGNORE', '//ignore'] as $suffix) {
        foreach ($samples as $cps) {
            $add('UTF-8', $to . $suffix, $hex(implode('', array_map('utf8', $cps))));
        }
        $add('UCS-4BE', $to . $suffix, $hex(ucs4(0x61, 0xD800, 0x7FFFFFFF, 0x62)));
    }
}

// the irregular sequences
foreach (['UHC' => ["\xa2\xe8"], 'EUC-JP-MS' => ["\x8e\xe0", "\x8e\xf0", "\x8e\xff"]] as $cs => $list) {
    foreach ($list as $x) {
        foreach (['', 'a', 'ab', 'abc', "\xb0\xa1", "\xb0\xa1\xb0\xa1", "\xa2\xe8", "\x8e\xe0"] as $after) {
            foreach (['', 'z', "\xb0\xa1"] as $before) {
                foreach (['UTF-8', 'UTF-8//IGNORE', 'UTF-8//ignore', 'UTF-16//IGNORE//TRANSLIT', 'ISO-8859-1//IGNORE'] as $to) {
                    $add($cs, $to, $hex($before . $x . $after));
                }
            }
        }
    }
}
// sequences cut off by the end
foreach (['GB18030' => ["\x81", "\x81\x30", "\x81\x30\x81", "\x90\x30\x81", "\x81\x30\x20", "\xfe\x39\xfe"],
    'EUC-TW' => ["\x8e", "\x8e\xa2", "\x8e\xa2\xa1", "\x8e\xa1\xa1", "\x8e\xb1"],
    'EUC-JP' => ["\x8f", "\x8f\xb0", "\x8f\xa1"], 'EUC-JP-MS' => ["\x8f", "\x8f\xb0", "\x8f\xa1", "\x8f\xff"],
    'SJIS' => ["\x81", "\xea"], 'BIG5-HKSCS' => ["\x88", "\x81"]] as $cs => $list) {
    foreach ($list as $x) {
        foreach (['UTF-8', 'UTF-8//IGNORE', 'UTF-8//TRANSLIT', 'UTF-8//ignore'] as $to) {
            $add($cs, $to, $hex($x));
            $add($cs, $to, $hex('ab' . $x));
            $add($cs, $to, [['rep', '61', 8159], ['hex', bin2hex($x)]]);
            $add($cs, $to, [['rep', '61', 8160], ['hex', bin2hex($x)], ['rep', '62', 10]]);
        }
    }
}

// BIG5-HKSCS: the characters its encoder holds back
$hk = [0xCA, 0xEA];
$after = [null, 0x304, 0x30C, 0x61, 0xCA, 0xEA, 0xFFFF, 0xE0041, 0x4E00, 0xE9, 0x300];
foreach (['UCS-4BE', 'UTF-8', 'ISO-8859-1'] as $from) {
    foreach (['BIG5-HKSCS', 'BIG5-HKSCS//IGNORE', 'BIG5-HKSCS//TRANSLIT', 'BIG5-HKSCS//TRANSLIT//IGNORE', 'big5hkscs//ignore'] as $to) {
        foreach ($hk as $h) {
            foreach ($after as $a) {
                foreach ($after as $b) {
                    $cps = array_values(array_filter([$h, $a, $b], static fn (?int $c): bool => $c !== null));
                    if ($from === 'ISO-8859-1' && max($cps) > 0xFF) {
                        continue;
                    }
                    $in = match ($from) {
                        'UCS-4BE' => ucs4(...$cps),
                        'UTF-8' => implode('', array_map('utf8', $cps)),
                        'ISO-8859-1' => implode('', array_map('chr', $cps)),
                    };
                    $add($from, $to, $hex($in));
                }
            }
        }
    }
}
// ... where PHP's buffer is full: Latin-1 Ê (one byte) is two bytes in BIG5-HKSCS, 'a' one; the buffer starts at the
// input length + 32 bytes and grows by the input length
foreach ([0, 1, 2, 3, 30, 31, 32, 33, 34] as $n) {
    foreach ([0, 1, 2, 5] as $tail) {
        foreach (['BIG5-HKSCS', 'BIG5-HKSCS//IGNORE', 'BIG5-HKSCS//TRANSLIT'] as $to) {
            $add('ISO-8859-1', $to, [['rep', 'ca', $n], ['rep', '61', $tail]]);
            $add('ISO-8859-1', $to, [['rep', '61', $tail], ['rep', 'ca', $n]]);
            $add('ISO-8859-1', $to, [['rep', 'cae9', $n], ['hex', 'ca']]);
            $add('UTF-8', $to, [['rep', bin2hex("\u{ca}\u{304}"), $n], ['rep', '61', $tail], ['hex', 'c38a']]);
        }
    }
}
// the cells that decode to two characters: around the rounds and where PHP's buffer is full between the two (UCS-4BE
// writes four bytes per character from one input byte)
foreach ([8157, 8158, 8159, 8160, 8161, 16318, 16319, 16320] as $n) {
    foreach (['UCS-4BE', 'UTF-8//IGNORE', 'ISO-8859-1//TRANSLIT', 'ISO-8859-1//IGNORE', 'ASCII'] as $to) {
        $add('BIG5-HKSCS', $to, [['rep', '61', $n], ['hex', '8862'], ['rep', '62', 3]]);
    }
}
for ($n = 0; $n < 24; $n++) {
    foreach (['UCS-4BE', 'UTF-16', 'UCS-4BE//IGNORE'] as $to) {
        $add('BIG5-HKSCS', $to, [['rep', '61', $n], ['hex', '8862']]);
        $add('BIG5-HKSCS', $to, [['rep', '61', $n], ['rep', '88628864', 3], ['hex', '61']]);
        $add('BIG5-HKSCS', $to, [['rep', '8862', $n]]);
    }
}

// long inputs: characters of every length around the rounds, invalid and cut-off sequences there
$round = 8160;
foreach (MULTIBYTES as $cs) {
    [$single, $long] = valid_sequences($cs, 8);
    $fillers = ['61', bin2hex($long[0]), bin2hex($long[1] . 'a')];
    $longest = '';
    foreach ($long as $l) {
        if (strlen($l) > strlen($longest)) {
            $longest = $l;
        }
    }
    $fillers[] = bin2hex($longest);
    $specials = array_map('bin2hex', array_slice(INVALID[$cs], 0, 4));
    $specials[] = bin2hex($long[2]);
    foreach ($fillers as $f => $filler) {
        $perFiller = $f === 2 ? 2 : 1;
        foreach ([$round - 2, $round - 1, $round, $round + 1, 2 * $round - 1] as $chars) {
            $count = intdiv($chars, $perFiller);
            foreach ($specials as $special) {
                foreach (['UTF-8//IGNORE', 'ISO-8859-1//IGNORE', 'UTF-16//TRANSLIT//IGNORE'] as $to) {
                    $add($cs, $to, [['rep', $filler, $count], ['hex', $special], ['rep', '62', 20]]);
                }
                $add($cs, 'UTF-8//ignore', [['hex', $special], ['rep', $filler, $count], ['hex', $special]]);
            }
        }
        for ($i = 0; $i < 6; $i++) {
            $segments = [];
            for ($n = mt_rand(2, 6); $n > 0; $n--) {
                $segments[] = ['rep', $filler, mt_rand(0, 9000)];
                $segments[] = ['hex', $specials[mt_rand(0, count($specials) - 1)]];
            }
            $add($cs, ['UTF-8', 'UTF-8//IGNORE', 'UCS-2//TRANSLIT', 'ASCII//TRANSLIT//IGNORE', 'EUC-KR//IGNORE'][mt_rand(0, 4)], $segments);
        }
    }
    // E2BIG: into UTF-32 (four bytes per character) and from ASCII and Latin-1 into the charset (a character of up
    // to four bytes per input byte)
    for ($i = 0; $i < 10; $i++) {
        $segments = [];
        for ($n = mt_rand(2, 6); $n > 0; $n--) {
            $segments[] = ['rep', $fillers[mt_rand(0, count($fillers) - 1)], mt_rand(1, 3000)];
            if (mt_rand(0, 1) === 1) {
                $segments[] = ['hex', $specials[mt_rand(0, count($specials) - 1)]];
            }
        }
        $add($cs, ['UTF-32', 'UCS-4BE//IGNORE', 'UTF-32//TRANSLIT'][mt_rand(0, 2)], $segments);
    }
    foreach ([30, 31, 32, 33, 34, 100, 8159, 8160, 8161] as $n) {
        $add('ISO-8859-1', $cs, [['rep', 'a9', $n]]);
        $add('ISO-8859-1', "$cs//TRANSLIT", [['rep', '61', $n], ['rep', 'bd', $n]]);
        $add('UTF-8', "$cs//IGNORE", [['rep', bin2hex("\u{20ac}\u{e9}"), $n]]);
        $add('UTF-8', "$cs//TRANSLIT", [['rep', bin2hex("\u{3300}\u{fb01}"), $n]]);
    }
}

emit('multibytes_cases.gen.php', $cases);

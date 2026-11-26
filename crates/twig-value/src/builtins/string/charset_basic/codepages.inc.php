<?php
// The single-byte charsets charset_basic converts with tables (codepage.rs), shared by codepage.gen.php,
// translit.gen.php and names.gen.php: the `Codepage` variant, the name glibc's `iconv -l` lists for it (any of its
// aliases would do: names.gen.php finds the others) and the unicode.org mapping file it is compared with
// (under https://www.unicode.org/Public/MAPPINGS/; null: there is none).

declare(strict_types=1);

const CODEPAGES = [
    'Iso8859_2' => ['ISO-8859-2', 'ISO8859/8859-2.TXT'],
    'Iso8859_3' => ['ISO-8859-3', 'ISO8859/8859-3.TXT'],
    'Iso8859_4' => ['ISO-8859-4', 'ISO8859/8859-4.TXT'],
    'Iso8859_5' => ['ISO-8859-5', 'ISO8859/8859-5.TXT'],
    'Iso8859_6' => ['ISO-8859-6', 'ISO8859/8859-6.TXT'],
    'Iso8859_7' => ['ISO-8859-7', 'ISO8859/8859-7.TXT'],
    'Iso8859_8' => ['ISO-8859-8', 'ISO8859/8859-8.TXT'],
    'Iso8859_9' => ['ISO-8859-9', 'ISO8859/8859-9.TXT'],
    'Iso8859_10' => ['ISO-8859-10', 'ISO8859/8859-10.TXT'],
    'Iso8859_11' => ['ISO-8859-11', 'ISO8859/8859-11.TXT'],
    'Iso8859_13' => ['ISO-8859-13', 'ISO8859/8859-13.TXT'],
    'Iso8859_14' => ['ISO-8859-14', 'ISO8859/8859-14.TXT'],
    'Iso8859_15' => ['ISO-8859-15', 'ISO8859/8859-15.TXT'],
    'Iso8859_16' => ['ISO-8859-16', 'ISO8859/8859-16.TXT'],
    'Cp1250' => ['CP1250', 'VENDORS/MICSFT/WINDOWS/CP1250.TXT'],
    'Cp1251' => ['CP1251', 'VENDORS/MICSFT/WINDOWS/CP1251.TXT'],
    'Cp1252' => ['CP1252', 'VENDORS/MICSFT/WINDOWS/CP1252.TXT'],
    'Cp1253' => ['CP1253', 'VENDORS/MICSFT/WINDOWS/CP1253.TXT'],
    'Cp1254' => ['CP1254', 'VENDORS/MICSFT/WINDOWS/CP1254.TXT'],
    'Cp1255' => ['CP1255', 'VENDORS/MICSFT/WINDOWS/CP1255.TXT'],
    'Cp1256' => ['CP1256', 'VENDORS/MICSFT/WINDOWS/CP1256.TXT'],
    'Cp1257' => ['CP1257', 'VENDORS/MICSFT/WINDOWS/CP1257.TXT'],
    'Cp1258' => ['CP1258', 'VENDORS/MICSFT/WINDOWS/CP1258.TXT'],
    'Koi8R' => ['KOI8-R', 'VENDORS/MISC/KOI8-R.TXT'],
    'Koi8U' => ['KOI8-U', 'VENDORS/MISC/KOI8-U.TXT'],
    'Koi8Ru' => ['KOI8-RU', null],
    'Cp437' => ['CP437', 'VENDORS/MICSFT/PC/CP437.TXT'],
    'Cp850' => ['CP850', 'VENDORS/MICSFT/PC/CP850.TXT'],
    'Cp852' => ['CP852', 'VENDORS/MICSFT/PC/CP852.TXT'],
    'Cp866' => ['CP866', 'VENDORS/MICSFT/PC/CP866.TXT'],
    'Macintosh' => ['MACINTOSH', 'VENDORS/APPLE/ROMAN.TXT'],
];

// iconv() fails with a notice on input it cannot convert; the generators look at the result only
set_error_handler(static fn (): bool => true);

/** The code points of a one-byte input in charset $cs, null when glibc rejects it (one call, PHP's flush included). */
function decode_byte(string $cs, int $byte): ?array
{
    $r = iconv($cs, 'UCS-4BE', chr($byte));
    return $r === false ? null : array_values(unpack('N*', $r) ?: []);
}

/** The bytes code point $cp encodes to in charset $cs, null when glibc cannot represent it. */
function encode_cp(string $cs, int $cp): ?string
{
    $r = iconv('UCS-4BE', $cs, pack('N', $cp));
    return $r === false ? null : $r;
}

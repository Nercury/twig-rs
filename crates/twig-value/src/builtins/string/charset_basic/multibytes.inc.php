<?php
// The multi-byte charsets charset_basic converts with tables (multibyte.rs), shared by multibyte.gen.php,
// translit.gen.php and names.gen.php: the `Multibyte` variant and the name glibc's `iconv -l` lists for it (any of its
// aliases would do: names.gen.php finds the others).

declare(strict_types=1);

const MULTIBYTES = [
    'Sjis' => 'SJIS', 'Cp932' => 'CP932', 'EucJp' => 'EUC-JP', 'EucJpMs' => 'EUC-JP-MS', 'Gbk' => 'GBK',
    'Gb18030' => 'GB18030', 'EucCn' => 'EUC-CN', 'Big5' => 'BIG5', 'Big5Hkscs' => 'BIG5-HKSCS', 'EucKr' => 'EUC-KR',
    'Uhc' => 'UHC', 'Johab' => 'JOHAB', 'EucTw' => 'EUC-TW',
];

/** The stateful ISO-2022 charsets charset_basic converts (iso2022.rs): the `Iso2022` variant and glibc's name. */
const ISO2022S = ['Jp' => 'ISO-2022-JP', 'Jp2' => 'ISO-2022-JP-2', 'Kr' => 'ISO-2022-KR'];

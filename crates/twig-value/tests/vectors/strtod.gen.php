<?php
// Generates strtod.json: (float) $s, i.e. zend_strtod(), for decimal strings that stress the conversion:
// long mantissas (up to 800 digits), exponents up to +-330 and beyond, subnormals, 17-20 significant digit
// strings close to halfway points, leading zeros and the 19999 exponent clamp.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/strtod.gen.php > strtod.json
//
// cases: [input string (ASCII), IEEE bits of (float) $input (hex, big-endian)]

declare(strict_types=1);
mt_srand(5318008);

$digits = static function (int $n, bool $nonzeroFirst = true): string {
    $s = $nonzeroFirst ? (string) mt_rand(1, 9) : (string) mt_rand(0, 9);
    for ($i = 1; $i < $n; $i++) {
        $s .= (string) mt_rand(0, 9);
    }
    return $s;
};
$sign = static fn (): string => ['', '', '-', '+'][mt_rand(0, 3)];

$inputs = ['0', '-0', '0.0', '-0.0e10', '1', '1.5', '1e308', '1.7976931348623157e308', '1.7976931348623158e308',
    '1.7976931348623159e308', '2e308', '4.9e-324', '5e-324', '2.4703282292062327e-324', '2.4703282292062328e-324',
    '2.2250738585072011e-308', '2.2250738585072014e-308', '1e-400', '1e400', '9007199254740993', '9007199254740992.5',
    '9007199254740993.0000000000000000001', '0.1', '0.30000000000000004', '123456789012345678901234567890',
    '1e19999', '1e-19999', '1e20000', '1e99999999', '1e-99999999', '0.'.str_repeat('0', 30000).'1e30005',
    '1'.str_repeat('0', 400).'e-400', '0.'.str_repeat('0', 400).'1e400', str_repeat('9', 800), '.'.str_repeat('9', 800),
    '00000000000000000000001e5', '1.e5', '.5e-3', '-.5E+3'];

// Long mantissas, optional decimal point, wide exponents.
for ($i = 0; $i < 1500; $i++) {
    $m = $digits(mt_rand(20, 800));
    if (mt_rand(0, 1)) {
        $p = mt_rand(1, strlen($m));
        $m = substr($m, 0, $p).'.'.substr($m, $p);
    }
    $inputs[] = $sign().$m.(mt_rand(0, 2) ? 'e'.mt_rand(-1150, 330) : '');
}
// 15-20 significant digits over the whole exponent range.
for ($i = 0; $i < 4000; $i++) {
    $m = $digits(mt_rand(15, 20));
    $inputs[] = $sign().$m[0].'.'.substr($m, 1).'e'.mt_rand(-330, 310);
}
// Subnormal range and the underflow boundary.
for ($i = 0; $i < 1500; $i++) {
    $m = $digits(mt_rand(1, 25));
    $inputs[] = $m[0].'.'.substr($m, 1).'e'.mt_rand(-326, -305);
}
// Decimal strings of doubles with their last digits perturbed (near halfway points).
for ($i = 0; $i < 3000; $i++) {
    $f = unpack('E', pack('NN', mt_rand(0, 0x7FEFFFFF), (mt_rand(0, 0xFFFF) << 16) | mt_rand(0, 0xFFFF)))[1];
    $s = sprintf('%.16e', $f);
    [$mant, $exp] = explode('e', $s);
    $mant = rtrim($mant, '0');
    $extra = ['5', '49999', '50001', '4999999999', '5000000001', '50', '500000000000000000001'][mt_rand(0, 6)];
    $inputs[] = $mant.$extra.'e'.$exp;
}
// Leading zeros and long zero runs.
for ($i = 0; $i < 500; $i++) {
    $inputs[] = str_repeat('0', mt_rand(1, 60)).'.'.str_repeat('0', mt_rand(0, 340)).$digits(mt_rand(1, 30))
        .(mt_rand(0, 1) ? 'e'.mt_rand(-20, 340) : '');
}

$rows = [];
foreach (array_values(array_unique($inputs)) as $s) {
    $rows[] = [$s, bin2hex(pack('E', (float) $s))];
}
echo "{\"generator\":\"strtod.gen.php\",\"php\":\"", PHP_VERSION, "\",\n\"cases\":[\n";
$n = count($rows);
foreach ($rows as $k => $r) {
    echo json_encode($r, JSON_UNESCAPED_SLASHES), $k + 1 < $n ? ",\n" : "\n";
}
echo "]}\n";

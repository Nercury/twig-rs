<?php
// Test classes of the format/ generators (in addition to ../values.inc.php). tests/format.rs
// rebuilds them as host objects.

declare(strict_types=0);

/** __toString() throws. */
class Thrower
{
    public function __toString(): string
    {
        throw new \Exception('boom');
    }
}

/** jsonSerialize() returns the public property $v. */
class J implements \JsonSerializable
{
    public function __construct(public $v = null)
    {
    }

    public function jsonSerialize(): mixed
    {
        return $this->v;
    }
}

/** jsonSerialize() returns $this: the properties are encoded. */
class JSelf implements \JsonSerializable
{
    public $a = 1;
    protected $b = 2;
    private $c = 3;

    public function jsonSerialize(): mixed
    {
        return $this;
    }
}

/** jsonSerialize() throws. */
class JThrow implements \JsonSerializable
{
    public function jsonSerialize(): mixed
    {
        throw new \Exception('json boom');
    }
}

/** Strings as JSON text when valid UTF-8, else {"hex": ...}. */
function out_enc(string $s)
{
    return preg_match('//u', $s) ? $s : ['hex' => bin2hex($s)];
}

/** A result: the value (via $enc) or {"err": [class, message], "prev": [class, message]?}. */
function out_res($r, callable $enc)
{
    if ($r instanceof \Throwable) {
        $e = ['err' => [get_class($r), out_enc($r->getMessage())]];
        if ($r->getPrevious() !== null) {
            $e['prev'] = [get_class($r->getPrevious()), out_enc($r->getPrevious()->getMessage())];
        }
        return $e;
    }
    return $enc($r);
}

/** Diagnostics captured by common.inc.php as [level, text]. */
function out_diags(array $d): array
{
    return array_map(static fn ($p) => [$p[0], out_enc(hex2bin($p[1]))], $d);
}

/** A row: [inputs..., result] plus the diagnostics when there are any. */
function row(array $inputs, $r, array $d, callable $enc): array
{
    $row = $inputs;
    $row[] = out_res($r, $enc);
    if ($d) {
        $row[] = out_diags($d);
    }
    return $row;
}

function fbits(float $f): string
{
    return bin2hex(pack('E', $f));
}

/** A random finite double from the full bit space. */
function anybits(): float
{
    do {
        $hi = mt_rand(0, 0xFFFFFFFF);
        $lo = mt_rand(0, 0xFFFFFFFF);
        $f = unpack('E', pack('NN', $hi, $lo))[1];
    } while (is_nan($f) || is_infinite($f));
    return $f;
}

/** A random double of a "human" shape: few decimals, ties, integers, log-uniform magnitudes. */
function humanfloat(): float
{
    switch (mt_rand(0, 7)) {
        case 0:
            return mt_rand(-100000, 100000) / 10 ** mt_rand(0, 6);
        case 1:
            return (mt_rand(-20000, 20000) * 2 + 1) / (2 * 10 ** mt_rand(0, 5)); // ties x.5, x.05, ...
        case 2:
            return (float) mt_rand(-PHP_INT_MAX, PHP_INT_MAX);
        case 3:
            return (mt_rand() / mt_getrandmax()) * 10 ** mt_rand(-30, 30);
        case 4:
            return -(mt_rand() / mt_getrandmax()) * 10 ** mt_rand(-320, 308);
        case 5:
            return mt_rand(0, 999999) * 10 ** mt_rand(-25, 25);
        case 6:
            return round(mt_rand() / mt_getrandmax() * 1000, mt_rand(0, 4)) + mt_rand(0, 3) * 0.005;
        default:
            return anybits();
    }
}

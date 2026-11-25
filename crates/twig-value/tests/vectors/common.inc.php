<?php
// Helpers shared by the twig-value vector generators.

declare(strict_types=0);

$GLOBALS['__diags'] = [];
set_error_handler(static function (int $no, string $msg): bool {
    $level = match ($no) {
        E_WARNING => 'W',
        E_NOTICE => 'N',
        E_DEPRECATED => 'D',
        E_USER_DEPRECATED => 'UD',
        default => (string) $no,
    };
    $GLOBALS['__diags'][] = [$level, bin2hex($msg)];
    return true;
});

/** Runs $f, returning [result-or-throwable, diagnostics]. */
function capture(callable $f): array
{
    $GLOBALS['__diags'] = [];
    try {
        $r = $f();
    } catch (\Throwable $e) {
        $r = $e;
    }
    return [$r, $GLOBALS['__diags']];
}

/** Typed JSON form of a scalar/array/object value (doubles as bits, strings as hex). */
function typed($v): array
{
    return match (true) {
        null === $v => ['t' => 'null'],
        is_bool($v) => ['t' => 'bool', 'v' => $v],
        is_int($v) => ['t' => 'int', 'v' => $v],
        is_float($v) => ['t' => 'float', 'bits' => bin2hex(pack('E', $v))],
        is_string($v) => ['t' => 'string', 'hex' => bin2hex($v)],
        is_array($v) => ['t' => 'array', 'n' => count($v)],
        is_object($v) => ['t' => 'object', 'class' => get_class($v)],
    };
}

/** {"ok": typed} or {"err": [class, hex message]}. */
function result($r): array
{
    if ($r instanceof \Throwable) {
        return ['err' => [get_class($r), bin2hex($r->getMessage())]];
    }
    return ['ok' => typed($r)];
}

function emit(string $generator, array $sections): void
{
    echo '{"generator":"', $generator, '","php":"', PHP_VERSION, '"';
    foreach ($sections as $name => $rows) {
        echo ",\n\"", $name, "\":[\n";
        $n = count($rows);
        foreach ($rows as $k => $r) {
            echo json_encode($r, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), $k + 1 < $n ? ",\n" : "\n";
        }
        echo ']';
    }
    echo "}\n";
}

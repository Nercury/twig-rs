<?php
/*
 * Shared helpers of the twig-date vector generators (PHP 8.5.13 with the fakenow.c clock shim, see regen.sh).
 *
 * Strings that are not valid UTF-8 are written as "hex:<bytes>"; exceptions as "Class: message"; diagnostics as
 * "W: …" (E_WARNING), "N: …" (E_NOTICE), "D: …" (E_DEPRECATED).
 */
error_reporting(E_ALL);
ini_set('display_errors', 'stderr');

$GLOBALS['diags'] = [];
set_error_handler(function (int $no, string $msg): bool {
    $level = match ($no) {
        E_WARNING, E_USER_WARNING => 'W',
        E_NOTICE, E_USER_NOTICE => 'N',
        E_DEPRECATED, E_USER_DEPRECATED => 'D',
        default => (string) $no,
    };
    $GLOBALS['diags'][] = $level . ': ' . enc($msg);
    return true;
});

/** Diagnostics reported since the last call. */
function take_diags(): array
{
    $d = $GLOBALS['diags'];
    $GLOBALS['diags'] = [];
    return $d;
}

/** Sets PHP's clock (fakenow.c). */
function now(int $sec, int $usec = 0): void
{
    putenv("FAKE_SEC=$sec");
    putenv("FAKE_USEC=$usec");
}

function enc($s)
{
    if (!is_string($s)) {
        return $s;
    }
    return mb_check_encoding($s, 'UTF-8') ? $s : 'hex:' . bin2hex($s);
}

function exc(Throwable $e): string
{
    return get_class($e) . ': ' . enc($e->getMessage());
}

function pick(array $a)
{
    return $a[mt_rand(0, count($a) - 1)];
}

/** `Y-m-d H:i:s.u T P U e` of a date object (the generators' standard rendering). */
function show(DateTimeInterface $d): string
{
    return $d->format('Y-m-d H:i:s.u T P U e');
}

function emit(string $gen, array $data): void
{
    echo json_encode(
        ['generator' => "tests/vectors/$gen", 'php' => PHP_VERSION, 'timezone_version' => timezone_version_get()] + $data,
        JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR | JSON_PRESERVE_ZERO_FRACTION
    ), "\n";
}

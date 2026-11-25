<?php
// Shared by the string vector generators: a table of named calls and the case encoding.
//
// Every entry of calls() is a closure that calls the builtin directly with a fixed number of arguments, so PHP
// compiles it the way a direct call in Twig's code is compiled (frameless variants where they exist); the
// "_dyn" entries call through call_user_func() to get the regular PHP_FUNCTION. tests/string_*.rs maps the
// same names to the Rust functions.
//
// A case is [name, [args...]]; its row is {"f": name, "a": [enc(arg)...], "r": result, "d": diagnostics} with
// result {"ok": enc(value)} or {"err": [class, hex message]} and diagnostics [[level, hex message]...].

declare(strict_types=0);

require_once __DIR__.'/../common.inc.php';
require_once __DIR__.'/../values.inc.php';

function calls(): array
{
    static $calls = null;
    return $calls ??= [
        'strlen' => static fn ($s) => strlen($s),
        'substr2' => static fn ($s, $o) => substr($s, $o),
        'substr3' => static fn ($s, $o, $l) => substr($s, $o, $l),
        'strpos2' => static fn ($h, $n) => strpos($h, $n),
        'strpos3' => static fn ($h, $n, $o) => strpos($h, $n, $o),
        'strrpos2' => static fn ($h, $n) => strrpos($h, $n),
        'strrpos3' => static fn ($h, $n, $o) => strrpos($h, $n, $o),
        'substr_count2' => static fn ($h, $n) => substr_count($h, $n),
        'substr_count3' => static fn ($h, $n, $o) => substr_count($h, $n, $o),
        'substr_count4' => static fn ($h, $n, $o, $l) => substr_count($h, $n, $o, $l),
        'str_contains' => static fn ($h, $n) => str_contains($h, $n),
        'str_starts_with' => static fn ($h, $n) => str_starts_with($h, $n),
        'str_ends_with' => static fn ($h, $n) => str_ends_with($h, $n),
        'strtolower' => static fn ($s) => strtolower($s),
        'strtoupper' => static fn ($s) => strtoupper($s),
        'ucfirst' => static fn ($s) => ucfirst($s),
        'lcfirst' => static fn ($s) => lcfirst($s),
        'ucwords1' => static fn ($s) => ucwords($s),
        'ucwords2' => static fn ($s, $d) => ucwords($s, $d),
        'strcmp' => static fn ($a, $b) => strcmp($a, $b),
        'strcasecmp' => static fn ($a, $b) => strcasecmp($a, $b),
        'strncasecmp' => static fn ($a, $b, $l) => strncasecmp($a, $b, $l),
        'ord' => static fn ($s) => ord($s),
        'chr' => static fn ($c) => chr($c),
        'str_repeat' => static fn ($s, $n) => str_repeat($s, $n),
        'trim1' => static fn ($s) => trim($s),
        'trim2' => static fn ($s, $c) => trim($s, $c),
        'ltrim1' => static fn ($s) => ltrim($s),
        'ltrim2' => static fn ($s, $c) => ltrim($s, $c),
        'rtrim1' => static fn ($s) => rtrim($s),
        'rtrim2' => static fn ($s, $c) => rtrim($s, $c),
        'explode2' => static fn ($d, $s) => explode($d, $s),
        'explode3' => static fn ($d, $s, $l) => explode($d, $s, $l),
        'implode1' => static fn ($a) => implode($a),
        'implode2' => static fn ($g, $a) => implode($g, $a),
        'implode1_dyn' => static fn ($a) => call_user_func('implode', $a),
        'implode2_dyn' => static fn ($g, $a) => call_user_func('implode', $g, $a),
        'strtr2' => static fn ($s, $f) => strtr($s, $f),
        'strtr3' => static fn ($s, $f, $t) => strtr($s, $f, $t),
        'str_replace' => static fn ($s, $r, $subject) => str_replace($s, $r, $subject),
        'str_replace_count' => static function ($s, $r, $subject) {
            $result = str_replace($s, $r, $subject, $count);
            return [$result, $count];
        },
        'nl2br1' => static fn ($s) => nl2br($s),
        'nl2br2' => static fn ($s, $x) => nl2br($s, $x),
        'strip_tags1' => static fn ($s) => strip_tags($s),
        'strip_tags2' => static fn ($s, $a) => strip_tags($s, $a),
        'htmlspecialchars1' => static fn ($s) => htmlspecialchars($s),
        'htmlspecialchars2' => static fn ($s, $f) => htmlspecialchars($s, $f),
        'htmlspecialchars3' => static fn ($s, $f, $e) => htmlspecialchars($s, $f, $e),
        'htmlspecialchars4' => static fn ($s, $f, $e, $d) => htmlspecialchars($s, $f, $e, $d),
        'rawurlencode' => static fn ($s) => rawurlencode($s),
        'urlencode' => static fn ($s) => urlencode($s),
        'http_build_query1' => static fn ($d) => http_build_query($d),
        'http_build_query2' => static fn ($d, $p) => http_build_query($d, $p),
        'http_build_query3' => static fn ($d, $p, $s) => http_build_query($d, $p, $s),
        'http_build_query4' => static fn ($d, $p, $s, $e) => http_build_query($d, $p, $s, $e),
        'levenshtein2' => static fn ($a, $b) => levenshtein($a, $b),
        'levenshtein5' => static fn ($a, $b, $i, $r, $d) => levenshtein($a, $b, $i, $r, $d),
        'ctype_alnum' => static fn ($v) => ctype_alnum($v),
        'ctype_alpha' => static fn ($v) => ctype_alpha($v),
        'ctype_cntrl' => static fn ($v) => ctype_cntrl($v),
        'ctype_digit' => static fn ($v) => ctype_digit($v),
        'ctype_lower' => static fn ($v) => ctype_lower($v),
        'ctype_graph' => static fn ($v) => ctype_graph($v),
        'ctype_print' => static fn ($v) => ctype_print($v),
        'ctype_punct' => static fn ($v) => ctype_punct($v),
        'ctype_space' => static fn ($v) => ctype_space($v),
        'ctype_upper' => static fn ($v) => ctype_upper($v),
        'ctype_xdigit' => static fn ($v) => ctype_xdigit($v),
        'iconv' => static fn ($f, $t, $s) => iconv($f, $t, $s),
    ];
}

/** {"ok": enc(value)} or {"err": [class, hex message]}. */
function rres($r): array
{
    if ($r instanceof \Throwable) {
        return ['err' => [get_class($r), bin2hex($r->getMessage())]];
    }
    return ['ok' => enc($r)];
}

/** Runs the cases and returns their rows. */
function run_cases(array $cases): array
{
    $calls = calls();
    $rows = [];
    foreach ($cases as [$name, $args]) {
        if (!isset($calls[$name])) {
            throw new \LogicException("unknown call $name");
        }
        $f = $calls[$name];
        [$r, $d] = capture(static fn () => $f(...$args));
        $rows[] = ['f' => $name, 'a' => array_map(static fn ($a) => enc_graph($a), $args), 'r' => rres($r), 'd' => $d];
    }
    return $rows;
}

/** A deterministic pseudo-random string over $alphabet (bytes) of length $min..$max. */
function rand_str(string $alphabet, int $min, int $max): string
{
    $n = mt_rand($min, $max);
    $s = '';
    for ($i = 0; $i < $n; $i++) {
        $s .= $alphabet[mt_rand(0, strlen($alphabet) - 1)];
    }
    return $s;
}

/** A deterministic pseudo-random string made of the given pieces. */
function rand_pieces(array $pieces, int $min, int $max): string
{
    $n = mt_rand($min, $max);
    $s = '';
    for ($i = 0; $i < $n; $i++) {
        $s .= $pieces[mt_rand(0, count($pieces) - 1)];
    }
    return $s;
}

/** Values of every type, for the argument-coercion cases (objects from values.inc.php). */
function odd_values(): array
{
    return [null, true, false, 0, -7, 1.5, -0.0, 1e20, NAN, INF, '', '12', [], [1, 2], new stdClass(), new S('str'),
        Suit::Hearts, U::A];
}

/**
 * enc() for object graphs that may contain cycles: an object already being encoded is written as
 * {"t": "up", "n": k}, the object k levels up the current object path (tests/support/values.rs).
 */
function enc_graph($v, array &$path = []): array
{
    if (is_array($v)) {
        $items = [];
        foreach ($v as $k => $x) {
            $items[] = [is_int($k) ? ['t' => 'int', 'v' => $k] : ['t' => 'string', 'hex' => bin2hex($k)], enc_graph($x, $path)];
        }
        return ['t' => 'array', 'items' => $items];
    }
    if (is_object($v) && !($v instanceof \UnitEnum) && !($v instanceof \Closure)) {
        foreach ($path as $i => $o) {
            if ($o === $v) {
                return ['t' => 'up', 'n' => count($path) - 1 - $i];
            }
        }
        $path[] = $v;
        $props = [];
        foreach ((array) $v as $k => $x) {
            $props[] = [bin2hex((string) $k), enc_graph($x, $path)];
        }
        array_pop($path);
        $o = ['t' => 'object', 'class' => get_class($v), 'props' => $props];
        if ($v instanceof S) {
            $o['str'] = bin2hex((string) $v);
        }
        return $o;
    }
    return enc($v);
}

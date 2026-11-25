<?php

/*
 * Oracle vectors for twig-pcre: preg_match() as PHP 8.5.13 runs it (bundled PCRE2 10.44, JIT on), plus the
 * messages of Twig 3.31.1's `matches` operator (CoreExtension::matches() and MatchesBinary's compile-time
 * check). Output: preg_match.json next to this file. Regenerate from the repository root (Git Bash):
 *
 *   MSYS_NO_PATHCONV=1 docker run --rm -v "$(cygpath -wa crates/twig-pcre/tests/vectors):/v:ro" \
 *       twig-oracle:3.31.1 php /v/preg_match.gen.php > crates/twig-pcre/tests/vectors/preg_match.json
 *
 * PCRE state carries over between calls (compiled-pattern cache, pcre.jit switched off after a JIT allocation
 * failure, ini settings), so every group runs in a fresh PHP process (the script re-invokes itself) and the
 * Rust test replays each group on a fresh twig_pcre::Pcre, step by step, in the same order. Every group is
 * recorded twice: "groups" with PHP's default pcre.jit=1 and "groups_nojit" with pcre.jit=0 from the start
 * (pcre.jit ini steps dropped), which is PCRE2's interpreter path.
 *
 * Step formats in the output (one JSON object per line):
 *   {"ini": [["pcre.backtrack_limit", "100"], ...]}                ini_set() calls, in order
 *   {"p": PATTERN, "s": SUBJECT, "utf8": "auto"|"flagged"|"unflagged", "flags": INT, "offset": INT,
 *    "ops": [{"op": "match", "result": 1|0|false, "warnings": [[ERRNO, TEXT], ...], "error": INT}, ...]}
 * Op kinds: "match" = preg_match($p, $s); "twig_runtime" = CoreExtension::matches($p, $s) -> {"ok": INT} or
 * {"error": RAW MESSAGE}; "twig_compile" = new MatchesBinary(..., new ConstantExpression($p)) -> {"error":
 * null | RAW MESSAGE}; "twig_compile_null" = the same with a null constant instead of $p -> {"error": ...,
 * "last_error": preg_last_error() afterwards}; "captures" = preg_match($p, $s, $m, $flags, $offset) -> result/warnings/error plus
 * "matches" (null = $m untouched, else [[KEY, VALUE], ...]) or "value_error".
 * Byte strings are JSON strings when valid UTF-8, else {"hex": "..."}; long generated strings are
 * {"rep": UNIT, "n": COUNT, "prefix": ..., "suffix": ...}.
 * "utf8" says how the subject string is prepared, which decides PHP's IS_STR_VALID_UTF8 flag: "auto" = empty
 * and one-byte strings are PHP's interned singletons (flagged when ASCII), longer ones fresh (unflagged);
 * "flagged" = a fresh copy flagged by a successful preg_match('//u') first; "unflagged" = a fresh copy.
 */

declare(strict_types=1);

require '/oracle/vendor/autoload.php';

use Twig\Error\RuntimeError;
use Twig\Error\SyntaxError;
use Twig\Extension\CoreExtension;
use Twig\Node\Expression\Binary\MatchesBinary;
use Twig\Node\Expression\ConstantExpression;

const DEFAULT_OPS = ['match', 'twig_runtime', 'twig_compile', 'captures'];

// ---------------------------------------------------------------------------------------------------------
// Byte string helpers

/** A generated string: $prefix . str_repeat($unit, $n) . $suffix, stored compactly in the JSON. */
function rep(string $unit, int $n, string $prefix = '', string $suffix = ''): array
{
    return ['__rep' => [$unit, $n, $prefix, $suffix]];
}

function expand(string|array $s): string
{
    if (is_array($s)) {
        [$unit, $n, $prefix, $suffix] = $s['__rep'];

        return $prefix.str_repeat($unit, $n).$suffix;
    }

    return $s;
}

function enc(string $s): string|array
{
    return mb_check_encoding($s, 'UTF-8') ? $s : ['hex' => bin2hex($s)];
}

function enc_spec(string|array $s): string|array
{
    if (is_array($s)) {
        [$unit, $n, $prefix, $suffix] = $s['__rep'];

        return ['rep' => enc($unit), 'n' => $n, 'prefix' => enc($prefix), 'suffix' => enc($suffix)];
    }

    return enc($s);
}

/** A subject string prepared so that PHP's IS_STR_VALID_UTF8 flag is as described in the header. */
function subject(string $s, string $utf8): string
{
    switch ($utf8) {
        case 'auto':
            if (strlen($s) === 0) {
                return '';
            }
            if (strlen($s) === 1) {
                return $s[0]; // interned one-char string (flagged iff ASCII)
            }

            return hex2bin(bin2hex($s));
        case 'unflagged':
            if (strlen($s) === 0) {
                throw new LogicException('an empty string is always flagged');
            }

            return hex2bin(bin2hex($s));
        case 'flagged':
            if (strlen($s) <= 1) {
                return $s === '' ? '' : $s[0];
            }
            $copy = hex2bin(bin2hex($s));
            preg_match('//u', $copy);

            return $copy;
    }
    throw new LogicException($utf8);
}

// ---------------------------------------------------------------------------------------------------------
// Running steps

function collect_warnings(array &$warnings): void
{
    set_error_handler(static function (int $no, string $str) use (&$warnings): bool {
        $warnings[] = [$no, enc($str)];

        return true;
    });
}

function enc_matches(array $m): array
{
    $out = [];
    foreach ($m as $k => $v) {
        $out[] = [is_int($k) ? $k : enc($k), enc_value($v)];
    }

    return $out;
}

function enc_value(mixed $v): mixed
{
    if (is_string($v)) {
        return enc($v);
    }
    if (is_array($v)) {
        return [is_string($v[0]) ? enc($v[0]) : null, $v[1]];
    }

    return $v; // null
}

function run_case(array $case): array
{
    $p = expand($case['p']);
    $s = expand($case['s']);
    $utf8 = $case['utf8'] ?? 'auto';
    $flags = $case['flags'] ?? 0;
    $offset = $case['offset'] ?? 0;
    $out = ['p' => enc_spec($case['p']), 's' => enc_spec($case['s']), 'utf8' => $utf8, 'flags' => $flags, 'offset' => $offset, 'ops' => []];
    foreach ($case['ops'] ?? DEFAULT_OPS as $op) {
        switch ($op) {
            case 'match':
                $warnings = [];
                $subject = subject($s, $utf8);
                collect_warnings($warnings);
                $r = preg_match($p, $subject);
                restore_error_handler();
                $out['ops'][] = ['op' => 'match', 'result' => $r, 'warnings' => $warnings, 'error' => preg_last_error()];
                break;
            case 'twig_runtime':
                $subject = subject($s, $utf8);
                try {
                    $out['ops'][] = ['op' => 'twig_runtime', 'ok' => CoreExtension::matches($p, $subject)];
                } catch (RuntimeError $e) {
                    $out['ops'][] = ['op' => 'twig_runtime', 'error' => enc($e->getRawMessage())];
                }
                break;
            case 'twig_compile':
                try {
                    new MatchesBinary(new ConstantExpression('', 1), new ConstantExpression($p, 1), 1);
                    $out['ops'][] = ['op' => 'twig_compile', 'error' => null];
                } catch (SyntaxError $e) {
                    $out['ops'][] = ['op' => 'twig_compile', 'error' => enc($e->getRawMessage())];
                }
                break;
            case 'twig_compile_null':
                // A null constant regexp: PHP's null-argument deprecation; preg_last_error() shows that the
                // PCRE state is not touched.
                try {
                    new MatchesBinary(new ConstantExpression('', 1), new ConstantExpression(null, 1), 1);
                    $out['ops'][] = ['op' => 'twig_compile_null', 'error' => null, 'last_error' => preg_last_error()];
                } catch (SyntaxError $e) {
                    $out['ops'][] = ['op' => 'twig_compile_null', 'error' => enc($e->getRawMessage()), 'last_error' => preg_last_error()];
                }
                break;
            case 'captures':
                $warnings = [];
                $subject = subject($s, $utf8);
                $m = 'untouched';
                collect_warnings($warnings);
                try {
                    $r = preg_match($p, $subject, $m, $flags, $offset);
                    restore_error_handler();
                    $out['ops'][] = [
                        'op' => 'captures', 'result' => $r, 'warnings' => $warnings, 'error' => preg_last_error(),
                        'matches' => $m === 'untouched' ? null : enc_matches($m),
                    ];
                } catch (ValueError $e) {
                    restore_error_handler();
                    $out['ops'][] = ['op' => 'captures', 'value_error' => $e->getMessage(), 'warnings' => $warnings];
                }
                break;
            default:
                throw new LogicException($op);
        }
    }

    return $out;
}

/** A step's output, or null when the step is dropped (a pcre.jit-only ini step in the "nojit" run). */
function run_step(array $step, bool $nojit): ?array
{
    if (isset($step['ini'])) {
        if ($nojit) {
            unset($step['ini']['pcre.jit']);
            if ($step['ini'] === []) {
                return null;
            }
        }
        foreach ($step['ini'] as $name => $value) {
            ini_set($name, $value);
        }

        return ['ini' => array_map(null, array_keys($step['ini']), array_values($step['ini']))];
    }

    return run_case($step);
}

// ---------------------------------------------------------------------------------------------------------
// Boundaries (computed once in the parent process, so that the groups start from a fresh PCRE state)

/** Smallest $n in [$lo, $hi] for which preg_match($p, $make($n)) returns false. */
function boundary(string $p, callable $make, int $lo, int $hi): int
{
    if (preg_match($p, $make($hi)) !== false) {
        throw new RuntimeException("no failure for $p up to $hi");
    }
    while ($lo < $hi) {
        $mid = intdiv($lo + $hi, 2);
        if (preg_match($p, $make($mid)) === false) {
            $hi = $mid;
        } else {
            $lo = $mid + 1;
        }
    }

    return $lo;
}

/** Smallest $n in [$lo, $hi] for which compiling $make($n) emits a warning. */
function warning_boundary(callable $make, int $lo, int $hi): int
{
    $warns = static function (string $p): bool {
        $w = false;
        set_error_handler(static function () use (&$w): bool {
            $w = true;

            return true;
        });
        preg_match($p, '');
        restore_error_handler();
        ini_set('pcre.jit', '1'); // a JIT allocation failure switches the JIT off

        return $w;
    };
    if (!$warns($make($hi))) {
        throw new RuntimeException("no warning up to $hi");
    }
    while ($lo < $hi) {
        $mid = intdiv($lo + $hi, 2);
        if ($warns($make($mid))) {
            $hi = $mid;
        } else {
            $lo = $mid + 1;
        }
    }

    return $lo;
}

function bounds(): array
{
    $a = static fn (int $n): string => str_repeat('a', $n);
    $ab = static fn (int $n): string => str_repeat('a', $n).'b';
    $ac = static fn (int $n): string => str_repeat('a', $n).'c';
    $b = [];
    $b['jit_stack'] = boundary('/^(a|b)*$/', $a, 1, 1 << 21);
    $b['jit_stack_nc'] = boundary('/^(?:a|b)*$/', $a, 1, 1 << 21);
    $b['jit_stack_lazy'] = boundary('/^(a|b)*?$/', $a, 1, 1 << 21);
    $b['depth'] = boundary('/(*NO_JIT)^(a|b)*$/', $a, 1, 1 << 21);
    $b['depth_1000'] = boundary('/(*LIMIT_DEPTH=1000)(*NO_JIT)^(a|b)*$/', $a, 1, 1 << 21);
    $b['match_1000'] = boundary('/(*LIMIT_MATCH=1000)(*NO_JIT)^(a|b)*$/', $a, 1, 1 << 21);
    $b['retry_depth'] = boundary('/x*|^(a|b)*$/', $a, 1, 1 << 21);
    $b['retry_backtrack'] = boundary('/x*|(?:a+)+b/', $ac, 1, 64);
    $b['backtrack'] = boundary('/(a+)+$/', $ab, 1, 64);
    $b['backtrack_nojit'] = boundary('/(*NO_JIT)(a+)+$/', $ab, 1, 64);
    $b['backtrack_alt'] = boundary('/(?:a|a)+$/', $ab, 1, 64);
    $b['backtrack_alt_nojit'] = boundary('/(*NO_JIT)(?:a|a)+$/', $ab, 1, 64);
    // A JIT pattern compiled while pcre.jit=1 keeps its JIT code; with pcre.jit=0 it runs without the JIT
    // stack (32 KiB machine-stack area).
    preg_match('/^(a|b)*$/S', 'a');
    ini_set('pcre.jit', '0');
    $b['jit_machine_stack'] = boundary('/^(a|b)*$/S', $a, 1, 1 << 21);
    ini_set('pcre.jit', '1');
    $b['too_large'] = warning_boundary(static fn (int $n): string => '/'.str_repeat('a', $n).'/', 1, 1 << 17);
    $b['jit_nomem'] = warning_boundary(static fn (int $n): string => '/'.str_repeat('(a)', $n).'/', 1, 1 << 14);

    return $b;
}

// ---------------------------------------------------------------------------------------------------------
// Cases

function c(string|array $p, string|array $s, array $extra = []): array
{
    return ['p' => $p, 's' => $s] + $extra;
}

function groups(array $b): array
{
    $g = [];

    // Delimiters, modifiers and every warning of pcre_get_compiled_regex_cache_ex().
    $d = [
        c('', 'a'), c(' ', 'a'), c("\t\n\r\x0B\x0C ", 'a'), c(' /a/', 'a'), c("\x0B/a/", 'a'), c("\x0C/a/", 'a'),
        c("\n/a/\n", 'a'), c("\r\n\t/a/", 'a'), c("\xA0/a/", 'a'), c("\x85a\x85", 'a'), c("\xFFa\xFF", 'a'),
        c("\xA0a\xA0", "x\xA0a"), c("\x00/a/", 'a'), c("\x00", ''), c("\x00\x00", ''),
        c('a/b/', 'b'), c('Z/b/', 'b'), c('0/b/', 'b'), c('9a9', 'a'), c('\\a\\', 'a'), c('\\/', ''),
        c('/', ''), c('//', ''), c('//', 'abc'), c('/a', 'a'), c('/a/', 'a'), c('/a/', 'b'),
        c('/a\\/b/', 'a/b'), c('/a\\/', 'a'), c('/a\\\\/', 'a\\'), c('/\\', ''), c('/a\\', 'a'), c('/\\\\/', '\\'),
        c('(a)', 'a'), c('[a]', 'a'), c('{a}', 'a'), c('<a>', 'a'), c('(a(b)c)', 'abc'), c('(a(b)c', 'abc'),
        c('(a\\)b)', 'a)b'), c('(a\\(b)', 'a(b'), c('(a(b\\)c)', 'abc'), c('(', ''), c('()', ''), c('(()', ''),
        c('((a))', 'a'), c(')a)', 'a'), c(']a]', 'a'), c('}a}', 'a'), c('>a>', 'a'), c(')a(', 'a'),
        c('[a-z]', 'a-z'), c('[a-z]', 'q'), c('[a-z]+', 'q'), c('[[a]]', 'a'), c('{a{2}}', 'aa'), c('<a<b>>', 'ab'),
        c('{a}i', 'A'), c('(a)(b)', 'ab'), c('<a>u', 'a'), c('[a]]', 'a'), c('{a}}', 'a'), c('(a))', 'a'),
        c('/a/i ', 'A'), c("/a/\ni", 'A'), c("/a/\r\n", 'a'), c("/a/ \n \r", 'a'), c("/a/\t", 'a'), c('/a/e', 'a'),
        c('/a/E', 'a'), c('/a/g', 'a'), c("/a/\x00", 'a'), c("/a/i\x00", 'a'), c("/a/\x00i", 'a'), c("/a/\xFF", 'a'),
        c("/a/\xC3\xA9", 'a'), c('/a/iu', 'A'), c('/a/imsxADSUXJnru', 'a'), c('/a/uuuu', 'a'), c('/a/k', 'a'),
        c('/a/I', 'a'), c('/a/L', 'a'), c('/a/0', 'a'), c('/a/\\', 'a'), c('/a/ /', 'a'), c('/a//', 'a'),
        c('/a/i/', 'a'), c('/a/ie', 'a'), c('/a/ei', 'a'), c("/a/\x0B", 'a'), c("/a/\x0C", 'a'),
        c("/a\x00b/", "a\x00b"), c('/a/', "b\x00a"), c("/\x00/", "\x00"), c("/[\x00-\x02]/", "\x01"), c('#a#', 'A'),
    ];
    foreach (str_split('#~!@%+|\'"`,;:=-*?^$.&_') as $delim) {
        $d[] = c($delim.'a'.$delim, 'a');
    }
    $g['delimiters'] = $d;

    // Every byte as the first character, as the delimiter and as a modifier: isspace() and isalnum() of the
    // oracle's LC_CTYPE (C.UTF-8), the bracket pairs and the whole modifier switch.
    $sw = [];
    for ($i = 0; $i < 256; $i++) {
        $ch = chr($i);
        $sw[] = c($ch.'/a/', 'a', ['ops' => ['match']]);
        $sw[] = c($ch.'a'.$ch, 'a', ['ops' => ['match']]);
        $sw[] = c('/a/'.$ch, 'A', ['ops' => ['match']]);
    }
    $g['byte_sweeps'] = $sw;

    // Modifiers and their PCRE2 options.
    $g['modifiers'] = [
        c('/abc/i', 'ABC'), c('/abc/', 'ABC'), c('/^b/m', "a\nb"), c('/^b/', "a\nb"), c('/a$/m', "a\nb"),
        c('/a.b/s', "a\nb"), c('/a.b/', "a\nb"), c('/a b c/x', 'abc'), c("/a#comment\nb/x", 'ab'), c('/a b/', 'ab'),
        c('/a\\ b/x', 'a b'), c('/[a b]/x', ' '), c('/b/A', 'ab'), c('/a/A', 'ab'), c('/a$/D', "a\n"), c('/a$/', "a\n"),
        c('/a$/mD', "a\nb"), c('/a+/U', 'aaa'), c('/a+?/U', 'aaa'), c('/(a+)(a*)/U', 'aaa'),
        c('/(?<n>a)|(?<n>b)/J', 'b'), c('/(?<n>a)|(?<n>b)/', 'b'), c('/(?<n>a)|(?<n>b)\\k<n>/J', 'bb'),
        c('/(a)\\1/n', 'aa'), c('/(a)(b)/n', 'ab'), c('/(?<x>a)\\k<x>/n', 'aa'), c('/(?<x>a)(b)/n', 'ab'),
        c('/\\x{212A}/iu', 'k'), c('/\\x{212A}/iur', 'k'), c('/k/iu', "\u{212A}"), c('/k/iur', "\u{212A}"),
        c('/s/iu', 'ſ'), c('/s/iur', 'ſ'), c('/k/ir', 'K'), c('/k/r', 'K'), c('/ſ/iu', 'S'), c('/ſ/iur', 'S'),
        c('/\\w/u', 'é'), c('/\\w/', 'é'), c('/^\\w$/u', 'é'), c('/^\\W+$/', 'é'), c('/\\d/u', '٣'), c('/\\d/', '٣'),
        c('/[[:alpha:]]/u', 'é'), c('/[[:alpha:]]/', 'é'), c('/\\s/u', "\u{00A0}"), c('/\\s/', "\xC2\xA0"),
        c('/\\s/', "\xA0"), c('/\\s/', "\x0B"), c('/\\s/', "\x85"), c('/\\s/u', "\u{0085}"), c('/\\bé/u', ' é'),
        c('/\\bé/', ' é'), c('/./', 'é'), c('/^.$/', 'é'), c('/^.$/u', 'é'), c('/^..$/', 'é'), c('/\\C/', 'é'),
        c('/^\\C\\C$/', 'é'), c('/É/i', 'é'), c('/É/iu', 'é'), c('/a/S', 'a'), c('/a/X', 'a'), c('/\\y/', 'y'),
        c('/\\y/X', 'y'), c('/[[:upper:]]/i', 'a'), c('/[[:upper:]]/iu', 'ä'), c('/\\p{Lu}/i', 'a'),
        c('/ǅ/iu', 'ǆ'), c('/ß/iu', 'SS'), c('/ß/iu', 'ẞ'), c('/i/iu', 'İ'), c('/ı/iu', 'I'), c('/σ/iu', 'ς'),
        c('/(?i)a(?-i)b/', 'AB'), c('/(?i)a(?-i)b/', 'Ab'), c('/(?x) a b /', 'ab'), c('/(?xx)[a b]/', ' '),
        c('/(?s).(?-s)./', "\n\n"), c('/(?m)^b/', "a\nb"), c('/(?U)a+/', 'aa'), c('/(?J)(?<a>x)|(?<a>y)/', 'y'),
        c('/(?n)(a)\\1/', 'aa'), c('/(?^i)a/', 'A'), c('/(?i)(?^)a/', 'A'), c('/(?aD)\\d/u', '٣'),
        c('/(?aT)\\d/u', '٣'), c('/(?aS)\\s/u', "\u{00A0}"), c('/(?aW)\\w/u', 'é'), c('/(?aP)[[:alpha:]]/u', 'é'),
        c('/(?r)k/iu', "\u{212A}"), c('/(*UCP)\\w/', 'é'), c('/(*UTF)(*UCP)\\w/', 'é'), c('/(*UTF)^.$/', 'é'),
        c('/(*UTF)\\C/', 'é'),
    ];

    // Compilation errors: PCRE2 messages and offsets.
    $ce = [
        c('/(/', 'a'), c('/)/', 'a'), c('/[a-/', 'a'), c('/a{2,1}/', 'a'), c('/x**/', 'a'), c('/?/', 'a'),
        c('/*a/', 'a'), c('/+/', 'a'), c('/a{99999}/', 'a'), c('/a{65536}/', 'a'), c('/a{65535}/', 'a'),
        c('/(?<n>a)(?<n>b)/', 'a'), c('/(?P<1a>x)/', 'x'), c('/\\p{Foo}/u', 'a'), c('/\\p{Foo}/', 'a'),
        c('/\\p{L/', 'a'), c('/(?<=a+)b/', 'ab'), c('/(?<=a{1,256})b/', 'ab'), c('/(?<=a{1,255})b/', 'ab'),
        c('/(?<=a*)b/', 'ab'), c('/(?<=\\d+)b/', '1b'), c('/[[:foo:]]/', 'a'), c('/[z-a]/', 'a'), c('/(?z)/', 'a'),
        c('/\\k<missing>/', 'a'), c('/(?(1)a|b|c)/', 'a'), c('/\\g/', 'a'), c('/(*FOO)/', 'a'), c('/\\N{U+41}/', 'A'),
        c('/\\N{U+41}/u', 'A'), c('/\\o{}/', 'a'), c('/\\x{110000}/u', 'a'), c('/\\x{100}/', 'a'), c('/\\x{FF}/', "\xFF"),
        c('/(?=a\\K)/', 'a'), c('/(?<=\\Ka)b/', 'ab'), c('/a\\Kb/', 'ab'), c('/\\c/', 'a'), c('/\\cé/', 'a'),
        c('/(?#unterminated/', 'a'), c('/(?C256)/', 'a'), c("/(?C'abc)/", 'a'), c('/(*MARK)/', 'a'), c('/\\u0041/', 'A'),
        c('/\\L/', 'a'), c('/\\U/', 'a'), c('/\\l/', 'a'), c('/[\\d-z]/', 'a'), c('/(?|(a)|(b))\\2/', 'a'),
        c('/(?P=x)/', 'a'), c('/(?P>x)/', 'a'), c('/(?&x)/', 'a'), c('/(?R/', 'a'), c('/(?1/', 'a'), c('/(?1)/', 'a'),
        c('/\\8/', 'a'), c('/(a)\\2/', 'a'), c('/a{1,65536}/', 'a'), c('/(?<=\\X)a/', 'a'), c('/(?<=a|bc)d/', 'bcd'),
        c('/(?<=a(b|cd))e/', 'acde'), c('/\\Q/', ''), c('/[a/', 'a'), c('/[]/', 'a'), c('/[]a]/', ']'), c('/(?i/', 'a'),
        c('/(?-/', 'a'), c('/{/', '{'), c('/x{2/', 'x{2'), c("/(?'abc'x)/", 'x'), c('/(?<abc/', 'a'),
        c('/(?<123>x)/', 'x'), c('/(?<a-b>x)/', 'x'), c('/(?<é>x)/u', 'x'), c('/(?<é>x)/', 'x'), c('/\\/', 'a'),
        c('/[\\C]/u', 'C'), c('/\\C/u', 'a'), c('/a\\C/u', 'a'), c('/(?<=\\C)a/', 'a'), c('/\\R{2}/', "\n\n"),
        c('/[\\R]/', 'R'), c('/[\\X]/', 'X'), c('/[\\B]/', 'B'), c('/\\9/', 'a'), c('/(?(?=a)b|c|d)/', 'a'),
        c('/(?(DEFINE)a|b)/', 'a'), c('/(?(VERSION>=10.0)a|b)/', 'a'), c('/(?(VERSION>=x)a|b)/', 'a'),
        c('/(*LIMIT_MATCH=x)a/', 'a'), c('/(*LIMIT_MATCH=99999999999)a/', 'a'), c('/(*NOTEMPTY/', 'a'),
        c('/(*:)/', 'a'), c('/(*MARK:'.str_repeat('m', 256).')/', 'a'), c('/(*MARK:'.str_repeat('m', 255).')/', 'a'),
        c('/(?<'.str_repeat('n', 128).'>x)/', 'x'), c('/(?<'.str_repeat('n', 129).'>x)/', 'x'),
        c(rep('(', 249, '/', str_repeat(')', 249).'/'), ''), c(rep('(', 250, '/', str_repeat(')', 250).'/'), ''),
        c(rep('(', 251, '/', str_repeat(')', 251).'/'), ''), c(rep('(?:', 251, '/', str_repeat(')', 251).'/'), ''),
        c(rep('a', $b['too_large'] - 1, '/', '/'), 'a', ['ops' => ['match', 'twig_compile']]),
        c(rep('a', $b['too_large'], '/', '/'), 'a', ['ops' => ['match', 'twig_compile']]),
        c(rep('()', 65535, '/', '/'), '', ['ops' => ['match']]), c(rep('()', 65536, '/', '/'), '', ['ops' => ['match']]),
        c("/\xFF/u", 'a'), c("/\xC3/u", 'a'), c("/a\xC3(/u", 'a'), c("/\xC0\x80/u", 'a'), c("/\xED\xA0\x80/u", 'a'),
        c("/\xF4\x90\x80\x80/u", 'a'), c("/\xE2\x82/u", 'a'), c("/abc\x80/u", 'a'), c("/\xF8\x88\x80\x80\x80/u", 'a'),
        c("/\xFC\x84\x80\x80\x80\x80/u", 'a'), c("/\xE0\x80\x80/u", 'a'), c("/\xF0\x80\x80\x80/u", 'a'),
        c("/a\xE2\x82\xACb\xFF/u", 'a'), c('/é(/u', 'é'), c('/é(/', 'é'), c("/\xFF(/", 'a'),
    ];
    $g['compile_errors'] = $ce;

    // UTF-8 subjects and Unicode semantics (PCRE2 10.44 = Unicode 15.0.0).
    $u = [];
    foreach (["\xC0\x80", "\xF0\x8F\x80\x80", "\xED\xA0\x80", "\xED\xBF\xBF", "\xF4\x90\x80\x80", "\xF5\x80\x80\x80",
        "\xF8\x88\x80\x80", "\xE2\x82", "\x80", "\xBF", "a\xFF", "\xFE", "\xC2", "a\xC3\x28", "\xE2\x28\xA1",
        "\xF0\x28\x8C\xBC", "\xF4\x8F\xBF\xBF", "\xEF\xBF\xBF", "\xEE\xBF\xBF", "\xC2\xA0", "\x00", "\x7F",
        "\xE0\xA0\x80", "\xF0\x90\x80\x80", "\xED\x9F\xBF", "\xEE\x80\x80", 'aé€😀'] as $subj) {
        $u[] = c('//u', $subj);
        $u[] = c('/x/u', $subj);
        $u[] = c('/./', $subj, ['ops' => ['match']]);
    }
    array_push($u,
        c('/(*UTF)é/', 'é'), c('/(*UTF)./', 'aé'), c('/^\\X$/u', "e\u{301}"), c('/^\\X$/u', '👍🏽'),
        c('/^\\X$/u', '🇱🇹'), c('/^\\X$/u', '🇱🇹🇺'), c('/^\\X$/u', "\r\n"), c('/^\\X$/u', '👨‍👩‍👧'),
        c('/^\\X\\X$/', 'é'), c('/\\p{Greek}/u', 'α'), c('/\\p{Lu}/u', 'É'), c('/\\p{Han}/u', '漢'),
        c('/\\p{Cyrillic}/u', "\u{1E030}"), c('/\\p{So}/u', "\u{1F6DC}"), c('/\\p{Lu}/u', "\u{1C89}"),
        c('/\\p{Cn}/u', "\u{1C89}"), c('/\\p{Cn}/u', "\u{1F6DC}"), c('/\\x{1C89}/iu', "\u{1C8A}"),
        c('/\\p{Emoji}/u', '😀'), c('/\\p{Bidi_Class=AL}/u', 'ب'), c('/\\p{sc:Latin}/u', 'a'),
        c('/\\p{Script_Extensions=Greek}/u', "\u{0342}"), c('/\\p{Any}/u', "\u{10FFFF}"), c('/\\x{10FFFF}/u', "\u{10FFFF}"),
        c('/é/', 'é'), c('/[é]/', 'é'), c('/^[é]$/', 'é'), c('/^[é]$/u', 'é'), c('/[^é]/u', 'é'), c('/[^é]/', 'é'),
        c('/\\x{e9}/u', 'é'), c('/\\xe9/', 'é'), c('/\\xe9/', "\xE9"), c('/\\xe9/u', "\xE9"), c('/[\\x{80}-\\x{10FFFF}]/u', 'é'),
        c('/\\N{U+1F600}/u', '😀'), c('/^.{3}$/u', 'aé€'), c('/^.{6}$/', 'aé€'), c('/\\h/u', "\u{2003}"),
        c('/\\v/u', "\u{2028}"), c('/\\R/u', "\u{2029}"), c('/\\R/', "\xE2\x80\xA9"), c('/^\\R$/', "\x85"),
        c('/^\\R$/u', "\u{0085}"), c('/a/u', "\xFF", ['utf8' => 'unflagged']), c('/(*NOTEMPTY)/u', 'é'),
    );
    $g['utf8'] = $u;

    // General PCRE features.
    $g['matching'] = [
        c('/(a)\\1/', 'aa'), c('/(a)\\1/', 'ab'), c('/(?<x>a)\\k{x}/', 'aa'), c('/(?<x>a)\\g{x}/', 'aa'),
        c('/(a)\\g{-1}/', 'aa'), c('/(?i)(a)\\1/', 'aA'), c('/a(?=b)/', 'ab'), c('/a(?!b)/', 'ab'), c('/(?<=a)b/', 'ab'),
        c('/(?<!a)b/', 'ab'), c('/a++a/', 'aaa'), c('/(?>a+)a/', 'aaa'), c('/a+a/', 'aaa'), c('/a*+b/', 'aab'),
        c('/a\\Kb/', 'ab'), c('/^(\\((?1)*\\))$/', '(()())'), c('/^(\\((?1)*\\))$/', '(()'), c('/^(a|b(?1))$/', 'bba'),
        c('/^(?R)?a/', 'aa'), c('/^(a)?(?(1)b|c)$/', 'ab'), c('/^(a)?(?(1)b|c)$/', 'c'), c('/^(a)?(?(1)b|c)$/', 'ac'),
        c('/(?(?=a)ab|cd)/', 'cd'), c('/a\\Rb/', "a\r\nb"), c('/\\h/', "\t"), c('/\\v/', "\x0B"), c('/a\\Nb/', "a\nb"),
        c('/^b$/m', "a\r\nb\r\n"), c('/(*CRLF)^b$/m', "a\r\nb\r\n"), c('/(*ANYCRLF)^b$/m', "a\rb\r"), c('/(*CR)a$/', "a\r"),
        c('/(*ANY)a$/', "a\u{2028}"), c('/(*ANY)a$/u', "a\u{2028}"), c('/(*NUL)a$/', "a\x00"), c('/(*LF)a$/', "a\r"),
        c('/(*BSR_ANYCRLF)\\R/', "\x0B"), c('/(*BSR_UNICODE)\\R/', "\x0B"), c('/a\\z/', "a\n"), c('/a\\Z/', "a\n"),
        c('/\\Aa/', 'ba'), c('/\\Ga/', 'ba'), c('/\\Gb/', 'b'), c('/aaa(*SKIP)(*FAIL)|a/', 'aaaa'), c('/a(*ACCEPT)b/', 'ac'),
        c('/a(*COMMIT)b|ac/', 'ac'), c('/a(*PRUNE)b|ac/', 'ac'), c('/a(*THEN)b|ac/', 'ac'), c('/(*MARK:x)a/', 'a'),
        c('/a{,3}/', 'aaa'), c('/^a{,3}$/', 'aaaa'), c('/^a{ 1 , 2 }$/', 'aa'), c('/\\x41/', 'A'), c('/\\101/', 'A'),
        c('/\\o{101}/', 'A'), c('/\\cA/', "\x01"), c('/\\e/', "\x1B"), c('/\\a/', "\x07"), c('/\\Qa.b\\E/', 'a.b'),
        c('/\\Qa.b\\E/', 'axb'), c('/[[:^alpha:]]/', 'a1'), c('/[\\w-]/', '-'), c('/[a-\\d]/', '-'), c('/[%--]/', ','),
        c('/^(*sr:\\w+)$/u', 'abc'), c('/^(*sr:\\w+)$/u', 'aа'), c('/^(*script_run:\\d+)$/u', '1٢'), c('/(*pla:a)a/', 'a'),
        c('/(*atomic:a+)a/', 'aa'), c('/(*nla:a)b/', 'b'), c('/(*plb:a)b/', 'ab'), c('/(*nlb:a)b/', 'ab'),
        c('/(*napla:a)a/', 'a'), c('/a(?C1)b/', 'ab'), c("/a(?C'x')b/", 'ab'), c('/a(?C)b/', 'ab'), c('/(?C1)/', ''),
        c('/(*LIMIT_MATCH=10)(?:a+)+$/', 'aaaaaaaaaaaab'), c('/(*LIMIT_MATCH=1)a/', 'a'), c('/(*LIMIT_MATCH=1)a/', 'ba'),
        c('/(*LIMIT_DEPTH=10)(a)*$/', str_repeat('a', 40)), c('/(*LIMIT_DEPTH=10)(*NO_JIT)(a)*$/', str_repeat('a', 40)),
        c('/(*LIMIT_HEAP=1)(*NO_JIT)(a)*$/', rep('a', 4000)), c('/(*LIMIT_HEAP=0)(*NO_JIT)a/', 'a'),
        c('/(*NO_JIT)(?:\\D+|<\\d+>)*[!?]/', 'foobar foobar foobar'), c('/(?:\\D+|<\\d+>)*[!?]/', 'foobar foobar foobar'),
        c('/(*NO_START_OPT)(*NO_JIT)(a+)+$/', str_repeat('a', 16).'b'), c('/(*NO_AUTO_POSSESS)a+b/', 'aab'),
        c('/(*NO_DOTSTAR_ANCHOR).*a/', "b\na"), c('/(*NO_JIT)(*NO_START_OPT)x/', 'abc'), c('/a|b|c|d|e/', 'e'),
        c('/(?|(a)|(b))/', 'b'), c('/(?|(?<n>a)|(?<n>b))/', 'b'), c('/(a)|(b)|(c)/', 'c'), c('/(a)?b/', 'b'),
        c('/(?<year>\\d{4})-(?<month>\\d{2})/', '2024-06'), c('/(a)(?:b)(c)/', 'abc'), c('/(*MARK:A)a|(*MARK:B)b/', 'b'),
        c('/(*MARK:A)(*MARK:B)c/', 'c'), c('/a(*MARK:m)(*FAIL)|b/', 'ab'), c('/(*:mark)a/', 'a'),
        c('/(?:x|(*MARK:A)a)/', 'a'), c(rep('(a)', 40, '/', '/'), str_repeat('a', 40)),
        c(rep('(a)?', 40, '/', 'b/'), 'b'), c('/\\d+(?:\\.\\d+)?/', 'v1.25'), c('/^[\\w.+-]+@\\w+\\.\\w+$/', 'a.b+c@d.ef'),
        c('/colou?r/', 'The color'), c('/\\bcat\\b/', 'concatenate'), c('/\\Bcat\\B/', 'concatenate'),
        c('/^\\s*$/', " \t\r\n\x0B\x0C"), c('/[[:punct:]]/', '~'), c('/[[:cntrl:]]/', "\x7F"), c('/[[:graph:]]/', ' '),
        c('/[[:print:]]/', ' '), c('/[[:word:]]/', '_'), c('/[[:xdigit:]]+$/', 'deadBEEF'), c('/[[:blank:]]/', "\t"),
    ];

    // Empty matches: preg_match() without $matches retries PCRE2_NOTEMPTY_ATSTART | PCRE2_ANCHORED.
    $g['empty_matches'] = [
        c('/a*/', 'b'), c('//', ''), c('//', 'abc'), c('/$/', 'abc'), c('/^/', ''), c('/\\b/', 'a'), c('/(?=a)/', 'a'),
        c('/(*NOTEMPTY)a*/', 'b'), c('/(*NOTEMPTY)a*/u', 'b'), c('/(*NOTEMPTY)a*/u', 'b', ['utf8' => 'unflagged']),
        c('/(*NOTEMPTY)a*/u', 'bb'), c('/(*NOTEMPTY)a*/u', 'bb', ['utf8' => 'flagged']), c('/(*NOTEMPTY)a*/', 'ba'),
        c('/(*NOTEMPTY_ATSTART)a*/', 'b'), c('/(*NOTEMPTY_ATSTART)a*/u', 'bc'), c('/(*NOTEMPTY_ATSTART)a*/u', 'b'),
        c('/(*NOTEMPTY_ATSTART)^a*/u', 'bc'), c('/(*NOTEMPTY_ATSTART)^a*/u', 'bc', ['utf8' => 'flagged']),
        c('/(*NOTEMPTY)/', ''), c('/(*NOTEMPTY)/u', ''), c('/(*NOTEMPTY)/u', 'xy'), c('/(*NOTEMPTY)/u', 'xy', ['utf8' => 'flagged']),
        c('/(*NO_JIT)(*NOTEMPTY)a*/u', 'b'), c('/(*NO_JIT)(*NOTEMPTY)a*/', 'b'), c('/(*NOTEMPTY)a*/u', 'ab', ['utf8' => 'flagged']),
        c('/x*|^(a|b)*$/', rep('a', $b['retry_depth'] - 1)), c('/x*|^(a|b)*$/', rep('a', $b['retry_depth'])),
        c('/x*|(?:a+)+b/', rep('a', $b['retry_backtrack'] - 1, '', 'c')), c('/x*|(?:a+)+b/', rep('a', $b['retry_backtrack'], '', 'c')),
        c('/x*|(?:a+)+b/u', rep('a', $b['retry_backtrack'], '', 'c')), c('/(*LIMIT_MATCH=3)a*|b+c/', 'bbbbbbbbb'),
        c('/(*LIMIT_DEPTH=2)x*|(a)(b)(c)d/', 'abce'),
    ];

    // Backtrack, recursion (depth) and JIT stack limits at PHP's settings, at the exact boundaries.
    $l = [
        c('/(?:\\D+|<\\d+>)*[!?]/', 'foobar foobar foobar'), c('/(?:\\D+|<\\d+>)*[!?]/', 'foobar foobar'),
        c('/(*NO_JIT)(?:\\D+|<\\d+>)*[!?]/', 'foobar foobar'),
    ];
    foreach ([
        ['/^(a|b)*$/', 'jit_stack', '', ''], ['/^(?:a|b)*$/', 'jit_stack_nc', '', ''], ['/^(a|b)*?$/', 'jit_stack_lazy', '', ''],
        ['/(*NO_JIT)^(a|b)*$/', 'depth', '', ''], ['/(*LIMIT_DEPTH=1000)(*NO_JIT)^(a|b)*$/', 'depth_1000', '', ''],
        ['/(*LIMIT_MATCH=1000)(*NO_JIT)^(a|b)*$/', 'match_1000', '', ''], ['/(a+)+$/', 'backtrack', '', 'b'],
        ['/(*NO_JIT)(a+)+$/', 'backtrack_nojit', '', 'b'], ['/(?:a|a)+$/', 'backtrack_alt', '', 'b'],
        ['/(*NO_JIT)(?:a|a)+$/', 'backtrack_alt_nojit', '', 'b'],
    ] as [$p, $key, $prefix, $suffix]) {
        $l[] = c($p, rep('a', $b[$key] - 1, $prefix, $suffix), ['ops' => ['match', 'twig_runtime']]);
        $l[] = c($p, rep('a', $b[$key], $prefix, $suffix), ['ops' => ['match', 'twig_runtime']]);
    }
    array_push($l,
        c('/^(a|b)*$/u', rep('a', $b['jit_stack'] - 1), ['ops' => ['match']]), c('/^(a|b)*$/u', rep('a', $b['jit_stack']), ['ops' => ['match']]),
        c('/^(a|b)*$/', rep('a', $b['jit_stack']), ['ops' => ['captures']]), c('/(*LIMIT_MATCH=1000)^(a|b)*$/', rep('a', $b['jit_stack'] - 1), ['ops' => ['match']]),
        c('/^(a|b)*$/', rep('ab', $b['jit_stack'])), c('/(?R)/', ''), c('/(?R)/', 'abc'), c('/(*NO_JIT)(?R)/', ''),
        c('/(a|(?R))/', 'b'), c('/(*NO_JIT)(a|(?R))/', 'b'), c('/^(?:(a)|b)*$/', rep('ab', 3000)),
    );
    $g['limits'] = $l;

    // preg_match() with $matches, flags and offsets.
    $cap = static fn (string|array $p, string|array $s, int $flags = 0, int $offset = 0, array $extra = []): array => c($p, $s, ['flags' => $flags, 'offset' => $offset, 'ops' => ['captures']] + $extra);
    $both = PREG_OFFSET_CAPTURE | PREG_UNMATCHED_AS_NULL;
    $captures = [];
    foreach ([
        ['/(a)(b)?(c)?/', 'a'], ['/(a)?(b)(c)?/', 'b'], ['/(?<x>a)|(?<y>b)/', 'b'], ['/(?<x>a)|(?<x>b)/J', 'b'],
        ['/(?<x>a)|(?<x>b)/J', 'a'], ['/(?J)(?<x>a)?(?<x>b)/', 'b'], ['/(?<n>a)(?<m>b)?/', 'a'], ['/x/', 'abc'],
        ['/(*MARK:A)a|(*MARK:B)b/', 'b'], ['/(?<MARK>a)(*MARK:x)/', 'a'], ['/(?|(?<a>x)|(?<a>y))/', 'y'], ['//', ''],
        ["/(*MARK:a\0b)x/", 'x'], ["/(*MARK:\0b)x/", 'x'], ['/(*MARK:a\\x00b)x/', 'x'], ['/(*MARK:a b)x/x', 'x'],
        ['/(b)?$/', 'a'], ['/(\\d+)-(\\d+)/', 'tel 12-345'], ['/(?<w>\\w+) (?<d>\\d+)?/', 'abc '], ['/é(.)/u', 'aéb'],
        ['/x*|(?:a+)+b/', rep('a', $b['retry_backtrack'], '', 'c')], ['/x*|^(a|b)*$/', rep('a', $b['retry_depth'])],
    ] as [$p, $s]) {
        foreach ([0, PREG_OFFSET_CAPTURE, PREG_UNMATCHED_AS_NULL, $both] as $flags) {
            $captures[] = $cap($p, $s, $flags);
        }
    }
    array_push($captures,
        $cap('/a/', 'aba', 0, 1), $cap('/a/', 'aba', 0, 2), $cap('/a/', 'aba', 0, 3), $cap('/a/', 'aba', 0, 4),
        $cap('/a/', 'aba', 0, -1), $cap('/a/', 'aba', 0, -2), $cap('/a/', 'aba', 0, -3), $cap('/a/', 'aba', 0, -4),
        $cap('/a/', 'aba', 0, PHP_INT_MAX), $cap('/a/', 'aba', 0, PHP_INT_MIN), $cap('/a/', 'aba', 0, PHP_INT_MIN + 1),
        $cap('/^a/', 'ba', 0, 1), $cap('/\\Ga/', 'ba', 0, 1), $cap('/(?<=b)a/', 'ba', 0, 1), $cap('/./u', 'é', 0, 1),
        $cap('/./u', 'é', 0, 1, ['utf8' => 'flagged']), $cap('/./u', 'aé', 0, 1, ['utf8' => 'flagged']), $cap('/./u', 'é', 0, 2),
        $cap('/./u', 'é', 0, -1), $cap('/./', 'é', 0, 1), $cap('/$/u', 'é', 0, 2, ['utf8' => 'flagged']), $cap('/./u', "a\xFF", 0, 1),
        $cap('/a/', 'a', 1), $cap('/a/', 'a', 2), $cap('/a/', 'a', 255), $cap('/a/', 'a', 256 | 1), $cap('/a/', 'a', 1024),
        $cap('/a/', 'a', 768 | 1024), $cap('/a/', 'a', -1), $cap('/a/', 'a', PREG_SET_ORDER), $cap('/(/', 'a', 1),
        $cap('/(/', 'a', 0, PHP_INT_MIN), $cap('/a/', 'a', 1, PHP_INT_MIN), $cap('/(*NOTEMPTY)a*/', 'b'), $cap('/(*NOTEMPTY)a*/u', 'b'),
    );
    $g['captures'] = $captures;

    // ini settings: pcre.backtrack_limit, pcre.recursion_limit, pcre.jit.
    $g['ini'] = [
        ['ini' => ['pcre.backtrack_limit' => '100']], c('/(a+)+$/', 'aaaaaaab'), c('/(?:\\D+|<\\d+>)*[!?]/', 'foobar'),
        c('/(*NO_JIT)(a+)+$/', 'aaaaaaab'), ['ini' => ['pcre.backtrack_limit' => '0']], c('/a/', 'a'), c('/(*NO_JIT)a/', 'a'),
        c('/(*LIMIT_MATCH=5)a/', 'a'), ['ini' => ['pcre.backtrack_limit' => '-1']], c('/(a+)+$/', rep('a', 22, '', 'b')),
        ['ini' => ['pcre.backtrack_limit' => '1000000']], c('/(a+)+$/', rep('a', 22, '', 'b')),
        ['ini' => ['pcre.recursion_limit' => '50']], c('/(*NO_JIT)^(a|b)*$/', rep('a', 30)), c('/(*NO_JIT)^(a|b)*$/', rep('a', 60)),
        c('/^(a|b)*$/', rep('a', 60)), c('/x*|^(a|b)*$/', rep('a', 60)), ['ini' => ['pcre.recursion_limit' => '0']],
        c('/(*NO_JIT)a/', 'a'), c('/(*NO_JIT)(a)/', 'a'), ['ini' => ['pcre.recursion_limit' => '100000']],
        c('/^(a|b)*$/', rep('a', $b['jit_machine_stack'])), ['ini' => ['pcre.jit' => '0']],
        c('/^(a|b)*$/', rep('a', $b['jit_machine_stack'] - 1)), c('/^(a|b)*$/', rep('a', $b['jit_machine_stack'])),
        c('/^(a|b)*$/S', rep('a', $b['jit_stack'])), c('/^(a|b)*$/S', rep('a', $b['depth'])),
        c('/(?:a|a)+$/', rep('a', $b['backtrack_alt_nojit'], '', 'b')), c('/(*NOTEMPTY)a*/u', 'b'),
        ['ini' => ['pcre.jit' => '1']], c('/^(a|b)*$/', rep('a', $b['jit_machine_stack'])),
        c('/^(a|b)*$/S', rep('a', $b['jit_stack'])), c('/^(a|b)*$/X', rep('a', $b['jit_stack'])),
    ];

    // JIT allocation failure: warning, pcre.jit switched off for later patterns, cached patterns unchanged.
    $nomem = rep('(a)', $b['jit_nomem'], '/', '/');
    $g['jit_failure'] = [
        c('/^(a|b)*$/', rep('a', $b['jit_stack'])), c(rep('(a)', $b['jit_nomem'] - 1, '/', '/'), ''),
        c('/(?:a|a)+$/', rep('a', $b['backtrack_alt_nojit'], '', 'b'), ['ops' => ['match']]),
        c($nomem, '', ['ops' => ['twig_runtime']]), c($nomem, ''), c($nomem, rep('a', $b['jit_nomem'])),
        c('/^(a|b)*$/', rep('a', $b['jit_stack'])), c('/^(a|b)*$/i', rep('a', $b['jit_stack'])),
        c('/(?:a|a)+$/', rep('a', $b['backtrack_alt_nojit'], '', 'b'), ['ops' => ['match']]),
        c('/(?:a|a)+$/i', rep('a', $b['backtrack_alt_nojit'], '', 'b'), ['ops' => ['match']]), c('/(*NOTEMPTY)a*/u', 'b'),
        ['ini' => ['pcre.jit' => '1']], c('/^(a|b)*$/m', rep('a', $b['jit_stack'])),
        c(rep('(a)', $b['jit_nomem'], '/', '/i'), '', ['ops' => ['twig_compile', 'twig_runtime', 'match']]),
        c('/^(a|b)*$/s', rep('a', $b['jit_stack'])),
    ];

    // Twig's messages for every kind of failure.
    $g['twig'] = [
        c('/(/', 'abc'), c('', 'abc'), c('abc', 'abc'), c('/a/e', 'a'), c('(abc', 'abc'), c('/abc', 'abc'), c("/a/\x00", 'a'),
        c('/(?:\\D+|<\\d+>)*[!?]/', 'foobar foobar foobar'), c('/./u', "\xFF"), c('/^(a|b)*$/', rep('a', $b['jit_stack'])),
        c('/(*NO_JIT)^(a|b)*$/', rep('a', $b['depth'])), c('/(*LIMIT_HEAP=1)(*NO_JIT)(a)*$/', rep('a', 4000)),
        c('/\\C/u', 'a'), c("~(abc~", 'abc'), c("/\xFF/", "\xFF"), c("\xFF(\xFF", "\xFF("), c('/^\\d+$/', '123'),
        c('/^\\d+$/', ''), c('/^$/', ''), c('#"#', '"'), c('/%s/', '%s'),
        c('/a/', 'a', ['ops' => ['match', 'twig_compile_null']]),
        c('/(?:\\D+|<\\d+>)*[!?]/', 'foobar foobar foobar', ['ops' => ['match', 'twig_compile_null', 'match']]),
    ];

    // preg_match() calls of php-src's own tests (PHPSRC/ext/pcre/tests): null_bytes.phpt, delimiters.phpt,
    // preg_match_caseless_restrict.phpt, bug76909.phpt, dollar_endonly.phpt, ungreedy.phpt, marks.phpt,
    // and recursion_limit.phpt / backtrack_limit.phpt with their --INI-- settings (as preg_match).
    $t = [
        c("\0//i", ''), c("/\0/i", ''), c("/\0/i", "\0"), c("//\0i", ''), c("//i\0", ''), c("/\\\0/i", ''),
        c("/\\\0/i", "\\\0"), c("\0[]i", ''), c("[\0]i", ''), c("[\0]i", "\0"), c("[]\0i", ''), c("[]i\0", ''),
        c("[\\\0]i", ''), c("[\\\0]i", "\\\0"), c("/abc\0def/", 'abc'), c("/abc\0def/", "abc\0def"), c("/abc\0def/", "abc\0fed"),
        c("[abc\0def]", 'abc'), c("[abc\0def]", "abc\0def"), c("[abc\0def]", "abc\0fed"),
        c('', ''), c('      ', ''), c('@@', ''), c('12', ''), c('<>', ''), c('~a', ''), c('@\\@\\@@', '@@'), c('//z', '@@'),
        c('{', ''), c("\0\0", ''),
        c('/^[\\x{0100}-\\x{017f}]{1,63}$/iu', ' domain.com'), c('/(*NO_JIT)^[\\x{0100}-\\x{017f}]{1,63}$/iu', ' domain.com'),
        c('/a$/', "a\n"), c('/a$/D', "a\n"), c('/a$/m', "a\nb"), c('/<.*>/U', '<a><b>'), c('/<.*?>/U', '<a><b>'),
        c('/(*MARK:A)x|(*MARK:B)y/', 'y'), c('/a(*MARK:m1)b(*MARK:m2)c/', 'abc'), c('/(*MARK:A)(*SKIP)x|y/', 'axy'),
    ];
    foreach (['AskZ', 'aSKz', "A\u{17f}kZ", "As\u{212a}Z"] as $s) {
        foreach (['/AskZ/iur', '/AskZ/iu', '/[AskZ]+/iur', '/[AskZ]+/iu', '/[\\x{17f}\\x{212a}]+/iur', '/[\\x{17f}\\x{212a}]+/iu',
            '/A\\x{17f}\\x{212a}Z/iu', '/A\\x{17f}\\x{212a}Z/iur'] as $p) {
            $t[] = c($p, $s, ['ops' => ['match']]);
        }
    }
    array_push($t,
        c('/[^s]+/iur', "A\u{17f}Z"), c('/[^s]+/iu', "A\u{17f}Z"), c('/[^k]+/iur', "A\u{212a}Z"), c('/[^k]+/iu', "A\u{212a}Z"),
        c('/k/iu', "\u{212A}"), c('/k/iur', "\u{212A}"),
        ['ini' => ['pcre.recursion_limit' => '2', 'pcre.jit' => '0']],
        c('/\\p{Ll}(\\p{L}((\\p{Ll}\\p{Ll})))/', 'aeiou'), c('/\\p{Ll}\\p{L}\\p{Ll}\\p{Ll}/', 'aeiou'),
        ['ini' => ['pcre.recursion_limit' => '100000', 'pcre.backtrack_limit' => '2']],
        c('/.*\\p{N}/', '0123456789'), c('/\\p{Nd}/', '0123456789'),
        ['ini' => ['pcre.backtrack_limit' => '1000000', 'pcre.jit' => '1']], c('/.*\\p{N}/', '0123456789'),
    );
    $g['php_tests'] = $t;

    return $g;
}

// ---------------------------------------------------------------------------------------------------------
// Main

$flags = JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR;

if (($argv[1] ?? '') === '--group') {
    $b = json_decode(base64_decode($argv[3]), true, 512, JSON_THROW_ON_ERROR);
    $nojit = ($argv[4] ?? '') === 'nojit';
    if ($nojit) {
        ini_set('pcre.jit', '0');
    }
    $steps = [];
    foreach (groups($b)[$argv[2]] as $step) {
        $result = run_step($step, $nojit);
        if ($result !== null) {
            $steps[] = json_encode($result, $flags);
        }
    }
    echo implode("\n", $steps), "\n";
    exit(0);
}

// "groups": PHP's default pcre.jit=1. "groups_nojit": the same steps in processes that start with
// ini_set('pcre.jit', '0') and never switch the JIT back on (pcre.jit ini steps are dropped), i.e. PCRE2's
// interpreter, which is what a build without the JIT runs.
$b = bounds();
$out = ['jit' => [], 'nojit' => []];
foreach (['jit', 'nojit'] as $mode) {
    foreach (array_keys(groups($b)) as $name) {
        $cmd = escapeshellarg(PHP_BINARY).' '.escapeshellarg(__FILE__).' --group '.escapeshellarg($name).' '
            .escapeshellarg(base64_encode(json_encode($b))).' '.escapeshellarg($mode);
        $lines = rtrim((string) shell_exec($cmd), "\n");
        foreach (explode("\n", $lines) as $line) {
            json_decode($line, true, 512, JSON_THROW_ON_ERROR); // fail loudly on a broken child
        }
        $out[$mode][] = '{"group":'.json_encode($name).',"steps":['."\n".str_replace("\n", ",\n", $lines)."\n]}";
    }
}
echo '{"generator":"preg_match.gen.php","php":'.json_encode(PHP_VERSION).',"pcre":'.json_encode(PCRE_VERSION)
    .',"twig":'.json_encode(Twig\Environment::VERSION).',"bounds":'.json_encode($b).',"groups":['."\n"
    .implode(",\n", $out['jit'])."\n],\"groups_nojit\":[\n".implode(",\n", $out['nojit'])."\n]}\n";

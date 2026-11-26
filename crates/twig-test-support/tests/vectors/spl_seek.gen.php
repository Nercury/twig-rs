<?php
// Generates spl_seek.json: SplObjectStorage::seek() (PHPSRC/ext/spl/spl_observer.c) on storages of stdClass objects
// with `n` = their position and info "i<n>", after other iteration steps.
//
//   cd crates/twig-test-support/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/spl_seek.gen.php > spl_seek.json
//
// Every step records its result (or exception), its diagnostics and the iterator state after it: valid(), key(),
// current()->n (or the exception) and getInfo().
declare(strict_types=0);
$diags = [];
set_error_handler(static function (int $no, string $msg) use (&$diags): bool { $diags[] = $msg; return true; });
function state(SplObjectStorage $s): array {
    $valid = $s->valid();
    try { $cur = $s->current()->n; } catch (\Throwable $e) { $cur = get_class($e).': '.$e->getMessage(); }
    return ['valid' => $valid, 'key' => $s->key(), 'current' => $cur, 'info' => $s->getInfo()];
}
function storage(int $n): SplObjectStorage {
    $s = new SplObjectStorage();
    for ($i = 0; $i < $n; $i++) { $o = new stdClass(); $o->n = $i; $s[$o] = "i$i"; }
    return $s;
}
$scenarios = [
    'seek0' => fn () => [3, [['seek', 0]]],
    'seek2' => fn () => [3, [['seek', 2]]],
    'seek2_1' => fn () => [3, [['seek', 2], ['seek', 1]]],
    'seek3' => fn () => [3, [['seek', 3]]],
    'seekm1' => fn () => [3, [['seek', -1]]],
    'end_seek2' => fn () => [3, [['rewind'], ['next'], ['next'], ['next'], ['seek', 2]]],
    'end_seek0' => fn () => [3, [['rewind'], ['next'], ['next'], ['next'], ['seek', 0]]],
    'end_seek1' => fn () => [3, [['rewind'], ['next'], ['next'], ['next'], ['seek', 1]]],
    'far_seek2' => fn () => [3, [['next'], ['next'], ['next'], ['next'], ['next'], ['seek', 2]]],
    'far_seek1_of4' => fn () => [4, [['next'], ['next'], ['next'], ['next'], ['next'], ['seek', 3]]],
    'seek_str' => fn () => [3, [['seek', '1']]],
    'seek_bad' => fn () => [3, [['seek', 'x']]],
    'seek_float' => fn () => [3, [['seek', 1.5]]],
    'seek_null' => fn () => [3, [['seek', null]]],
    'seek_none' => fn () => [3, [['seek']]],
    'seek_two' => fn () => [3, [['seek', 1, 2]]],
    'empty_seek0' => fn () => [0, [['seek', 0]]],
    'detach_seek1' => fn () => [3, [['detach', 1], ['seek', 1]]],
    'seek1_back0' => fn () => [3, [['seek', 2], ['seek', 1], ['seek', 1], ['seek', 0]]],
    'seek_after_next' => fn () => [4, [['seek', 1], ['next'], ['seek', 3], ['seek', 2]]],
    'seek_named' => fn () => [3, [['seek_named', 2]]],
];
$out = [];
foreach ($scenarios as $name => $make) {
    [$n, $ops] = $make();
    $s = storage($n);
    $objects = []; foreach ($s as $i => $o) { $objects[] = $o; } $s->rewind();
    $steps = [];
    foreach ($ops as $op) {
        $diags = [];
        try {
            switch ($op[0]) {
                case 'seek': $r = $s->seek(...array_slice($op, 1)); break;
                case 'seek_named': $r = $s->seek(offset: $op[1]); break;
                case 'rewind': $r = $s->rewind(); break;
                case 'next': $r = $s->next(); break;
                case 'detach': $r = $s->offsetUnset($objects[$op[1]]); break;
            }
            $res = ['ok' => $r];
        } catch (\Throwable $e) {
            $res = ['err' => [get_class($e), $e->getMessage()]];
        }
        $steps[] = ['op' => $op, 'result' => $res, 'diagnostics' => $diags, 'state' => state($s)];
    }
    $out[] = ['name' => $name, 'n' => $n, 'steps' => $steps];
}
echo json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_PRESERVE_ZERO_FRACTION), "\n";

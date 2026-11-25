<?php
// Generates array_ops.json: random sequences of array writes, appends and unsets, recording the keys
// appends receive, the final key order and the next append key (nNextFreeElement, packed vs hash
// layout, copy-on-write separation).
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/array_ops.gen.php > array_ops.json
//
// ops: ["set", int] ($a[k] = 1), ["sets", str], ["push"] ($a[] = 2), ["unset", int], ["unsets", str],
// ["copy"] ($keep[] = $a, so the next write separates).

declare(strict_types=0);
require __DIR__.'/common.inc.php';
mt_srand(2026);

$cases = [];
for ($n = 0; $n < 4000; $n++) {
    $ops = [];
    $count = mt_rand(1, 45);
    $max = [3, 8, 12, 20, 40, 70][mt_rand(0, 5)];
    for ($i = 0; $i < $count; $i++) {
        $r = mt_rand(0, 99);
        $ops[] = match (true) {
            $r < 30 => ['push'],
            $r < 55 => ['set', mt_rand(-3, $max)],
            $r < 83 => ['unset', mt_rand(-3, $max)],
            $r < 88 => ['sets', 'k'.mt_rand(0, 3)],
            $r < 91 => ['unsets', 'k'.mt_rand(0, 3)],
            $r < 93 => ['set', [PHP_INT_MAX, PHP_INT_MIN, PHP_INT_MAX - 1, -1000][mt_rand(0, 3)]],
            default => ['copy'],
        };
    }
    $a = [];
    $keep = [];
    $pushes = [];
    foreach ($ops as $op) {
        switch ($op[0]) {
            case 'set':
            case 'sets':
                $a[$op[1]] = 1;
                break;
            case 'push':
                try {
                    $a[] = 2;
                    $pushes[] = array_key_last($a);
                } catch (\Error $e) {
                    $pushes[] = $e->getMessage();
                }
                break;
            case 'unset':
            case 'unsets':
                unset($a[$op[1]]);
                break;
            case 'copy':
                $keep[] = $a;
                break;
        }
    }
    $keys = array_keys($a);
    try {
        $b = $a;
        $b[] = null;
        $next = array_key_last($b);
    } catch (\Error $e) {
        $next = $e->getMessage();
    }
    $cases[] = ['ops' => $ops, 'pushes' => $pushes, 'keys' => $keys, 'next' => $next];
}
emit('array_ops.gen.php', ['cases' => $cases]);

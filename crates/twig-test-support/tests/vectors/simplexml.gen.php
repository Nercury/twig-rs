<?php
// Generates simplexml.json: SimpleXMLElement objects (PHPSRC/ext/simplexml/simplexml.c) reached from a document root
// by property reads, dimension reads and children()/attributes(), and what they answer.
//
//   cd crates/twig-test-support/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/simplexml.gen.php > simplexml.json
//
// A case starts at `new SimpleXMLElement($docs[doc])` and applies `steps`: ['p', name] reads a property ($o->name),
// ['pi', name] reads it the way `??` does ($o->name ?? null), ['d', key] reads a dimension ($o[key]) and
// ['m', method] calls a method without arguments. Each of `finals` then runs on the result, in order, on the same
// object (each records its value or exception, and the diagnostics it raised). Values are described by `describe()`:
// a SimpleXMLElement as ['sxe', getName(), (string), count(), asXML()], an array as ['array', [[key, value]...]].
declare(strict_types=0);

$docs = [
    'a' => '<a b="1" c=""><c>t</c><c>u</c><d x="0" y="2"><e>v</e><e/></d>tail</a>',
    'r' => '<r>text</r>',
    'n' => '<n><m>0</m><m></m><p/><q> </q><s>x<t/>y</s></n>',
    'w' => "<w>\n  <i k=\"1\">1</i>\n  <i>2</i>\n</w>",
    'e' => '<e/>',
    'x' => '<?xml version="1.0" encoding="UTF-8"?><x><y z="w">1</y></x>',
];

$object = ['string', 'int', 'float', 'bool', 'count', 'name', 'xml', 'array', 'debug', 'json', 'iter'];
$walk = array_merge($object, ['walk']);
$cases = [
    ['a', [], array_merge($walk, [
        ['isset_p', 'c'], ['isset_p', 'nope'], ['isset_p', 'b'], ['isset_p', '0'], ['empty_p', 'c'], ['empty_p', 'd'],
        ['exists_p', 'c'], ['exists_p', 'nope'], ['isset_d', 'b'], ['isset_d', 'c'], ['isset_d', 'nope'],
        ['empty_d', 'b'], ['empty_d', 'c'], ['isset_d', 0], ['isset_d', 1], ['empty_d', 0], ['isset_d', 1.5],
        ['eq', [['p', 'c']]], ['eq', [['m', 'children']]], ['eq', [['d', 'b']]], ['eq', [['p', 'c'], ['d', 0]]],
    ])],
    ['a', [['p', 'c']], array_merge($walk, [
        ['isset_d', 0], ['isset_d', 1], ['isset_d', 2], ['isset_d', -1], ['empty_d', 0], ['isset_p', 'nope'],
        ['isset_p', 'c'], ['isset_d', 'b'], ['eq', [['p', 'c'], ['d', 0]]], ['eq', [['p', 'd']]],
    ])],
    ['a', [['p', 'c'], ['d', 1]], $object],
    ['a', [['p', 'c'], ['d', 0]], array_merge($object, [['eq', [['m', 'children'], ['d', 0]]], ['isset_d', 0], ['isset_d', 1]])],
    ['a', [['p', 'c'], ['d', 2]], ['describe']],
    ['a', [['p', 'c'], ['d', -1]], ['describe']],
    ['a', [['p', 'c'], ['d', '1']], ['describe']],
    ['a', [['p', 'd']], array_merge($walk, [['isset_d', 'x'], ['empty_d', 'x'], ['isset_d', 'y'], ['isset_p', 'e'], ['empty_p', 'e']])],
    ['a', [['p', 'd'], ['p', 'e']], array_merge($walk, [['empty_d', 0], ['empty_d', 1], ['isset_d', 1]])],
    ['a', [['p', 'd'], ['p', 'e'], ['d', 1]], $object],
    ['a', [['p', 'd'], ['d', 'x']], array_merge($object, [['isset_d', 0], ['empty_d', 0], ['isset_p', 'x']])],
    ['a', [['p', 'nope']], array_merge($walk, [['isset_d', 0], ['isset_p', 'x'], ['eq', []]])],
    ['a', [['pi', 'nope']], ['describe']],
    ['a', [['pi', 'c']], ['describe', 'count']],
    ['a', [['d', 'b']], array_merge($walk, [['isset_d', 0], ['empty_d', 0], ['isset_d', 1], ['isset_p', 'x'], ['eq', [['m', 'attributes'], ['d', 0]]]])],
    ['a', [['d', 'c']], array_merge($object, [['empty_d', 0]])],
    ['a', [['d', 'nope']], ['describe']],
    ['a', [['d', 0]], array_merge($object, [['eq', []]])],
    ['a', [['d', 1]], ['describe']],
    ['a', [['d', true]], ['describe']],
    ['a', [['m', 'children']], array_merge($walk, [
        ['isset_d', 0], ['isset_d', 2], ['isset_d', 3], ['isset_p', 'c'], ['isset_p', 'd'], ['empty_p', 'c'], ['isset_d', 'b'],
    ])],
    ['a', [['m', 'children'], ['p', 'c']], $object],
    ['a', [['m', 'children'], ['d', 1]], $object],
    ['a', [['m', 'children'], ['d', 2]], $object],
    ['a', [['m', 'children'], ['d', 'b']], ['describe']],
    ['a', [['m', 'attributes']], array_merge($walk, [
        ['isset_p', 'b'], ['isset_p', 'c'], ['isset_p', 'x'], ['empty_p', 'b'], ['empty_p', 'c'], ['isset_d', 0], ['isset_d', 1],
        ['isset_d', 2], ['empty_d', 1], ['isset_d', 'b'], ['empty_d', 'c'], ['exists_p', 'b'],
    ])],
    ['a', [['m', 'attributes'], ['p', 'b']], $object],
    ['a', [['m', 'attributes'], ['p', 'c']], $object],
    ['a', [['m', 'attributes'], ['p', 'x']], ['describe']],
    ['a', [['m', 'attributes'], ['d', 1]], $object],
    ['a', [['m', 'attributes'], ['d', 'b']], $object],
    ['a', [['m', 'attributes'], ['m', 'children']], ['describe']],
    ['a', [['m', 'attributes'], ['m', 'attributes']], ['describe']],
    ['a', [['p', 'c'], ['m', 'children']], $object],
    ['a', [['p', 'c'], ['m', 'attributes']], $object],
    ['a', [['p', 'd'], ['m', 'attributes']], $walk],
    ['a', [['p', 'd'], ['m', 'attributes'], ['p', 'y']], $object],
    ['a', [['p', 'd'], ['m', 'children']], $walk],
    ['a', [['d', 'b'], ['m', 'children']], $object],
    ['a', [['d', 'b'], ['m', 'attributes']], $object],
    ['a', [['p', 'c'], ['p', 'nope']], $object],
    ['n', [], array_merge($object, [['empty_p', 'm'], ['empty_p', 'p'], ['empty_p', 'q'], ['empty_p', 's'], ['isset_p', 'p']])],
    ['n', [['p', 'm']], array_merge($walk, [['empty_d', 0], ['empty_d', 1], ['isset_d', 1]])],
    ['n', [['p', 'm'], ['d', 1]], $object],
    ['n', [['p', 'p']], $object],
    ['n', [['p', 'q']], $object],
    ['n', [['p', 's']], $object],
    ['n', [['m', 'children']], $walk],
    ['w', [], $walk],
    ['w', [['p', 'i']], $walk],
    ['w', [['m', 'children']], $walk],
    ['r', [], $walk],
    ['r', [['m', 'children']], $object],
    ['r', [['m', 'attributes']], $object],
    ['e', [], $walk],
    ['e', [['m', 'children']], $object],
    ['x', [], $object],
    ['x', [['p', 'y']], $object],
    ['x', [['p', 'y'], ['m', 'attributes']], $object],
];

$diags = [];
set_error_handler(static function (int $no, string $msg) use (&$diags): bool {
    $diags[] = $msg;

    return true;
});

function describe($v)
{
    if ($v instanceof SimpleXMLElement) {
        $xml = $v->asXML();

        return ['sxe', $v->getName(), (string) $v, count($v), $xml];
    }
    if (is_array($v)) {
        $items = [];
        foreach ($v as $k => $x) {
            $items[] = [$k, describe($x)];
        }

        return ['array', $items];
    }

    return $v;
}

function step($o, array $step)
{
    return match ($step[0]) {
        'p' => $o->{$step[1]},
        'pi' => $o->{$step[1]} ?? null,
        'd' => $o[$step[1]],
        'm' => $o->{$step[1]}(),
    };
}

function walk(SimpleXMLElement $root, array $steps)
{
    $o = $root;
    foreach ($steps as $s) {
        $o = step($o, $s);
    }

    return $o;
}

function run_final($o, $final, SimpleXMLElement $root)
{
    $op = is_array($final) ? $final[0] : $final;

    return match ($op) {
        'describe' => describe($o),
        'string' => (string) $o,
        'int' => (int) $o,
        'float' => (float) $o,
        'bool' => (bool) $o,
        'count' => count($o),
        'name' => $o->getName(),
        'xml' => $o->asXML(),
        'array' => describe((array) $o),
        'debug' => describe($o->__debugInfo()),
        'json' => json_encode($o),
        'iter' => (static function () use ($o) {
            $out = [];
            foreach ($o as $k => $x) {
                $out[] = [$k, describe($x)];
            }

            return $out;
        })(),
        'walk' => (static function () use ($o) {
            $out = [];
            $o->rewind();
            while ($o->valid()) {
                $out[] = [$o->key(), describe($o->current()), $o->hasChildren(), describe($o->getChildren())];
                $o->next();
            }
            $out[] = [$o->valid(), describe($o->getChildren())];

            return $out;
        })(),
        'isset_p' => isset($o->{$final[1]}),
        'empty_p' => empty($o->{$final[1]}),
        'exists_p' => property_exists($o, $final[1]),
        'isset_d' => isset($o[$final[1]]),
        'empty_d' => empty($o[$final[1]]),
        'eq' => $o == walk($root, $final[1]),
    };
}

$out = [];
foreach ($cases as [$doc, $steps, $finals]) {
    $root = new SimpleXMLElement($docs[$doc]);
    $case = ['doc' => $docs[$doc], 'steps' => $steps, 'results' => []];
    $diags = [];
    try {
        $o = walk($root, $steps);
    } catch (\Throwable $e) {
        $case['error'] = [get_class($e), $e->getMessage()];
        $out[] = $case;
        continue;
    }
    $case['step_diags'] = $diags;
    foreach ($finals as $final) {
        $diags = [];
        try {
            $r = ['final' => $final, 'ok' => run_final($o, $final, $root)];
        } catch (\Throwable $e) {
            $r = ['final' => $final, 'error' => [get_class($e), $e->getMessage()]];
        }
        $r['diags'] = $diags;
        $case['results'][] = $r;
    }
    $out[] = $case;
}
echo json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_PRESERVE_ZERO_FRACTION), "\n";

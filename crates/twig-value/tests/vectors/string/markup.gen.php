<?php
// Generates markup.json: nl2br() and strip_tags() (allow lists as strings and arrays, the state machine on
// hand-written and random tag soup).
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/string/markup.gen.php > string/markup.json

declare(strict_types=0);
require __DIR__.'/calls.inc.php';

mt_srand(4876);
$cases = [];
foreach (['', 'a', "a\nb\r\nc\n\rd\re", "\n", "\r", "\r\n\r\n", "\n\n\r\r", "x\r", "\n\r\n", "a\0\nb"] as $s) {
    $cases[] = ['nl2br1', [$s]];
    foreach ([true, false, 0, 1, '', 'x', null, 1.5, [], new stdClass()] as $x) {
        $cases[] = ['nl2br2', [$s, $x]];
    }
}
foreach (odd_values() as $v) {
    $cases[] = ['nl2br1', [$v]];
}
for ($k = 0; $k < 300; $k++) {
    $cases[] = ['nl2br2', [rand_str("ab\r\n", 0, 12), (bool) mt_rand(0, 1)]];
}

$docs = [
    '', 'plain', '<p>Hello <b>World</b><br/><!-- c --> <?php echo 1; ?>x</p> a < b', 'a < b > c', '1<2', "a<\nb>c",
    '<P>a</P>', '<b>x</b><i>y</i>', '<a href="x>y">link</a>', "<a href='x>y'>l</a>", '<a title="it\'s">x</a>',
    '<!DOCTYPE html><html><body>t</body></html>', '<!doctype html>x', '<?xml version="1.0"?><r>t</r>',
    '<?xml-stylesheet a="b"?>z', '<!-- a > b -->c', '<!--x-->y', '<!-- <b> -->z', '<script>if (a < b) {}</script>t',
    '<?php if ($a > 1) { echo "x>y"; } ?>after', '<? echo (">") ?>after', '<a<b>c>d', '<a <b>c', 'x<!x>y', 'x<!>y',
    "x<\0b>y", "\0a\0", '<br/>', '</b>', '< b>', '<b >', '<b/>', '<B>x</B>', '<a/b>x', '<a>x</a >', '<<a>>', '<a>>b',
    'a>b', '<a"b>c"d>e', "<a 'b>c'>d", '<?xml-x?>', '<!-- -- -->x', '<!--->x-->y', 'end<', 'end<a', 'end<!--', 'end<?',
    '<a\>b>c', '<?php "a\"b" ?>c', '<!DOCTYPE x [<!ENTITY y "z">]>w', '<a b="c"d>e', '<sCrIpT>z</ScRiPt>', '<a<!-- x -->>y',
    "<?xml\nversion?>z", '<a href=\'x\' title="y">t</a>', '<p><br><br/><br /></p>', '<tag attr=">">v', '<?=a?>b',
];
$allows = [null, '', '<p>', '<b><i>', '<a>', 'b', '<br>', '<p><br>', '<B>', '<b', '<html><body>', '< html>', "<b>\0<i>",
    '<a href>', '<script>', '<!DOCTYPE html>', '<?xml>', '<a/b>', '</b>', ['b'], ['b', 'i'], ['p', 'br'], [], ['B'],
    ['a', ['x']], [1, 'b'], ['<b>'], [new S('i')], [1.5]];
foreach ($docs as $doc) {
    $cases[] = ['strip_tags1', [$doc]];
    foreach ($allows as $allow) {
        $cases[] = ['strip_tags2', [$doc, $allow]];
    }
}
foreach (odd_values() as $v) {
    $cases[] = ['strip_tags1', [$v]];
    $cases[] = ['strip_tags2', ['<b>1</b><i>2</i>', $v]];
}
$pieces = ['<', '>', '<b>', '</b>', '<i>', '<!--', '-->', '<!', '<?', '?>', '<?xml', '<!DOCTYPE', '"', "'", '\\', ' ', "\n",
    'a', 'x', '(', ')', '-', '/', "\0", '<br/>', '=', 'php', 'b', 'i', '>>', '<<'];
for ($k = 0; $k < 3000; $k++) {
    $allow = [null, '', '<b>', '<i><b>', '<br>', '<xml>', '<!doctype>', ['b', 'i']][mt_rand(0, 7)];
    $cases[] = ['strip_tags2', [rand_pieces($pieces, 0, 14), $allow]];
}

emit('markup.gen.php', ['cases' => run_cases($cases)]);

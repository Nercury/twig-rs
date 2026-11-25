"""
Reference (non-regex-engine-specific) port of Twig 3.31.1's .test fixture parser
(Twig\\Test\\IntegrationTestCase::assembleTests() + parseTemplates()), written as plain string scanning so it can be
transliterated to Rust without lookahead/backtracking regex support.

Validated against PHP's own PCRE parse of all 488 fixtures (see --check).

    python -I fixture_parser_reference.py --check <fixtures-parsed.json> <Fixtures dir>

fixtures-parsed.json is produced by tools/parse-fixtures.php (the exact PHP regexes).
"""
import json
import os
import sys

WS = ' \t\n\x0b\x0c\r'  # PCRE2 \s without /u: space, HT, LF, VT, FF, CR


def lstrip_ws(s):
    return s.lstrip(WS)


def parse_templates(block):
    """parseTemplates(): split on every '--TEMPLATE' occurrence.  Each piece is '--TEMPLATE' + optional '(name)' + '--'
    + body.  Name defaults to 'index.twig' (also when the name is empty: '--TEMPLATE()--').  The LAST body loses one
    trailing "\\n" (PCRE '$' without /D also matches before a final newline and the body is lazy)."""
    templates = {}
    pieces = []
    pos = block.find('--TEMPLATE')
    while pos != -1:
        nxt = block.find('--TEMPLATE', pos + len('--TEMPLATE'))
        pieces.append(block[pos:nxt if nxt != -1 else len(block)])
        pos = nxt
    for i, piece in enumerate(pieces):
        rest = piece[len('--TEMPLATE'):]
        name = ''
        if rest.startswith('('):
            close = rest.find(')--')  # (.*?) lazy, then ')--'
            name, rest = rest[1:close], rest[close + 1:]
        assert rest.startswith('--'), piece[:40]
        body = rest[2:]
        if i == len(pieces) - 1 and body.endswith('\n'):
            body = body[:-1]
        key = name if name not in ('', '0') else 'index.twig'  # PHP: $match[1] ?: 'index.twig' ('0' is falsy!)
        templates[key] = body  # duplicate names: last one wins (PHP array assignment), insertion position of first
    return templates


def parse_fixture(text):
    t0 = text.find('--TEST--')
    if t0 == -1:
        return None
    i_tpl = text.find('--TEMPLATE', t0)
    if i_tpl == -1:
        return None
    head = text[t0 + len('--TEST--'):i_tpl]
    condition = ''
    deprecation = ''
    # head = message [--CONDITION-- cond] [--DEPRECATION-- dep]  (that order)
    i_cond = head.find('--CONDITION--')
    i_dep = head.find('--DEPRECATION--')
    msg_end = min([x for x in (i_cond, i_dep) if x != -1] or [len(head)])
    message = head[:msg_end].strip(WS)
    if i_cond != -1:
        # greedy (.*): condition runs up to the DEPRECATION marker (if any) or the first --TEMPLATE; keeps the trailing
        # newline (the following \s* matches empty), leading whitespace is eaten by \s*
        cond_end = i_dep if i_dep != -1 else len(head)
        condition = lstrip_ws(head[i_cond + len('--CONDITION--'):cond_end])
    if i_dep != -1:
        deprecation = head[i_dep + len('--DEPRECATION--'):].strip(WS)  # lazy (.*?) between \s* and \s*

    i_exc = text.find('--EXCEPTION--', i_tpl)
    if i_exc != -1:
        # exception form: TEMPLATE... [--DATA-- data] --EXCEPTION-- exception
        i_data = text.find('--DATA--', i_tpl)
        tpl_end = i_data if (i_data != -1 and i_data < i_exc) else i_exc
        block = text[i_tpl:tpl_end].rstrip(WS)  # lazy template body + \s* before DATA/EXCEPTION
        data = ''
        if i_data != -1 and i_data < i_exc:
            data = lstrip_ws(text[i_data + len('--DATA--'):i_exc])  # greedy: keeps trailing whitespace
        exception = lstrip_ws(text[i_exc + len('--EXCEPTION--'):])
        return {
            'message': message, 'condition': condition, 'deprecation': deprecation,
            'templates': parse_templates(block), 'exception': exception,
            'outputs': [{'data': data, 'config': None, 'expect': ''}],
        }

    i_data = text.find('--DATA--', i_tpl)
    if i_data == -1 or text.find('--EXPECT--', i_data) == -1:
        return None
    block = text[i_tpl:i_data]  # templates end right before the first --DATA-- (no \s*)
    outputs = []
    # preg_match_all('/--DATA--(.*?)(?:--CONFIG--(.*?))?--EXPECT--(.*?)(?=\-\-DATA\-\-|$)/s') over the WHOLE text
    pos = text.find('--DATA--')
    while pos != -1:
        j_exp = text.find('--EXPECT--', pos + 8)
        if j_exp == -1:
            break
        seg = text[pos + 8:j_exp]
        j_cfg = seg.find('--CONFIG--')
        if j_cfg != -1:
            data, config = seg[:j_cfg], seg[j_cfg + len('--CONFIG--'):]
        else:
            data, config = seg, ''
        nxt = text.find('--DATA--', j_exp + len('--EXPECT--'))
        expect = text[j_exp + len('--EXPECT--'):nxt if nxt != -1 else len(text)]
        if nxt == -1 and expect.endswith('\n'):
            expect = expect[:-1]  # '$' matches before a final newline
        outputs.append({'data': data, 'config': config, 'expect': expect})
        pos = nxt
    return {
        'message': message, 'condition': condition, 'deprecation': deprecation,
        'templates': parse_templates(block), 'exception': False, 'outputs': outputs,
    }


def check(parsed_json, fixtures_dir):
    ref = json.load(open(parsed_json, encoding='utf-8'))
    bad = 0
    for r in ref:
        text = open(os.path.join(fixtures_dir, r['file']), encoding='utf-8', newline='').read()
        mine = parse_fixture(text)
        php = {k: r[k] for k in ('message', 'condition', 'deprecation', 'templates', 'exception', 'outputs')}
        if mine != php or list(mine['templates']) != list(php['templates']):
            bad += 1
            print('MISMATCH', r['file'])
            for k in php:
                if mine.get(k) != php[k]:
                    print('  ', k, '\n    php :', repr(php[k])[:300], '\n    mine:', repr(mine.get(k))[:300])
    print('checked %d fixtures, %d mismatches' % (len(ref), bad))
    return bad


if __name__ == '__main__':
    if len(sys.argv) == 4 and sys.argv[1] == '--check':
        sys.exit(1 if check(sys.argv[2], sys.argv[3]) else 0)
    print(__doc__)

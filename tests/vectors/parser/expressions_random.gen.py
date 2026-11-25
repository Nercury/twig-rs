"""Generator of tests/vectors/parser/expressions_random.gen.json: random Twig expression templates (valid ones and
corrupted ones) for differential testing of the parser against the PHP oracle.

    python tests/vectors/parser/expressions_random.gen.py > tests/vectors/parser/expressions_random.gen.json

The oracle results (expressions_random.json) are produced from that batch by expressions.gen.php, see its header.
Larger ad-hoc batches: python expressions_random.gen.py <count> <seed> <corruption rate> [valid] > cases.json, then
run them through the oracle and compare with `TWIG_PARSER_VECTORS=<results> cargo test --test parser_tests --
--ignored external`.
"""
import json
import random
import sys

rng = random.Random(0)
CORRUPT = 0.3
VALID = False

NAMES = ['a', 'b', 'foo', 'bar_baz', 'x1', '_self', '_context', '_charset', 'True', 'Null', 'loop',
         'in', 'is', 'and', 'or', 'matches', 'starts', 'cafÃ©', 'varargs', 'context', 'this']
SAFE_NAMES = ['a', 'b', 'foo', 'bar_baz', 'x1', '_context', 'loop', 'in', 'is', 'starts', 'café', 'context']
CONSTS = ['true', 'false', 'null', 'none', 'TRUE', 'NULL']
NUMBERS = ['0', '1', '42', '1.5', '0.1', '1e3', '1_000', '9223372036854775808', '007', '1.0', '2.5e-3']
STRINGS = ["'a'", '"b"', "''", '""', "'it\\'s'", '"x#{a}y"', '"#{b}"', '"#{ 1 + 2 }z"', "'\\n'"]
BINARY = ['or', 'xor', 'and', 'b-or', 'b-xor', 'b-and', '==', '!=', '<=>', '<', '>', '>=', '<=', 'not in', 'in',
          'matches', 'starts with', 'ends with', 'has some', 'has every', '===', '!==', '..', '+', '-', '~', '*',
          '/', '//', '%', '**', '??', '?:', '? :']
UNARY = ['not', '-', '+']
FILTERS = ['upper', 'lower', 'default', 'join', 'e', 'escape', 'raw', 'length', 'map', 'filter', 'slice', 'abs',
           'spaceless', 'format', 'title', 'first', 'reduce', 'sort', 'keys', 'round', 'date', 'default']
FUNCTIONS = ['max', 'min', 'range', 'block', 'attribute', 'constant', 'cycle', 'random', 'include',
             'source', 'enum', 'date', 'max', 'range']
TESTS = ['defined', 'null', 'none', 'even', 'odd', 'empty', 'iterable', 'same as', 'divisible by', 'constant',
         'sequence', 'mapping', 'true', 'defined', 'same as']
PUNCT_NOISE = ['(', ')', '[', ']', '{', '}', ',', ':', '.', '|', '?', '=', '=>', '...', '?.', 'is', 'not']


def ws():
    r = rng.random()
    if r < 0.85:
        return ' '
    if r < 0.95:
        return '\n'
    return '  \n '


def name():
    return rng.choice(SAFE_NAMES if VALID else NAMES)


def args(depth, named_ok=True):
    if depth > 4:
        return '()'
    n = rng.randint(0, 3)
    parts = []
    spread = False
    for _ in range(n):
        r = rng.random()
        if r < 0.15 and not (VALID and _ < n - 1):
            parts.append('...' + expr(depth + 1))
            spread = True
        elif named_ok and r < 0.4:
            sep = rng.choice([' = ', ': ', '='])
            parts.append(name() + sep + expr(depth + 1))
        else:
            parts.append(expr(depth + 1))
    s = ', '.join(parts)
    if parts and rng.random() < 0.1:
        s += ','
    return '(' + s + ')'


def primary(depth):
    if depth > 5:
        return rng.choice([name(), rng.choice(NUMBERS), rng.choice(CONSTS)])
    r = rng.random()
    if depth > 4:
        r = r * 0.45
    if r < 0.15:
        return name()
    if r < 0.22:
        return rng.choice(CONSTS)
    if r < 0.30:
        return rng.choice(NUMBERS)
    if r < 0.40:
        return rng.choice(STRINGS)
    if r < 0.48:
        items = [('...' if rng.random() < 0.1 else '') + expr(depth + 1) for _ in range(rng.randint(0, 3))]
        if rng.random() < 0.1:
            items.insert(rng.randint(0, len(items)), '')
        return '[' + ', '.join(items) + ']'
    if r < 0.56:
        items = []
        for _ in range(rng.randint(0, 3)):
            k = rng.random()
            if k < 0.25:
                items.append(name())
            elif k < 0.4:
                items.append(rng.choice(["'k'", '"k2"', '3', '1.5']) + ': ' + expr(depth + 1))
            elif k < 0.55:
                items.append('(' + expr(depth + 1) + '): ' + expr(depth + 1))
            elif k < 0.65:
                items.append('...' + expr(depth + 1))
            else:
                items.append(name() + ': ' + expr(depth + 1))
        return '{' + ', '.join(items) + '}'
    if r < 0.64:
        return '(' + expr(depth + 1) + ')'
    if r < 0.70:
        f = rng.choice(FUNCTIONS)
        return f + args(depth)
    if r < 0.75:
        params = rng.choice(['x', '(x)', '(x, y)', '(a, b)'])
        return params + ' => ' + expr(depth + 1)
    if r < 0.80:
        return rng.choice(UNARY) + ws() + primary(depth + 1)
    return name()


def postfix(e, depth):
    if depth > 4:
        return e
    for _ in range(rng.randint(0, 2)):
        r = rng.random()
        if r < 0.25:
            attr = rng.choice([name(), '0', '1', 'and', '(' + expr(depth + 1) + ')'])
            op = rng.choice(['.', '.', '?.'])
            e = e + op + attr
            if rng.random() < 0.3:
                e += args(depth)
        elif r < 0.45:
            e = e + '|' + rng.choice(FILTERS)
            if rng.random() < 0.5:
                e += args(depth)
        elif r < 0.6:
            k = rng.random()
            if k < 0.6:
                e = e + '[' + expr(depth + 1) + ']'
            else:
                a = expr(depth + 1) if rng.random() < 0.6 else ''
                b = expr(depth + 1) if rng.random() < 0.6 else ''
                e = e + '[' + a + ':' + b + ']'
        elif r < 0.62 and not VALID:
            e = e + args(depth)
        elif r < 0.8:
            t = rng.choice(TESTS)
            neg = rng.choice(['is', 'is not'])
            e = e + ' ' + neg + ' ' + t
            if VALID and t == 'defined':
                t = 'null'
                e = e[: e.rfind(' ')] + ' ' + t
            if t in ('same as', 'divisible by') and rng.random() < 0.7:
                e += ' ' + primary(depth + 1)
            elif rng.random() < 0.3:
                e += args(depth)
    return e


def expr(depth=0):
    e = postfix(primary(depth), depth)
    r = rng.random()
    if depth < 4 and r < 0.45:
        op = rng.choice(BINARY)
        e = e + ws() + op + ws() + expr(depth + 1)
    elif depth < 4 and r < 0.55:
        e = e + ' ? ' + expr(depth + 1)
        if rng.random() < 0.7:
            e += ' : ' + expr(depth + 1)
    elif depth < 3 and r < 0.6:
        lhs = rng.choice([name(), '[a, b]', '[a, , b]', '{a, b: c}'] if VALID else [name(), '[a, b]', '[a, , b]', '{a, b: c}', '1', 'a.b', '[]', '{x}'])
        e = lhs + ' = ' + expr(depth + 1)
    return e


def corrupt(s):
    r = rng.random()
    if r < 0.4:
        # drop a random character span
        if len(s) > 2:
            i = rng.randrange(len(s))
            j = min(len(s), i + rng.randint(1, 3))
            s = s[:i] + s[j:]
    elif r < 0.8:
        i = rng.randrange(len(s) + 1)
        s = s[:i] + ' ' + rng.choice(PUNCT_NOISE) + ' ' + s[i:]
    else:
        i = rng.randrange(len(s) + 1)
        s = s[:i] + rng.choice(['\n', '  ', ' 1 ', ' a ']) + s[i:]
    return s




def batch(count, seed, corruption, valid, prefix):
    global rng, CORRUPT, VALID
    rng = random.Random(seed)
    CORRUPT = corruption
    VALID = valid
    cases = []
    for n in range(count):
        parts = []
        for _ in range(rng.randint(1, 3)):
            e = expr()
            if rng.random() < CORRUPT:
                e = corrupt(e)
            prefix_text = rng.choice(['', '', 'text ', '\n', '{% line 3 %}', 'a\n\n'])
            parts.append(prefix_text + '{{' + ws() + e + ws() + '}}')
        cases.append({'id': f'{prefix}-{n}', 'source': ''.join(parts)})
    return cases


if len(sys.argv) > 1:
    cases = batch(int(sys.argv[1]), int(sys.argv[2]), float(sys.argv[3]), len(sys.argv) > 4 and sys.argv[4] == 'valid',
                  'random')
else:
    cases = batch(250, 101, 0.0, True, 'valid') + batch(600, 102, 0.3, False, 'mixed')
sys.stdout.reconfigure(encoding='utf-8', newline='\n')
print(json.dumps(cases, ensure_ascii=False, indent=0))

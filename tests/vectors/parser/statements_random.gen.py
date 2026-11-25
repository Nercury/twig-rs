"""Writes a batch of random templates with tags, valid and corrupted, for differential tests of the parser and the
CorrectnessNodeVisitor against the oracle (tests/vectors/parser/statements_random.gen.json and .json).

    python -I tests/vectors/parser/statements_random.gen.py [count] [seed] > tests/vectors/parser/statements_random.gen.json
    MSYS_NO_PATHCONV=1 docker run --rm -i -v "$PWD/tests/vectors/parser:/v" -e CASES_FILE=/v/statements_random.gen.json twig-oracle:3.31.1 php < tests/vectors/parser/expressions.gen.php > tests/vectors/parser/statements_random.json

The committed batch uses the defaults (count 1000, seed 31); larger batches can be run with the ignored
`external_vectors` test (TWIG_PARSER_VECTORS).
"""

import json
import random
import sys

NAMES = ['a', 'b', 'x', 'item', 'items', 'user', 'forms', 'macros', 'm', 'n', 'in', 'loop', 'true', 'none', '_self']
FILTERS = ['upper', 'lower', 'escape', 'e', 'raw', 'default', 'join', 'nope', 'spaceless', 'first']
FUNCTIONS = ['max', 'range', 'parent', 'block', 'include', 'cycle', 'nope', 'attribute', 'constant', 'm', 'n']
TESTS = ['defined', 'empty', 'even', 'same as', 'divisible by', 'null', 'nope']
TEMPLATES = ["'base'", "'b.twig'", '"layout"', "['a', 'b']", 'x', "'a' ~ b"]
BLOCK_NAMES = ['a', 'b', 'content', 'title', 'in', 'x']


class Gen:
    def __init__(self, rng):
        self.rng = rng

    def pick(self, items):
        return self.rng.choice(items)

    def chance(self, p):
        return self.rng.random() < p

    def ws(self):
        return self.pick([' ', ' ', ' ', '\n', '  ', ' \n '])

    def expr(self, depth=0):
        r = self.rng.random()
        if depth > 2 or r < 0.3:
            return self.pick(NAMES + ['1', '2.5', "'s'", '"t"', 'null', 'false', '[1, 2]', '{a: 1}'])
        if r < 0.4:
            return self.expr(depth + 1) + '|' + self.pick(FILTERS) + self.pick(['', '()', "('x')", '(1, 2)'])
        if r < 0.5:
            return self.pick(FUNCTIONS) + '(' + self.pick(['', self.expr(depth + 1), "'a'", 'a, b']) + ')'
        if r < 0.6:
            return self.pick(NAMES) + '.' + self.pick(['b', 'm', 'm()', '0', 'and', '(x)', '(x)()'])
        if r < 0.7:
            return self.expr(depth + 1) + ' ' + self.pick(['+', '~', '??', '?:', 'and', 'or', '==', 'in', '..']) + ' ' + self.expr(depth + 1)
        if r < 0.75:
            return self.expr(depth + 1) + ' is ' + self.pick(['', 'not ']) + self.pick(TESTS) + self.pick(['', ' 2', '(2)'])
        if r < 0.8:
            return self.expr(depth + 1) + ' ? ' + self.expr(depth + 1) + ' : ' + self.expr(depth + 1)
        if r < 0.85:
            return '(' + self.expr(depth + 1) + ')'
        if r < 0.9:
            return 'not ' + self.expr(depth + 1)
        return self.pick(NAMES) + ' = ' + self.expr(depth + 1)

    def open(self, inner):
        left = self.pick(['{%', '{%', '{%', '{%-', '{%~'])
        right = self.pick(['%}', '%}', '%}', '-%}', '~%}'])
        return left + ' ' + inner + ' ' + right

    def text(self):
        return self.pick(['x', ' ', '\n', '  \n  ', 'a b', '\n\n', '﻿', '\t'])

    def print_(self):
        return self.pick(['{{', '{{-', '{{~']) + ' ' + self.expr() + ' ' + self.pick(['}}', '-}}', '~}}'])

    def body(self, depth):
        n = self.rng.randint(0, 3 if depth < 3 else 1)
        return ''.join(self.item(depth + 1) for _ in range(n))

    def item(self, depth):
        r = self.rng.random()
        if r < 0.2:
            return self.text()
        if r < 0.35:
            return self.print_()
        if r < 0.38:
            return '{## ' + self.pick(['doc', 'Docs', '0', '']) + ' #}' + self.ws()
        if depth > 4:
            return self.text()
        return self.tag(depth)

    def tag(self, depth):
        kind = self.pick([
            'if', 'for', 'block', 'set', 'set_capture', 'apply', 'autoescape', 'with', 'macro', 'import', 'from',
            'embed', 'extends', 'use', 'include', 'do', 'flush', 'deprecated', 'types', 'guard', 'block_short',
        ])
        b = lambda: self.body(depth)
        if kind == 'if':
            s = self.open('if ' + self.expr()) + b()
            for _ in range(self.rng.randint(0, 2)):
                s += self.open('elseif ' + self.expr()) + b()
            if self.chance(0.4):
                s += self.open('else') + b()
            return s + self.open('endif')
        if kind == 'for':
            targets = self.pick(['x', 'k, v', 'a, b, c', 'in', 'true', 'x,'])
            s = self.open('for ' + targets + ' in ' + self.expr()) + b()
            if self.chance(0.3):
                s += self.open('else') + b()
            return s + self.open('endfor')
        if kind == 'block':
            name = self.pick(BLOCK_NAMES)
            end = self.pick(['endblock', 'endblock', 'endblock ' + name, 'endblock ' + self.pick(BLOCK_NAMES)])
            return self.open('block ' + name) + b() + self.open(end)
        if kind == 'block_short':
            return self.open('block ' + self.pick(BLOCK_NAMES) + ' ' + self.expr())
        if kind == 'set':
            names = self.pick(['a', 'a, b', 'in', 'none', 'a.b'])
            values = self.pick([self.expr(), self.expr() + ', ' + self.expr()])
            return self.open('set ' + names + ' = ' + values)
        if kind == 'set_capture':
            return self.open('set ' + self.pick(['a', 'a, b', 'c'])) + b() + self.open('endset')
        if kind == 'apply':
            return self.open('apply ' + self.pick(['upper', 'lower|escape', 'nope', 'upper(1)', '', 'upper x'])) + b() + self.open('endapply')
        if kind == 'autoescape':
            return self.open('autoescape' + self.pick(['', " 'js'", ' false', ' v', ' true'])) + b() + self.open('endautoescape')
        if kind == 'with':
            return self.open('with' + self.pick(['', ' {a: 1}', ' {a: 1} only', ' only', ' x'])) + b() + self.open('endwith')
        if kind == 'macro':
            name = self.pick(['m', 'n', 'input', 'and'])
            args = self.pick(['()', '(a)', '(a, b = 1)', '(a, ...rest)', '(...rest, a)', '(a = b)', '(a, a)', '(varargs)', '', '(a: 1)'])
            end = self.pick(['endmacro', 'endmacro', 'endmacro ' + name, 'endmacro z'])
            return self.open('macro ' + name + args) + b() + self.open(end)
        if kind == 'import':
            return self.open('import ' + self.pick(['_self', "'forms'"]) + self.pick([' as forms', ' as macros', ' as true', '', ' as m.n']))
        if kind == 'from':
            return self.open('from ' + self.pick(['_self', "'forms'"]) + self.pick([' import m', ' import m as n', ' import m, n', ' import', ' import m,', '']))
        if kind == 'embed':
            inner = ''.join(self.pick([
                self.open('block ' + self.pick(BLOCK_NAMES)) + self.body(depth + 1) + self.open('endblock'),
                self.text(), '\n', self.print_(),
            ]) for _ in range(self.rng.randint(0, 2)))
            return self.open('embed ' + self.pick(TEMPLATES) + self.pick(['', ' with {a: 1}', ' only', ' ignore missing'])) + inner + self.open('endembed')
        if kind == 'extends':
            return self.open('extends ' + self.pick(TEMPLATES + ['']))
        if kind == 'use':
            return self.open('use ' + self.pick(["'blocks'", 'x', "'b' with a as b", "'b' with a,"]))
        if kind == 'include':
            return self.open('include ' + self.pick(TEMPLATES) + self.pick(['', ' with {a: 1}', ' only', ' ignore missing', ' with {} ignore missing']))
        if kind == 'do':
            return self.open('do ' + self.pick([self.expr(), '[a, b] = [1, 2]', '{a, b: c} = x', '']))
        if kind == 'flush':
            return self.open('flush' + self.pick(['', '', ' x']))
        if kind == 'deprecated':
            return self.open('deprecated ' + self.pick(["'x'", "'x' package='p'", "'x' version='1' package='p'", "'x' foo='a'", '']))
        if kind == 'types':
            return self.open('types ' + self.pick(['', '{}', "{a: 'x'}", "a?: 'x', b: 'y'", "a: b", "{a: 'x' b: 'y'}"]))
        if kind == 'guard':
            s = self.open('guard ' + self.pick(['function max', 'function nope', 'filter upper', 'filter nope', 'test even', 'test divisible by', 'foo max'])) + b()
            if self.chance(0.4):
                s += self.open('else') + b()
            return s + self.open('endguard')
        raise AssertionError(kind)

    def corrupt(self, template):
        r = self.rng.random()
        if not template:
            return template
        i = self.rng.randrange(len(template))
        if r < 0.25:
            # drop a closing delimiter
            j = template.find('%}', i)
            if j >= 0:
                return template[:j] + template[j + 2:]
        if r < 0.5:
            # insert a stray token
            token = self.pick([' 1', " 'x'", ' ,', ' =', ' (', ' )', '{% endif %}', '{% else %}', '{% endblock %}', '{% end %}', ' x'])
            return template[:i] + token + template[i:]
        if r < 0.7:
            # cut the template
            return template[:i]
        if r < 0.85:
            # duplicate a part
            j = self.rng.randrange(i, len(template) + 1)
            return template[:j] + template[i:j] + template[j:]
        # swap an end tag
        for a, bb in [('endif', 'endfor'), ('endblock', 'endif'), ('endfor', 'endblock'), ('endmacro', 'endblock')]:
            if a in template:
                return template.replace(a, bb, 1)
        return template


def main():
    count = int(sys.argv[1]) if len(sys.argv) > 1 else 1000
    seed = int(sys.argv[2]) if len(sys.argv) > 2 else 31
    rng = random.Random(seed)
    gen = Gen(rng)
    cases = []
    for i in range(count):
        template = gen.body(0)
        if rng.random() < 0.2:
            template = gen.open('extends ' + gen.pick(TEMPLATES)) + template
        if rng.random() < 0.45:
            template = gen.corrupt(template)
        case = {'id': f'random-{seed}-{i}', 'source': template}
        if rng.random() < 0.05:
            case['extensions'] = ['sandbox']
            case['source'] = gen.open('sandbox') + gen.pick(["{% include 'a' %}", ' ', 'x', '{{ x }}', '']) + gen.open('endsandbox') + template
        cases.append(case)
    sys.stdout.reconfigure(newline='\n')
    json.dump(cases, sys.stdout, indent=1)
    sys.stdout.write('\n')


if __name__ == '__main__':
    main()

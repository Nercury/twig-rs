"""Writes the oracle batches tests/vectors/callables/misc/<topic>.gen.json of the misc callables (tests/callables_misc.rs:
numbers, dates, constants and enums, the inline core tests, dump, template_from_string, random) and the PHP-level
batch of the CoreExtension settings (settings.gen.json, run by the unit tests of src/extension/runtime/misc).

Template cases render in a plain Twig 3.31.1 environment (`new Environment(new ArrayLoader($templates), $config)`,
autoescape off unless a case says otherwise) and collect PHP diagnostics. Values that JSON cannot express are written
as tagged objects (`{"$date": ["DateTime", "2010-10-04 13:45", "Europe/Paris"]}`, see VALUE TAGS below); the
generator turns them into `data_php` for the oracle and tests/callables_misc.rs builds the same values in Rust.

Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):

    python -I tests/vectors/callables/misc/misc.gen.py [topic ...]            # writes <topic>.gen.json
    python -I tests/vectors/callables/misc/misc.gen.py --oracle [topic ...]   # and runs them: <topic>.json

`--oracle` pipes each batch through `docker run --rm -i twig-oracle:3.31.1 php runner.php --pretty`.

VALUE TAGS (one key starting with `$`):
    {"$date": [class, time, zone|null]}  new \\DateTime(time[, new \\DateTimeZone(zone)]) (class DateTime or
                                         DateTimeImmutable), created after the case's PHP default timezone is set
    {"$tz": zone}                        new \\DateTimeZone(zone)
    {"$interval": spec}                  new \\DateInterval(spec)
    {"$diff": [date, date]}              $a->diff($b) of two $date specs
    {"$float": "NAN"|"INF"|"-INF"|"-0"}  special floats
    {"$enum": [class, case]}             an enum case
    {"$object": class}                   new class() (TwigRsConsts, TwigRsPlain)
    {"$string": text}                    new TwigRsString(text) (Stringable)
    {"$markup": text}                    new \\Twig\\Markup(text, 'UTF-8')
    {"$arrayobject": value}              new \\ArrayObject(value)
    {"$bytes": hex}                      a binary string
"""

import json
import os
import subprocess
import sys

OUT = os.path.dirname(os.path.abspath(__file__))

PHP_CLASSES = r"""
if (!class_exists('TwigRsConsts', false)) {
    interface TwigRsIface { const IFACE = 'iface'; }
    class TwigRsConsts implements TwigRsIface {
        const FOO = 'foo';
        const NUM = 3;
        const ARR = [1, 2];
        private const PRIV = 'priv';
        protected const PROT = 'prot';
    }
    class TwigRsPlain { public $p = 'pp'; }
    class TwigRsString {
        public function __construct(private string $s) {}
        public function __toString(): string { return $this->s; }
    }
    enum TwigRsSuit { case Hearts; case Spades; }
    enum TwigRsStatus: string { case Active = 'active'; case Inactive = 'inactive'; }
    enum TwigRsEmpty {}
}
"""


def php_string(s):
    return "'" + s.replace("\\", "\\\\").replace("'", "\\'") + "'"


def php_value(v):
    """The PHP expression of a value spec."""
    if isinstance(v, dict) and len(v) == 1 and next(iter(v)).startswith("$"):
        tag, arg = next(iter(v.items()))
        if tag == "$date":
            cls, time, zone = arg
            tz = "" if zone is None else ", new \\DateTimeZone(%s)" % php_string(zone)
            return "new \\%s(%s%s)" % (cls, php_string(time), tz)
        if tag == "$tz":
            return "new \\DateTimeZone(%s)" % php_string(arg)
        if tag == "$interval":
            return "new \\DateInterval(%s)" % php_string(arg)
        if tag == "$diff":
            return "(%s)->diff(%s)" % (php_value({"$date": arg[0]}), php_value({"$date": arg[1]}))
        if tag == "$float":
            return {"NAN": "NAN", "INF": "INF", "-INF": "-INF", "-0": "-0.0"}[arg]
        if tag == "$enum":
            return "\\%s::%s" % (arg[0], arg[1])
        if tag == "$object":
            return "new \\%s()" % arg
        if tag == "$string":
            return "new \\TwigRsString(%s)" % php_string(arg)
        if tag == "$markup":
            return "new \\Twig\\Markup(%s, 'UTF-8')" % php_string(arg)
        if tag == "$arrayobject":
            return "new \\ArrayObject(%s)" % php_value(arg)
        if tag == "$bytes":
            return "hex2bin('%s')" % arg
        raise ValueError(tag)
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        return str(v) if v != -(2 ** 63) else "PHP_INT_MIN"
    if isinstance(v, float):
        return repr(v)
    if isinstance(v, str):
        return php_string(v)
    if isinstance(v, list):
        return "[" + ", ".join(php_value(x) for x in v) + "]"
    if isinstance(v, dict):
        return "[" + ", ".join("%s => %s" % (php_string(k), php_value(x)) for k, x in v.items()) + "]"
    raise ValueError(v)


def case(tpl, data=None, autoescape=False, strict=False, debug=False, extensions=(), timezone=None, globals=None,
         errors="collect", pending=None, templates=None, charset=None):
    cfg = {"autoescape": "html" if autoescape else False, "strict_variables": strict, "debug": debug}
    if charset:
        cfg["charset"] = charset
    c = {"templates": templates or {"index.twig": tpl}, "env": "plain", "config": cfg, "errors": errors}
    if extensions:
        c["extensions"] = list(extensions)
    if globals:
        c["globals"] = globals
    if timezone:
        c["timezone"] = timezone
    # the test classes only where a case uses them
    used = json.dumps([c["templates"], data])
    classes = PHP_CLASSES if any(m in used for m in ("TwigRs", '"$string"', '"$object"', '"$enum"')) else ""
    if data:
        c["data"] = data
        c["data_php"] = classes + "return [" + ", ".join(
            "%s => %s" % (php_string(k), php_value(v)) for k, v in data.items()) + "];"
    elif classes:
        c["data_php"] = classes + "return [];"
    if pending:
        c["pending"] = pending
    return c


# ---- numbers -----------------------------------------------------------------------------------------------------

def numbers():
    cases = []
    for tpl in [
        "{{ (-5)|abs }}", "{{ (-5.5)|abs }}", "{{ 5|abs }}", "{{ (-0.0)|abs }}", "{{ -5|abs }}",
        "{{ 1|abs(2) }}", "{{ 1|abs(num=2) }}", "{{ 1|abs(...[2]) }}", "{{ (-1)|abs(...{num: 2}) }}",
        "{{ (-2)|abs(...{nope: 2}) }}",
    ]:
        cases.append(case(tpl))
    for x in ["-3", " 5", "5 ", " 5 ", "1e3", "1.5", "-0", True, False, None, "abc", "5abc", "", "0x1A", [1],
              {"$object": "TwigRsConsts"}, {"$float": "INF"}, {"$float": "-INF"}, {"$float": "NAN"},
              {"$float": "-0"}, -(2 ** 63), 9007199254740993, {"$string": "5"}]:
        cases.append(case("{{ x|abs }}", {"x": x}))
    for tpl in [
        "{{ 42.55|round }}", "{{ 42.55|round(1, 'floor') }}", "{{ 42.55|round(1, 'ceil') }}", "{{ 2.5|round }}",
        "{{ (-2.5)|round }}", "{{ 1.955|round(2) }}", "{{ 5.045|round(2) }}", "{{ 5.055|round(2) }}",
        "{{ 1234.5|round(-2) }}", "{{ 0.285|round(2) }}", "{{ 1.45|round(1) }}", "{{ '3.7'|round }}",
        "{{ null|round }}", "{{ 'abc'|round }}", "{{ true|round }}", "{{ [1]|round }}", "{{ [] |round }}",
        "{{ (-3.21)|round(1, 'ceil') }}", "{{ (-3.21)|round(1, 'floor') }}", "{{ 3.21|round(-1, 'ceil') }}",
        "{{ 3.5|round(0, 'up') }}", "{{ 3.5|round(0, null) }}", "{{ 1.005|round(2, 'ceil') }}",
        "{{ 1.1|round(1, 'floor') }}", "{{ (-0.4)|round }}", "{{ 1e20|round }}", "{{ 5.5|round(method='floor') }}",
        "{{ 5.55|round(precision=1) }}", "{{ 5.5|round(1, 'common', 'extra') }}", "{{ 5.5|round('2') }}",
        "{{ 5.555|round(2.9) }}", "{{ 5.5|round('abc') }}", "{{ 5.5|round(null) }}", "{{ 5.5|round(1, 'ceil') }}",
        "{{ 5.5|round(null, 'ceil') }}", "{{ 5.5|round('abc', 'floor') }}", "{{ 5.5|round([1], 'floor') }}",
        "{{ 5.5|round(...{method: 'ceil'}) }}", "{{ 5.5|round(...{nope: 1}) }}", "{{ 123.456|round(-1, 'floor') }}",
        "{{ 0.1|round(20) }}", "{{ 1.23456789|round(25) }}", "{{ 1.5|round(2147483648) }}",
        "{{ 25|round(-1) }}", "{{ 35|round(-1) }}", "{{ 1e300|round(-300) }}",
    ]:
        cases.append(case(tpl))
    for x in [{"$object": "TwigRsConsts"}, {"$float": "NAN"}, {"$float": "INF"}, "1e3", " 2.5", {"$string": "2.5"},
              {"$markup": "2.5"}]:
        cases.append(case("{{ x|round }}", {"x": x}))
    for tpl in [
        "{{ 200.35|number_format }}", "{{ 9800.333|number_format(2, '.', ',') }}",
        "{{ 1234567.891|number_format(2, ',', '.') }}", "{{ 0.5|number_format }}", "{{ 1.5|number_format }}",
        "{{ 2.5|number_format }}", "{{ (-0.4)|number_format }}", "{{ (-1234.567)|number_format(1) }}",
        "{{ 1234|number_format(2, 'dec', 'thou') }}", "{{ 1234.5|number_format(-1) }}",
        "{{ 1234.5|number_format(2, '', '') }}", "{{ 1e20|number_format }}", "{{ 1e15|number_format(2) }}",
        "{{ 1.005|number_format(2) }}", "{{ 'abc'|number_format }}", "{{ '12abc'|number_format }}",
        "{{ '1e3'|number_format }}", "{{ []|number_format }}", "{{ [1]|number_format }}", "{{ null|number_format }}",
        "{{ true|number_format(1) }}", "{{ 1234.5678|number_format('2') }}", "{{ 1234.5678|number_format(2.7) }}",
        "{{ 1234.5678|number_format('x') }}", "{{ 1234.5678|number_format(2, null, null) }}",
        "{{ 1234.5678|number_format(2, [1]) }}", "{{ 1234.5678|number_format(2, '.', [1]) }}",
        "{{ -9800.333|number_format(2) }}", "{{ 9007199254740993|number_format }}",
        "{{ 9007199254740993|number_format(2) }}", "{{ 4503599627370497.0|number_format(3) }}",
        "{{ 1234.5|number_format(decimal_point=',') }}", "{{ 1234.5|number_format(thousand_sep=' ') }}",
        "{{ 1234.5|number_format(decimal=2) }}", "{{ 1234.5|number_format(1, '·', '‘') }}",
        "{{ 0.000001|number_format(8) }}", "{{ 123456789.12345678|number_format(20) }}",
        "{{ 1234.5|number_format(...{decimal: 1}) }}", "{{ 1234.5|number_format(...{nope: 1}) }}",
        "{{ 1234.5|number_format(1, ',', '.', 'extra') }}", "{{ 1e80|number_format }}",
        "{{ 0.5|number_format(0, ',') }}", "{{ 999.9999|number_format(3) }}",
    ]:
        cases.append(case(tpl))
    for x in [{"$object": "TwigRsConsts"}, {"$float": "NAN"}, {"$float": "INF"}, {"$float": "-INF"},
              {"$float": "-0"}, -(2 ** 63), 9223372036854775807, {"$string": "12.5"}, {"$markup": "1234.5"}]:
        cases.append(case("{{ x|number_format(1) }}", {"x": x}))
    return cases


# ---- dates -------------------------------------------------------------------------------------------------------

FULL = "Y-m-d H:i:s T e P U"

DATES = {
    "ts": 1286199900,
    "neg_ts": -189291360,
    "zero": 0,
    "ts_string": "1286199900",
    "neg_ts_string": "-189291360",
    "plain": "2010-10-04 13:45",
    "offset": "2010-01-28T15:00:00+04:00",
    "abbr": "2010-01-28 15:00 EST",
    "dt": {"$date": ["DateTime", "2010-10-04 13:45", None]},
    "dt_paris": {"$date": ["DateTime", "2010-10-04 13:45", "Europe/Paris"]},
    "dt_ny": {"$date": ["DateTime", "2010-10-04 13:45", "America/New_York"]},
    "dt_offset": {"$date": ["DateTime", "2010-10-04 13:45:30.123456+05:30", None]},
    "dti": {"$date": ["DateTimeImmutable", "2010-10-04 13:45", "Asia/Tokyo"]},
    "stringable": {"$string": "2010-10-04 13:45"},
    "stringable_ts": {"$string": "1286199900"},
}


def dates():
    cases = []
    for tz in [None, "Europe/Paris", "America/Los_Angeles"]:
        for name, value in DATES.items():
            for tpl in ["{{ d|date }}", "{{ d|date('%s') }}" % FULL]:
                cases.append(case(tpl, {"d": value}, timezone=tz))
    for name in ["dt", "dt_paris", "dt_ny", "ts", "plain", "offset", "dti"]:
        for zone in ["'Asia/Hong_Kong'", "false", "'America/Chicago'", "'+05:00'", "'EST'", "tz", "'UTC'"]:
            cases.append(case("{{ d|date('%s', %s) }}" % (FULL, zone), {"d": DATES[name], "tz": {"$tz": "Indian/Mauritius"}},
                              timezone="Europe/Paris"))
    for tpl, data in [
        ("{{ d|date('d/m/Y') }}", {"d": DATES["dt"]}),
        ("{{ d|date('\\\\Y-\\\\m Y') }}", {"d": DATES["dt"]}),
        ("{{ d|date('D, d M Y H:i:s O') }}", {"d": DATES["dt_ny"]}),
        ("{{ d|date('l jS \\\\o\\\\f F Y h:i:s A') }}", {"d": DATES["dt"]}),
        ("{{ d|date('N w z t L o W B g G u v I Z c r') }}", {"d": DATES["dt_offset"]}),
        ("{{ d|date }}", {"d": {"$interval": "P2D"}}),
        ("{{ d|date }}", {"d": {"$interval": "P1Y2M3DT4H5M6S"}}),
        ("{{ d|date('%y-%m-%d %h:%i:%s %R %a %%') }}", {"d": {"$diff": [["DateTime", "2024-01-01", None],
                                                                          ["DateTime", "2024-03-05 10:20:30", None]]}}),
        ("{{ d|date('%Y %M %D %H %I %S %F %f %r %x') }}", {"d": {"$interval": "PT36H"}}),
        ("{{ d|date('%d', 'Asia/Tokyo') }}", {"d": {"$interval": "P3D"}}),
        ("{{ d|date }}", {"d": {"$diff": [["DateTime", "2024-03-05", None], ["DateTime", "2024-01-01", None]]}}),
        ("{{ d|date(5) }}", {"d": DATES["dt"]}),
        ("{{ d|date(null, 'Asia/Tokyo') }}", {"d": DATES["dt"]}),
        ("{{ d|date(format='Y') }}", {"d": DATES["dt"]}),
        ("{{ d|date(timezone='Asia/Tokyo') }}", {"d": DATES["dt"]}),
        ("{{ d|date('Y', 'Nowhere/City') }}", {"d": DATES["dt"]}),
        ("{{ d|date('Y', '') }}", {"d": DATES["dt"]}),
        ("{{ d|date('Y', ['x']) }}", {"d": DATES["dt"]}),
        ("{{ d|date('Y', 5) }}", {"d": DATES["dt"]}),
        ("{{ d|date('Y', s) }}", {"d": DATES["dt"], "s": {"$string": "Asia/Tokyo"}}),
        ("{{ d|date('Y', o) }}", {"d": DATES["dt"], "o": {"$object": "TwigRsPlain"}}),
        ("{{ 'notadate'|date }}", None),
        ("{{ '2010-13-45'|date }}", None),
        ("{{ [1]|date('Y') }}", None),
        ("{{ o|date('Y') }}", {"o": {"$object": "TwigRsPlain"}}),
        ("{{ true|date('Y-m-d H:i:s e') }}", None),
        ("{{ 1.5e3|date('Y-m-d') }}", None),
        ("{{ '1700000000.5'|date('Y-m-d H:i') }}", None),
        ("{{ false|date('e') }}", None),
        ("{{ ''|date('e') }}", None),
        ("{{ null|date('e') }}", None),
        ("{{ 'now'|date('e') }}", None),
        ("{{ 'now'|date('e', 'Asia/Tokyo') }}", None),
        ("{{ 'NOW'|date('e', false) }}", None),
        ("{{ '-1'|date('c') }}", None),
        ("{{ '-'|date('Y') }}", None),
        ("{{ '0012'|date('c') }}", None),
        ("{{ '+5'|date('e', false) }}", None),
        ("{{ ' 5'|date('Y') }}", None),
        ("{{ 9223372036854775807|date('Y') }}", None),
        ("{{ '@1286199900'|date('c e') }}", None),
        ("{{ '-0'|date('c') }}", None),
        ("{{ '99999999999999999999'|date('Y') }}", None),
        ("{{ '1.5'|date('H:i') }}", None),
        ("{{ 1286199900.0|date('c') }}", None),
        ("{{ d|date('c e', false) }}", {"d": "2010-10-04 13:45 +0200"}),
        ("{{ d|date('c T e', false) }}", {"d": 1286199900}),
        ("{{ m|date('Y-m-d') }}", {"m": {"$markup": "2010-10-04"}}),
        ("{{ d|date('Y', 'Europe/Paris', 'x') }}", {"d": DATES["dt"]}),
        ("{{ d|date(...{format: 'Y'}) }}", {"d": DATES["dt"]}),
        ("{{ d|date(...{nope: 'Y'}) }}", {"d": DATES["dt"]}),
    ]:
        cases.append(case(tpl, data, timezone="Europe/Paris"))
    for tpl, data in [
        ("{{ '2024-01-31'|date_modify('+1 month')|date('Y-m-d') }}", None),
        ("{{ d|date_modify('+1 day')|date('Y-m-d H:i T') }} {{ d|date('Y-m-d H:i T') }}", {"d": DATES["dt_ny"]}),
        ("{{ d|date_modify('+1 day')|date('Y-m-d H:i T') }} {{ d|date('Y-m-d H:i T') }}", {"d": DATES["dti"]}),
        ("{{ (d|date_modify('+1 day')) is same as(d) }}", {"d": DATES["dti"]}),
        ("{{ d|date_modify('blah') }}", {"d": DATES["dt"]}),
        ("{{ d|date_modify('blah') }}", {"d": DATES["dti"]}),
        ("{{ 1286199900|date_modify('+1 hour')|date('c e') }}", None),
        ("{{ '2010-01-28T15:00:00+04:00'|date_modify('+1 hour')|date('c e') }}", None),
        ("{{ '2010-01-28 15:00'|date_modify('midnight')|date('c e') }}", None),
        ("{{ d|date_modify(null)|date('c') }}", {"d": DATES["dt"]}),
        ("{{ d|date_modify('')|date('c') }}", {"d": DATES["dt"]}),
        ("{{ d|date_modify([1])|date('c') }}", {"d": DATES["dt"]}),
        ("{{ d|date_modify(5)|date('c') }}", {"d": DATES["dt"]}),
        ("{{ d|date_modify('@0')|date('c e') }}", {"d": DATES["dt_paris"]}),
        ("{{ d|date_modify('last day of next month')|date('c') }}", {"d": DATES["dt"]}),
        ("{{ d|date_modify(modifier='+2 days')|date('c') }}", {"d": DATES["dt"]}),
        ("{{ 'notadate'|date_modify('+1 day') }}", None),
        ("{{ i|date_modify('+1 day') }}", {"i": {"$interval": "P1D"}}),
        ("{{ d|date_modify('+1 day', 'x')|date('c') }}", {"d": DATES["dt"]}),
        ("{{ d|date_modify(...{modifier: '+3 days'})|date('c') }}", {"d": DATES["dt"]}),
        ("{{ d|date_modify(...[]) }}", {"d": DATES["dt"]}),
    ]:
        cases.append(case(tpl, data, timezone="Europe/Paris"))
    for tpl, data in [
        ("{{ date('2010-10-04 13:45')|date('c e') }}", None),
        ("{{ date(1286199900)|date('c e') }}", None),
        ("{{ date('2010-10-04', 'Asia/Tokyo')|date('c e', false) }}", None),
        ("{{ date(d)|date('c e', false) }}", {"d": DATES["dt_ny"]}),
        ("{{ date(d) == d ? 'eq' : 'ne' }} {{ date(d) is same as(d) ? 'same' : 'clone' }}", {"d": DATES["dt_ny"]}),
        ("{{ date(d) is same as(d) ? 'same' : 'other' }}", {"d": DATES["dti"]}),
        ("{{ date(d, false) is same as(d) ? 'same' : 'other' }}", {"d": DATES["dti"]}),
        ("{{ date() > date('2000-01-01') ? 'later' : 'earlier' }}", None),
        ("{{ date('-2days') < date() ? 'yes' : 'no' }}", None),
        ("{{ date('notadate') }}", None),
        ("{{ date(null, 'Asia/Tokyo')|date('e', false) }}", None),
        ("{{ date(timezone='Asia/Tokyo')|date('e', false) }}", None),
        ("{{ date()|date('e', false) }}", None),
        ("{{ date('now', false)|date('e', false) }}", None),
        ("{{ date().timezone }}", None),
        ("{{ date('2024-01-01 12:00:00').timestamp }}", None),
        ("{{ date('2024-01-01 12:00:00').date }} {{ date('2024-01-01 12:00:00').timezone_type }}", None),
        ("{{ date('2024-01-01', 'nope') }}", None),
        ("{{ date(date('2024-01-01 00:00', 'UTC'), 'Asia/Tokyo')|date('c e', false) }}", None),
        ("{{ date(d, tz)|date('c e', false) }}", {"d": DATES["dt"], "tz": {"$tz": "Australia/Sydney"}}),
        ("{{ date([1]) }}", None),
        ("{{ date(i) }}", {"i": {"$interval": "P1D"}}),
        ("{{ date('2024-01-01', 'Asia/Tokyo', 'x')|date('c', false) }}", None),
        ("{{ date(...['2024-02-03'])|date('Y-m-d') }}", None),
        ("{{ date(...{nope: 1}) }}", None),
        ("{% set d = date('2024-01-01') %}{{ d|date_modify('+1 day')|date('Y-m-d') }} {{ d|date('Y-m-d') }}", None),
    ]:
        cases.append(case(tpl, data, timezone="Europe/Paris"))
    return cases


# ---- constants and enums -----------------------------------------------------------------------------------------

def constants():
    cases = []
    for tpl in [
        "{{ constant('PHP_INT_MAX') }}", "{{ constant('PHP_INT_MIN') }}", "{{ constant('PHP_INT_SIZE') }}",
        "{{ constant('M_PI') }}", "[{{ constant('PHP_EOL') }}]", "{{ constant('E_ALL') }}",
        "{{ constant('E_STRICT') }}", "{{ constant('DATE_ATOM') }}", "{{ constant('DATE_RFC7231') }}",
        "{{ constant('\\\\PHP_INT_SIZE') }}", "{{ constant('TRUE') }}", "{{ constant('true') }}",
        "{{ constant('NULL') is null ? 'null' }}", "{{ constant('PHP_VERSION') }}", "{{ constant('PHP_OS_FAMILY') }}",
        "{{ constant('JSON_PRETTY_PRINT') }}", "{{ constant('ENT_QUOTES') }}", "{{ constant('PHP_FLOAT_EPSILON') }}",
        "{{ constant('PHP_FLOAT_MAX') }}", "{{ constant('INF') }}", "{{ constant('NAN') }}",
        "{{ constant('PHP_ROUND_HALF_EVEN') }}", "{{ constant('SORT_STRING') }}", "{{ constant('MB_CASE_TITLE') }}",
        "{{ constant('PREG_SPLIT_NO_EMPTY') }}", "{{ constant('MT_RAND_PHP') }}", "{{ constant('SUNFUNCS_RET_STRING') }}",
        "{{ constant('NOPE') }}", "{{ constant('php_int_max') }}", "{{ constant('') }}",
        "{{ constant('TwigRsConsts::FOO') }}", "{{ constant('twigrsconsts::FOO') }}", "{{ constant('\\\\TwigRsConsts::NUM') }}",
        "{{ constant('TwigRsConsts::ARR')|join(',') }}", "{{ constant('TwigRsConsts::IFACE') }}",
        "{{ constant('TwigRsIface::IFACE') }}", "{{ constant('TwigRsConsts::NOPE') }}", "{{ constant('NoClass::FOO') }}",
        "{{ constant('TwigRsConsts::PRIV') }}", "{{ constant('TwigRsConsts::PROT') }}",
        "{{ constant('TwigRsConsts::class') }}", "{{ constant('TwigRsConsts::CLASS') }}", "{{ constant('Nope::class') }}",
        "{{ constant('::class') }}", "{{ constant('::FOO') }}", "{{ constant('TwigRsConsts::foo') }}",
        "{{ constant('TwigRsSuit::Hearts').name }}", "{{ constant('TwigRsStatus::Active').value }}",
        "{{ constant('TwigRsSuit::Nope') }}", "{{ constant('DateTimeInterface::ATOM') }}",
        "{{ constant('DateTime::RFC7231') }}", "{{ constant('DateTimeImmutable::COOKIE') }}",
        "{{ constant('DateTimeZone::EUROPE') }}", "{{ constant('ArrayObject::ARRAY_AS_PROPS') }}",
        "{{ constant('Twig\\\\Environment::VERSION') }}", "{{ constant('Twig\\\\Environment::VERSION_ID') }}",
        "{{ constant('Twig\\\\Environment::MAJOR_VERSION') }}.{{ constant('Twig\\\\Environment::MINOR_VERSION') }}",
        "{{ constant('Twig\\\\Template::ANY_CALL') }}", "{{ constant('RoundingMode::HalfEven').name }}",
        "{{ constant('Twig\\\\Extension\\\\CoreExtension::ARRAY_LIKE_CLASSES')|join(',') }}",
        "{{ constant('PHP_EOL') is defined ? 'y' : 'n' }}", "{{ constant('NOPE') is defined ? 'y' : 'n' }}",
        "{{ constant('E_STRICT') is defined ? 'y' : 'n' }}", "{{ constant('DATE_RFC7231') is defined ? 'y' : 'n' }}",
        "{{ constant('TwigRsConsts::FOO') is defined ? 'y' : 'n' }}",
        "{{ constant('TwigRsConsts::PRIV') is defined ? 'y' : 'n' }}",
        "{{ constant('TwigRsSuit::Spades') is defined ? 'y' : 'n' }}",
        "{{ constant('NoClass::FOO') is defined ? 'y' : 'n' }}",
        "{{ constant('true') is defined ? 'y' : 'n' }}", "{{ constant('TwigRsConsts::class') is defined ? 'y' : 'n' }}",
        "{{ constant('class', 'str') is defined ? 'y' : 'n' }}", "{{ constant('FOO', 'str') is defined ? 'y' : 'n' }}",
        "[{{ constant('PHP_EOL', null) }}]", "{{ constant('FOO', 'str') }}", "{{ constant('FOO', 5) }}",
        "{{ constant('FOO', true) }}", "{{ constant('FOO', [1]) }}", "{{ constant('class', 'str') }}",
        "{{ constant('PHP_INT_SIZE', check_defined=true) ? 'y' : 'n' }}",
        "{{ constant('PHP_INT_SIZE', check_defined='') ? 'y' : 'n' }}",
        "{{ constant('PHP_INT_SIZE', null, false) }}", "{{ constant('PHP_INT_SIZE', null, [1]) }}",
        "{{ constant(...['PHP_INT_SIZE']) }}", "{{ constant(...{constant: 'PHP_INT_SIZE'}) }}",
        "{{ constant(...{nope: 'PHP_INT_SIZE'}) }}", "{{ constant(...[]) }}",
        "{{ constant('PHP_INT_SIZE', null, false, 'extra') }}",
    ]:
        cases.append(case(tpl))
    obj = {"$object": "TwigRsConsts"}
    for tpl, data in [
        ("{{ constant('FOO', o) }}", {"o": obj}),
        ("{{ constant('IFACE', o) }}", {"o": obj}),
        ("{{ constant('class', o) }}", {"o": obj}),
        ("{{ constant('NOPE', o) }}", {"o": obj}),
        ("{{ constant('PRIV', o) }}", {"o": obj}),
        ("{{ constant('class', o) is defined ? 'y' : 'n' }}", {"o": obj}),
        ("{{ constant('FOO', o) is defined ? 'y' : 'n' }}", {"o": obj}),
        ("{{ constant('NOPE', o) is defined ? 'y' : 'n' }}", {"o": obj}),
        ("{{ constant('Hearts', e).name }}", {"e": {"$enum": ["TwigRsSuit", "Spades"]}}),
        ("{{ constant('class', e) }}", {"e": {"$enum": ["TwigRsSuit", "Spades"]}}),
        ("{{ constant('ATOM', d) }}", {"d": {"$date": ["DateTime", "2024-01-01", "UTC"]}}),
        ("{{ constant('RFC7231', d) }}", {"d": {"$date": ["DateTimeImmutable", "2024-01-01", "UTC"]}}),
        ("{{ constant(c) }}", {"c": ["x"]}),
        ("{{ constant(c) }}", {"c": None}),
        ("{{ constant(c) }}", {"c": 5}),
        ("{{ constant(c) }}", {"c": 1.5}),
        ("{{ constant(c) }}", {"c": True}),
        ("{{ constant(c) }}", {"c": {"$string": "PHP_INT_SIZE"}}),
        ("{{ constant(c) }}", {"c": {"$string": "TwigRsConsts::class"}}),
        ("{{ constant(c) }}", {"c": {"$object": "TwigRsPlain"}}),
        ("{{ constant(c, o) }}", {"c": None, "o": obj}),
        ("{{ constant(c, o) }}", {"c": ["x"], "o": obj}),
        ("{{ constant(c) is defined ? 'y' : 'n' }}", {"c": None}),
        ("{{ constant(c) is defined ? 'y' : 'n' }}", {"c": ["x"]}),
    ]:
        cases.append(case(tpl, data))
    for e in ["TwigRsSuit", "twigrssuit", "\\TwigRsStatus", "TwigRsEmpty", "TwigRsConsts", "TwigRsIface", "Nope", "",
              "RoundingMode", "ArrayObject", "DateTime", 5, 1.5, True, None, ["x"], {"$string": "TwigRsSuit"},
              {"$object": "TwigRsPlain"}]:
        cases.append(case("{{ enum(e).name }}", {"e": e}))
        cases.append(case("{{ enum_cases(e)|map(c => c.name)|join(',') }}", {"e": e}))
    for tpl, data in [
        ("{{ enum(e).value }}", {"e": "TwigRsStatus"}),
        ("{{ enum(e) is same as(constant('TwigRsSuit::Hearts')) ? 'same' : 'other' }}", {"e": "TwigRsSuit"}),
        ("{{ enum_cases(e)|map(c => c.value)|join(',') }}", {"e": "TwigRsStatus"}),
        ("{{ enum_cases(e)|length }}", {"e": "TwigRsEmpty"}),
        ("{{ enum(enum=e).name }}", {"e": "TwigRsSuit"}),
        ("{{ enum_cases(enum=e)|length }}", {"e": "TwigRsSuit"}),
        ("{{ enum(e, 'extra').name }}", {"e": "TwigRsSuit"}),
        ("{{ enum(...{enum: e}).name }}", {"e": "TwigRsSuit"}),
        ("{{ enum(...{nope: e}).name }}", {"e": "TwigRsSuit"}),
        ("{{ enum(...[]) }}", None),
        ("{{ enum(e) }}", {"e": "TwigRsSuit"}),
        ("{{ enum('TwigRsSuit').name }}", None),
        ("{{ enum_cases('TwigRsStatus')|map(c => c.value)|join(',') }}", None),
        ("{{ enum('TwigRsSuit').cases()|length }}", None),
        ("{{ enum('TwigRsStatus').from('inactive').name }}", None),
        ("{{ enum('Nope') }}", None),
        ("{{ enum_cases('Nope') }}", None),
        ("{{ enum(5) }}", None),
        ("{{ enum('TwigRsEmpty') }}", None),
    ]:
        cases.append(case(tpl, data))
    return cases


# ---- inline core tests -------------------------------------------------------------------------------------------

def tests():
    cases = []
    for tpl in [
        "{{ -3 is odd ? 'y' : 'n' }}", "{{ '4' is even ? 'y' : 'n' }}", "{{ 2.5 is even ? 'y' : 'n' }}",
        "{{ null is even ? 'y' : 'n' }}", "{{ 0 is odd ? 'y' : 'n' }}", "{{ 'abc' is even ? 'y' : 'n' }}",
        "{{ [1] is even ? 'y' : 'n' }}", "{{ '5abc' is odd ? 'y' : 'n' }}", "{{ true is odd ? 'y' : 'n' }}",
        "{{ 1.5e3 is even ? 'y' : 'n' }}", "{{ 2 is even(2) ? 'y' : 'n' }}", "{{ 3 is not odd ? 'y' : 'n' }}",
        "{{ '' is even ? 'y' : 'n' }}", "{{ ' 3' is odd ? 'y' : 'n' }}", "{{ 9223372036854775807 is odd ? 'y' : 'n' }}",
        "{{ 1e100 is even ? 'y' : 'n' }}",
        "{{ 9 is divisible by(3) ? 'y' : 'n' }}", "{{ 9 is divisible by 3 ? 'y' : 'n' }}",
        "{{ 10 is divisible by(3) ? 'y' : 'n' }}", "{{ 9.5 is divisible by(3) ? 'y' : 'n' }}",
        "{{ '9' is divisible by(3) ? 'y' : 'n' }}", "{{ 'abc' is divisible by(3) ? 'y' : 'n' }}",
        "{{ 9 is divisible by(0) ? 'y' : 'n' }}", "{{ 9 is not divisible by(2) ? 'y' : 'n' }}",
        "{{ 9 is divisible by(1.5) ? 'y' : 'n' }}", "{{ 9 is divisible by('x') ? 'y' : 'n' }}",
        "{{ -9 is divisible by(-3) ? 'y' : 'n' }}", "{{ 9 is divisible by(num=3) ? 'y' : 'n' }}",
        "{{ 1 is same as('1') ? 'y' : 'n' }}", "{{ [1] is same as([1]) ? 'y' : 'n' }}",
        "{{ 1.0 is same as(1) ? 'y' : 'n' }}", "{{ 1 is same as 1 ? 'y' : 'n' }}", "{{ null is same as(null) ? 'y' : 'n' }}",
        "{{ 'a' is same as('a') ? 'y' : 'n' }}", "{{ {a: 1, b: 2} is same as({b: 2, a: 1}) ? 'y' : 'n' }}",
        "{{ 1 is not same as(true) ? 'y' : 'n' }}", "{{ 1 is same as(value=1) ? 'y' : 'n' }}",
        "{{ null is null ? 'y' : 'n' }}", "{{ null is none ? 'y' : 'n' }}", "{{ undef is null ? 'y' : 'n' }}",
        "{{ 0 is null ? 'y' : 'n' }}", "{{ '' is not null ? 'y' : 'n' }}", "{{ false is none ? 'y' : 'n' }}",
        "[{{ 1 is true }}]", "[{{ 0 is true }}]", "[{{ 'a' is true }}]", "[{{ null is true }}]",
        "[{{ 0 is not true }}]", "[{{ [] is true ? 'y' : 'n' }}]", "[{{ '0' is true ? 'y' : 'n' }}]",
        "{{ n is defined ? 'y' : 'n' }}", "{{ undef is defined ? 'y' : 'n' }}", "{{ 'a' is defined ? 'y' : 'n' }}",
        "{{ range(1, 2) is defined ? 'y' : 'n' }}", "{{ range(1, 0)|length }}", "{{ a.b is defined ? 'y' : 'n' }}",
        "{{ a.c is defined ? 'y' : 'n' }}", "{{ a.b.c is defined ? 'y' : 'n' }}", "{{ [1] is defined ? 'y' : 'n' }}",
        "{{ (1 + 1) is defined }}", "{{ a|upper is defined }}", "{{ _self is defined ? 'y' : 'n' }}",
        "{{ random(0) is defined ? 'y' : 'n' }}",
    ]:
        cases.append(case(tpl, {"n": None, "a": {"b": 1}}))
    for tpl in ["{{ undef is null ? 'y' : 'n' }}", "{{ undef is defined ? 'y' : 'n' }}",
                "{{ undef.x is defined ? 'y' : 'n' }}", "{{ undef is even ? 'y' : 'n' }}"]:
        cases.append(case(tpl, strict=True))
    for tpl, data in [
        ("[{{ m is true }}]", {"m": {"$markup": ""}}),
        ("[{{ m is true }}]", {"m": {"$markup": "x"}}),
        ("[{{ m is true ? 'y' : 'n' }}]", {"m": {"$markup": "0"}}),
        ("{{ o is same as(o) ? 'y' : 'n' }}", {"o": {"$object": "TwigRsPlain"}}),
        ("{{ o is same as(p) ? 'y' : 'n' }}", {"o": {"$object": "TwigRsPlain"}, "p": {"$object": "TwigRsPlain"}}),
        ("{{ e is same as(constant('TwigRsSuit::Hearts')) ? 'y' : 'n' }}", {"e": {"$enum": ["TwigRsSuit", "Hearts"]}}),
        ("{{ o is even ? 'y' : 'n' }}", {"o": {"$object": "TwigRsPlain"}}),
        ("{{ x is odd ? 'y' : 'n' }}", {"x": {"$float": "INF"}}),
        ("{{ x is even ? 'y' : 'n' }}", {"x": {"$float": "NAN"}}),
        ("{{ x is divisible by(2) ? 'y' : 'n' }}", {"x": {"$float": "INF"}}),
        ("{{ 'foo' is constant('TwigRsConsts::FOO') ? 'y' : 'n' }}", None),
        ("{{ 'foo' is constant('FOO', o) ? 'y' : 'n' }}", {"o": {"$object": "TwigRsConsts"}}),
        ("{{ '3' is constant('TwigRsConsts::NUM') ? 'y' : 'n' }}", None),
        ("{{ 3 is constant('TwigRsConsts::NUM') ? 'y' : 'n' }}", None),
        ("{{ x is constant('DATE_ATOM') ? 'y' : 'n' }}", {"x": "Y-m-d\\TH:i:sP"}),
        ("{{ x is constant('DATE_RFC7231') ? 'y' : 'n' }}", {"x": "x"}),
        ("{{ x is constant('NOPE') ? 'y' : 'n' }}", {"x": 1}),
        ("{{ x is constant('TwigRsConsts::NOPE') ? 'y' : 'n' }}", {"x": 1}),
        ("{{ x is constant('Nope::X') ? 'y' : 'n' }}", {"x": 1}),
        ("{{ x is constant('FOO', 'str') ? 'y' : 'n' }}", {"x": 1}),
        ("{{ x is constant('FOO', null) ? 'y' : 'n' }}", {"x": 1}),
        ("{{ e is constant('TwigRsSuit::Hearts') ? 'y' : 'n' }}", {"e": {"$enum": ["TwigRsSuit", "Hearts"]}}),
        ("{{ e is constant('Spades', e) ? 'y' : 'n' }}", {"e": {"$enum": ["TwigRsSuit", "Hearts"]}}),
        ("{{ x is constant('DateTime::RFC7231') ? 'y' : 'n' }}", {"x": "x"}),
        ("{{ x is constant('TwigRsConsts::PRIV') ? 'y' : 'n' }}", {"x": "priv"}),
        ("{{ x is constant('PHP_EOL') ? 'y' : 'n' }}", {"x": "\n"}),
        ("{{ x is constant('E_STRICT') ? 'y' : 'n' }}", {"x": 2048}),
        ("{{ x is constant(name='PHP_EOL') ? 'y' : 'n' }}", {"x": "\n"}),
    ]:
        cases.append(case(tpl, data))
    return cases


# ---- dump --------------------------------------------------------------------------------------------------------

def debug():
    cases = []
    for debug_on in [False, True]:
        for tpl, data in [
            ("{{ dump(1) }}", None),
            ("{{ dump(1, 's', 1.5, true, null, [1, {a: 'b'}]) }}", None),
            ("{{ dump() }}", {"a": 1, "b": [1, "x"], "c": {"d": None}}),
            ("{{ dump(value=1) }}", None),
            ("{{ dump(foo=1) }}", None),
            ("{{ dump(1, foo=2) }}", None),
            ("{{ dump(1, value=2) }}", None),
            ("{{ dump(fooBar=1) }}", None),
            ("{{ dump(value=1, other=2) }}", None),
        ]:
            cases.append(case(tpl, data, debug=debug_on, extensions=["debug"]))
    for tpl, data in [
        ("{{ dump('<b>') }}", None),
        ("{{ dump(0.1 + 0.2, 1e100, -0.0, 1e15, 0.00001, 100.0) }}", None),
        ("{{ dump(x) }}", {"x": {"$float": "NAN"}}),
        ("{{ dump(x) }}", {"x": {"$float": "-INF"}}),
        ("{{ dump(e, s) }}", {"e": {"$enum": ["TwigRsSuit", "Hearts"]}, "s": {"$enum": ["TwigRsStatus", "Active"]}}),
        ("{{ dump(o) }}", {"o": {"$object": "TwigRsPlain"}}),
        ("{{ dump(o) }}", {"o": {"$arrayobject": [1, 2]}}),
        ("{{ dump(d) }}", {"d": {"$date": ["DateTime", "2010-10-04 13:45:30", "Europe/Paris"]}}),
        ("{{ dump(z) }}", {"z": {"$tz": "Europe/Paris"}}),
        ("{% set x = 1 %}{{ dump() }}", {"a": "ctx"}),
        ("{% for i in [1] %}{{ dump() }}{% endfor %}", None),
        ("{{ dump({'k': [1, [2, [3]]]}) }}", None),
        ("{{ dump(b) }}", {"b": {"$bytes": "c3a9ff00"}}),
        ("{{ dump(m) }}", {"m": {"$markup": "<i>x</i>"}}),
        ("{{ dump(...[1, 2]) }}", None),
        ("{{ dump(...{value: 3}) }}", None),
        ("{% set t = template_from_string('x') %}{{ dump() }}", {"a": 1}),
    ]:
        exts = ["debug", "string_loader"] if "template_from_string" in tpl else ["debug"]
        cases.append(case(tpl, data, debug=True, extensions=exts))
        if tpl == "{{ dump('<b>') }}":
            cases.append(case(tpl, data, debug=True, extensions=exts, autoescape=True))
    cases.append(case("{{ dump() }}", {"a": 1}, debug=True, extensions=["debug"], globals={"g": "global"}))
    cases.append(case("{{ dump(1) }}", None, debug=False, extensions=["debug"], autoescape=True))
    return cases


# ---- template_from_string ------------------------------------------------------------------------------------------

def string_loader():
    cases = []
    ext = ["string_loader"]
    for tpl, data in [
        ("{{ template_from_string('Hello {{ name }}').render({name: 'x'}) }}", None),
        ("{{ template_from_string('Hello').templateName }}", None),
        ("{{ template_from_string('').templateName }}", None),
        ("{{ template_from_string('Hello {{ name }}').templateName }}", None),
        ("{{ template_from_string('Hello', 'mytpl').templateName }}", None),
        ("{{ template_from_string('Hello', name='mytpl').getTemplateName() }}", None),
        ("{{ template_from_string('{{ x') }}", None),
        ("{{ template_from_string('{{ x', 'mytpl') }}", None),
        ("{{ template_from_string('{% if %}') }}", None),
        ("{{ template_from_string('x') }}", None),
        ("{{ template_from_string(5).render() }}", None),
        ("{{ template_from_string(null) }}", None),
        ("{{ template_from_string(['a']) }}", None),
        ("{{ template_from_string(s).render() }}", {"s": {"$string": "from {{ 'stringable' }}"}}),
        ("{{ template_from_string(o) }}", {"o": {"$object": "TwigRsPlain"}}),
        ("{{ template_from_string('x', ['n']) }}", None),
        ("{{ template_from_string('x', 5).templateName }}", None),
        ("{{ template_from_string('{{ 1 + [] }}').render() }}", None),
        ("{{ template_from_string('{{ x }}').render({x: [1]}) }}", None),
        ("{{ template_from_string('{{ undef.x }}').render() }}", None),
        ("{{ template_from_string('a{{ x }}b').sourceContext.code }}", None),
        ("{{ template_from_string('a').sourceContext.name }}", None),
        ("{{ template_from_string('a').sourceContext.path }}|", None),
        ("{{ template_from_string('x').render('str') }}", None),
        ("{{ template_from_string('{{ g }}').render() }}", None),
        ("{{ template_from_string('{{ v }}').render({v: '<b>'}) }}", None),
        ("{{ template_from_string('{{ v }}', 'tpl.txt').render({v: '<b>'}) }}", None),
        ("{{ template_from_string('{{ 1 }}').render() is same as('1') ? 'string' }}", None),
        ("{{ template_from_string('a') is same as(template_from_string('a')) ? 'same' : 'other' }}", None),
        ("{{ template_from_string('a') == template_from_string('a') ? 'eq' : 'ne' }}", None),
        ("{{ template_from_string('x').defaultEscapeStrategy }}", None),
        ("{{ template_from_string(template='x').render() }}", None),
        ("{{ template_from_string('x', null, 'extra').render() }}", None),
        ("{{ template_from_string(...{template: 'spread'}).render() }}", None),
        ("{{ template_from_string(...{env: 'x'}) }}", None),
        ("{{ template_from_string(...{nope: 'x'}) }}", None),
        ("{{ template_from_string(...[]) }}", None),
        ("{{ template_from_string('{{ \"#{x}\" }}').render({x: 1}) }}", None),
    ]:
        cases.append(case(tpl, data, extensions=ext, globals={"g": "global"}))
    for tpl in ["{{ template_from_string('{{ v }}').render({v: '<b>'}) }}",
                "{{ template_from_string('{{ v }}').templateName }}",
                "{{ template_from_string('{{ v }}', 'x.js').render({v: '<b>'}) }}"]:
        cases.append(case(tpl, None, extensions=ext, autoescape=True))
    cases.append(case("{{ include(template_from_string('{{ 1 + 1 }}')) }}", None, extensions=ext))
    return cases


# ---- random ------------------------------------------------------------------------------------------------------

def random_cases():
    cases = []
    for tpl, data in [
        ("{{ random(0) }}", None), ("{{ random(5, 5) }}", None), ("{{ random(-3, -3) }}", None),
        ("{{ random('a') }}", None), ("{{ random({a: 9}) }}", None), ("[{{ random('') }}]", None),
        ("{{ random([]) }}", None), ("{{ random({}) }}", None), ("{{ random(10, 5) }}", None),
        ("{{ random(2.9, 2.9) }}", None), ("{{ random(true) }}", None), ("[{{ random(false) }}]", None),
        ("{{ random(null, 0) }}", None), ("{{ random(null, -1) }}", None), ("{{ random(null, 'abc') }}", None),
        ("{{ random(null, '0') }}", None), ("{{ random(-2.5, -2.5) }}", None), ("{{ random(0.9) }}", None),
        ("{{ random(-0.5) }}", None), ("{{ random(1, '1') }}", None), ("{{ random(1, [1]) }}", None),
        ("{{ random('é') }}", None), ("{{ random('ééé') }}", None), ("{{ random(b) }}", {"b": {"$bytes": "ff"}}),
        ("{{ random(b) }}", {"b": {"$bytes": "61ff"}}), ("{{ random(a) }}", {"a": {"$arrayobject": ["x"]}}),
        ("{{ random(a) }}", {"a": {"$arrayobject": []}}), ("{{ random(o) is same as(o) ? 'same' }}", {"o": {"$object": "TwigRsPlain"}}),
        ("{{ random(m) }}", {"m": {"$markup": "q"}}),
        ("{{ random([1, 2, 3]) in [1, 2, 3] ? 'ok' }}", None), ("{{ random(10) <= 10 and random(10) >= 0 ? 'ok' }}", None),
        ("{{ random() >= 0 ? 'ok' }}", None), ("{{ random('abc') in 'abc' ? 'ok' }}", None),
        ("{{ random(values=[7]) }}", None), ("{{ random(max=0) }}", None), ("{{ random(7, max=7) }}", None),
        ("{{ random(...{values: 'z'}) }}", None), ("{{ random(...{charset: 'x'}) }}", None),
        ("{{ random(...{nope: 1}) }}", None), ("{{ random(1, 1, 'extra') }}", None),
        ("{{ random(9223372036854775807, 9223372036854775807) }}", None),
        ("{{ random(-9223372036854775807 - 1, -9223372036854775807 - 1) }}", None),
        ("{{ random(x) }}", {"x": {"$float": "NAN"}}), ("{{ random(x, x) }}", {"x": {"$float": "INF"}}),
    ]:
        cases.append(case(tpl, data))
    # a single-byte charset: the characters go through iconv() both ways
    for tpl, data in [
        ("{{ random('a') }}", None),
        ("{{ random(b) }}", {"b": {"$bytes": "e9"}}),
        ("{{ random(b) in 'abc' ? 'ok' }}", {"b": {"$bytes": "616263"}}),
        ("{{ random(b) in b ? 'ok' }}", {"b": {"$bytes": "c3a9"}}),
    ]:
        cases.append(case(tpl, data, charset="ISO-8859-1"))
    cases.append(case("{{ random(b) }}", {"b": {"$bytes": "e9"}}, charset="KOI8-U"))
    cases.append(case("{{ random(b) }}", {"b": {"$bytes": "e9"}}, charset="NOPE"))
    return cases


# ---- CoreExtension settings (PHP level, for the unit tests) --------------------------------------------------------

SETTINGS_FORMAT = "Y-m-d H:i:s T e"


def settings_case(call, args, timezone=None, date_format=None, number_format=None, core_timezone=None):
    """A PHP case: a CoreExtension configured with the setters, then one of its methods. Date results are formatted
    with SETTINGS_FORMAT (`->format()`)."""
    setup = ["$core = new \\Twig\\Extension\\CoreExtension();"]
    if date_format is not None:
        setup.append("$core->setDateFormat(%s, %s);" % (php_value(date_format[0]), php_value(date_format[1])))
    if number_format is not None:
        setup.append("$core->setNumberFormat(%s);" % ", ".join(php_value(v) for v in number_format))
    if core_timezone is not None:
        setup.append("$core->setTimezone(%s);" % php_value(core_timezone))
    call_php = "$core->%s(%s)" % (call, ", ".join(php_value(a) for a in args))
    if call in ("convertDate", "modifyDate"):
        body = "return %s->format(%s);" % (call_php, php_string(SETTINGS_FORMAT))
    else:
        body = "return %s;" % call_php
    c = {"php_code": " ".join(setup) + " " + body, "call": call, "args": args}
    if timezone:
        c["timezone"] = timezone
    for key, value in [("date_format", date_format), ("number_format", number_format),
                       ("core_timezone", core_timezone)]:
        if value is not None:
            c[key] = value
    return c


def settings():
    cases = []
    dt = {"$date": ["DateTime", "2010-10-04 13:45", None]}
    for date_format in [None, ["Y-m-d", "%d days %h hours"], ["d/m/Y", None], [None, "%d jours"], [5, None]]:
        for value in [1286199900, "2010-10-04 13:45", dt, {"$interval": "P2DT3H"}, {"$date": ["DateTimeImmutable", "2010-10-04 13:45", "Asia/Tokyo"]}]:
            cases.append(settings_case("formatDate", [value], date_format=date_format, timezone="UTC"))
    # TWIG/tests/Fixtures/filters/date_time_zone_conversion.test: PHP default Europe/Paris, Twig timezone UTC
    values = [1286199900, {"$date": ["DateTime", "2010-10-04 13:45", None]}, "2010-10-04 13:45", -189291360,
              {"$date": ["DateTime", "2010-10-04 13:45", "America/New_York"]}, "2010-01-28T15:00:00+04:00"]
    for core_timezone in [None, "UTC", "Asia/Hong_Kong", {"$tz": "America/Chicago"}, "+05:00"]:
        for value in values:
            for zone in [None, "Europe/Paris", False]:
                cases.append(settings_case("formatDate", [value, "d/m/Y H:i:s P e", zone], timezone="Europe/Paris",
                                           core_timezone=core_timezone))
            cases.append(settings_case("convertDate", [value], timezone="Europe/Paris", core_timezone=core_timezone))
            cases.append(settings_case("modifyDate", [value, "+1 day"], timezone="Europe/Paris",
                                       core_timezone=core_timezone))
        cases.append(settings_case("formatDate", [None, "e"], timezone="Europe/Paris", core_timezone=core_timezone))
        cases.append(settings_case("formatDate", ["now", "e"], timezone="Europe/Paris", core_timezone=core_timezone))
        cases.append(settings_case("formatDate", ["now", "e", False], timezone="Europe/Paris",
                                   core_timezone=core_timezone))
    for number_format in [None, [2, "!", "="], [1, ",", "."], ["2", ".", " "], [0, "", ""], [3, None, None]]:
        for value in [20, 20.25, 1020.25, -1234567.891, "abc"]:
            for args in [[value], [value, 1], [value, 2, ","], [value, 2, ",", "."], [value, None, None, "'"]]:
                cases.append(settings_case("formatNumber", args, number_format=number_format))
    for core_timezone in ["Nowhere/City", ""]:
        cases.append(settings_case("formatDate", [0], core_timezone=core_timezone))
    return cases


TOPICS = {
    "numbers": numbers,
    "dates": dates,
    "constants": constants,
    "tests": tests,
    "debug": debug,
    "string_loader": string_loader,
    "random": random_cases,
    "settings": settings,
}


def write(topic, cases, oracle):
    for i, c in enumerate(cases):
        c["id"] = "%s-%03d" % (topic, i + 1)
    batch = {"cases": cases, "isolation": "none"}
    gen_path = os.path.join(OUT, topic + ".gen.json")
    with open(gen_path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(batch, f, indent=1, ensure_ascii=False)
        f.write("\n")
    if not oracle:
        print(topic, len(cases), "cases")
        return
    with open(gen_path, "rb") as f:
        data = f.read()
    p = subprocess.run(["docker", "run", "--rm", "-i", "twig-oracle:3.31.1", "php", "runner.php", "--pretty"],
                       input=data, capture_output=True)
    if p.returncode != 0:
        print(p.stderr.decode(), file=sys.stderr)
    results = json.loads(p.stdout.decode())
    out_path = os.path.join(OUT, topic + ".json")
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(results, f, indent=1, ensure_ascii=False)
        f.write("\n")
    failures = [r for r in results if r.get("phase") in ("fatal", "lost")]
    print(topic, len(cases), "cases", "fatal:" if failures else "", failures[:1])


def main():
    args = sys.argv[1:]
    oracle = "--oracle" in args
    names = [a for a in args if not a.startswith("--")] or list(TOPICS)
    for name in names:
        write(name, TOPICS[name](), oracle)


if __name__ == "__main__":
    main()

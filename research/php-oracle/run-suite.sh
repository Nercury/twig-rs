#!/bin/sh
# Runs Twig 3.31.1's own PHPUnit suite inside the oracle image and prints pass/fail counts.
#
#   run-suite.sh [integration|all] [extra phpunit args...]
#
#   integration (default): tests/IntegrationTest.php only (every .test fixture: testIntegration +
#                          testLegacyIntegration)
#   all:                   the whole tests/ directory (what Twig's CI runs)
#
# From Windows Git Bash (MSYS_NO_PATHCONV=1 keeps Git Bash from rewriting arguments that look like paths):
#   MSYS_NO_PATHCONV=1 docker run --rm twig-oracle:3.31.1 ./run-suite.sh integration
#   MSYS_NO_PATHCONV=1 docker run --rm twig-oracle:3.31.1 ./run-suite.sh all
#   MSYS_NO_PATHCONV=1 docker run --rm twig-oracle:3.31.1 ./run-suite.sh integration --filter 'filters/abs'
#
# PHPUnit runs from the Twig repository root (/oracle/vendor/twig/twig) like in Twig's CI, with the image's
# non-root user. The process exit status is PHPUnit's.
set -u
SUITE="${1:-integration}"
[ $# -gt 0 ] && shift
JUNIT=/tmp/twig-suite-junit.xml
rm -f "$JUNIT"
cd /oracle/vendor/twig/twig || exit 2

php /oracle/vendor/bin/phpunit -c phpunit-oracle.xml --testsuite "$SUITE" --log-junit "$JUNIT" "$@"
STATUS=$?

php -r '
require "/oracle/vendor/autoload.php";
$f = $argv[1];
if (!is_file($f)) { fwrite(STDERR, "no junit log produced\n"); exit(0); }
$x = simplexml_load_file($f);
$tests = 0; $failures = 0; $errors = 0; $skipped = 0; $warnings = 0; $assertions = 0;
foreach ($x->testsuite as $s) {
    $tests += (int) $s["tests"]; $failures += (int) $s["failures"]; $errors += (int) $s["errors"];
    $skipped += (int) $s["skipped"]; $warnings += (int) $s["warnings"]; $assertions += (int) $s["assertions"];
}
$passed = $tests - $failures - $errors - $skipped - $warnings;
printf("\n== twig/twig %s suite \"%s\" on PHP %s: %d tests, %d passed, %d failed, %d errors, %d skipped, %d warnings, %d assertions\n",
    \Twig\Environment::VERSION, $argv[2], PHP_VERSION, $tests, $passed, $failures, $errors, $skipped, $warnings, $assertions);
foreach ($x->xpath("//testcase[failure or error]") as $tc) {
    $kind = isset($tc->failure) ? "FAIL" : "ERROR";
    $msg = (string) ($tc->failure ?? $tc->error);
    printf("   %s %s::%s\n      %s\n", $kind, $tc["class"], $tc["name"], str_replace("\n", "\n      ", trim(substr($msg, 0, 600))));
}
foreach ($x->xpath("//testcase[skipped]") as $tc) {
    printf("   SKIP %s::%s\n", $tc["class"], $tc["name"]);
}
' "$JUNIT" "$SUITE"
exit $STATUS

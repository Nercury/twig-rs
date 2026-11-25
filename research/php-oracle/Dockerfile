# twig-rs PHP oracle: PHP 8.5 CLI + Twig 3.31.1 (pinned by composer.lock, installed from git source so that
# Twig's tests/ directory - fixtures, helper classes, PHPUnit suite - is available).
#
# Build (from Windows Git Bash, in this directory):
#   docker build -t twig-oracle:3.31.1 .
# Render / eval (see runner.php for the JSON format):
#   ./oracle.sh < batch.json
# Twig's own PHPUnit suite:
#   docker run --rm twig-oracle:3.31.1 ./run-suite.sh integration
FROM php:8.5-cli@sha256:dbf32b33595283b79fd1a37ebc1614bdf1b83d3750daad9a57d67dbb9b96ec16

COPY --from=composer:2.11.3 /usr/bin/composer /usr/bin/composer

RUN apt-get update \
    && apt-get install -y --no-install-recommends git unzip \
    && rm -rf /var/lib/apt/lists/*

ENV COMPOSER_ALLOW_SUPERUSER=1 \
    COMPOSER_NO_INTERACTION=1 \
    SYMFONY_PHPUNIT_DISABLE_RESULT_CACHE=1

# No php.ini is installed by the official image: PHP runs with built-in defaults
# (precision=14, serialize_precision=-1, date.timezone=UTC, error_reporting=E_ALL, zend.assertions=1).
# Only raise the memory limit (Twig's CI uses memory_limit=-1).
RUN echo 'memory_limit=-1' > /usr/local/etc/php/conf.d/oracle.ini

WORKDIR /oracle
COPY composer.json composer.lock ./
RUN composer install --no-progress \
    && test -f vendor/twig/twig/tests/IntegrationTest.php \
    # Two Twig tests require "<twig root>/vendor/autoload.php"; point it at our vendor dir.
    && ln -s /oracle/vendor vendor/twig/twig/vendor \
    && git -C vendor/twig/twig log -1 --format='twig/twig source: %H %d'

COPY lib.php runner.php run-suite.sh phpunit-twig.xml ./
COPY tools ./tools
RUN chmod +x run-suite.sh && php -l runner.php && php -l lib.php \
    && cp phpunit-twig.xml vendor/twig/twig/phpunit-oracle.xml \
    && useradd --create-home --uid 1000 oracle

# Non-root: Twig's cache tests rely on chmod() making directories unwritable, which root ignores.
USER oracle
CMD ["php", "runner.php"]

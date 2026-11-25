/*
 * LD_PRELOAD shim for the twig-date vector generators: makes PHP's clock (gettimeofday(), time()) return the
 * instant in the environment variables FAKE_SEC / FAKE_USEC, so "now"-dependent results are reproducible.
 * Generators change the instant per case with putenv("FAKE_SEC=…"), which updates the C environment.
 * Built and loaded by regen.sh inside the oracle image.
 */
#define _GNU_SOURCE
#include <stdlib.h>
#include <sys/time.h>
#include <time.h>

static long long fake(const char *name)
{
    const char *v = getenv(name);
    return v ? atoll(v) : 0;
}

int gettimeofday(struct timeval *restrict tv, void *restrict tz)
{
    (void) tz;
    if (tv) {
        tv->tv_sec = (time_t) fake("FAKE_SEC");
        tv->tv_usec = (suseconds_t) fake("FAKE_USEC");
    }
    return 0;
}

time_t time(time_t *t)
{
    time_t s = (time_t) fake("FAKE_SEC");
    if (t) {
        *t = s;
    }
    return s;
}

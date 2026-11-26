/*
 * Black-box tracer for iconv(3) in the oracle image, used while working out how glibc's iconv() reports errors,
 * where a call stops (E2BIG, rounds of 8160 characters, //IGNORE), when UTF-16/UTF-32 write a byte order mark and
 * which characters the CP1255 and CP1258 decoders hold back until the next call or the flush.
 * It only calls the public iconv_open()/iconv() API and prints what comes back; it generates no vector file (the
 * vectors come from PHP, see the *.gen.php next to it) and the tests do not need it.
 *
 * Each input line is "FROM TO INPUT SPACE MODE", the charset names and the input in hex ("-" for empty):
 *   MODE one   one iconv() call with SPACE bytes of output space;
 *   MODE flush one iconv() call, then the flush (iconv(cd, NULL, NULL, ...)) with SPACE bytes left;
 *   MODE php   PHP's php_iconv_string() loop (SPACE -1: input length + 32), skipping one input byte after EILSEQ
 *              like PHP does for a target ending in //IGNORE, and its flush (the buffer growing by 16 on E2BIG);
 *   MODE seq   successive calls, the output space of each taken from $SEQ (comma separated).
 *
 *   MSYS_NO_PATHCONV=1 docker run --rm -i -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
 *       sh -c 'gcc -O1 -o /tmp/probe /t/probe.c && /tmp/probe' < cases.txt
 */
#include <errno.h>
#include <iconv.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static size_t unhex(const char *h, unsigned char *out)
{
    size_t n = 0;
    if (strcmp(h, "-") == 0) {
        return 0;
    }
    while (h[0] && h[1]) {
        unsigned v;
        sscanf(h, "%2x", &v);
        out[n++] = (unsigned char)v;
        h += 2;
    }
    return n;
}

static void hex(const unsigned char *p, size_t n)
{
    for (size_t i = 0; i < n; i++) {
        printf("%02x", p[i]);
    }
}

static const char *err(int e)
{
    return e == EILSEQ ? "EILSEQ" : e == EINVAL ? "EINVAL" : e == E2BIG ? "E2BIG" : "OTHER";
}

static char line[1 << 23], from_hex[512], to_hex[512], in_hex[1 << 23], mode[16];
static unsigned char in[1 << 22];

int main(void)
{
    setlocale(LC_CTYPE, "C.UTF-8"); /* what PHP runs with in the oracle */
    while (fgets(line, sizeof line, stdin)) {
        long space;
        char from[256], to[256];
        if (sscanf(line, "%511s %511s %8388607s %ld %15s", from_hex, to_hex, in_hex, &space, mode) != 5) {
            continue;
        }
        from[unhex(from_hex, (unsigned char *)from)] = 0;
        to[unhex(to_hex, (unsigned char *)to)] = 0;
        size_t len = unhex(in_hex, in);
        printf("%s %s %s:", from_hex, to_hex, in_hex);
        iconv_t cd = iconv_open(to, from);
        if (cd == (iconv_t)-1) {
            printf(" open-failed %s\n", err(errno));
            continue;
        }
        size_t size = space >= 0 ? (size_t)space : len + 32;
        unsigned char *out = malloc(size + (1 << 22));
        char *in_p = (char *)in, *out_p = (char *)out;
        size_t in_left = len, out_left = size;
        if (strcmp(mode, "seq") == 0) {
            char sizes[512];
            strncpy(sizes, getenv("SEQ") ? getenv("SEQ") : "1000000", sizeof sizes - 1);
            sizes[sizeof sizes - 1] = 0;
            for (char *tok = strtok(sizes, ","); tok && in_left > 0; tok = strtok(NULL, ",")) {
                char *start = out_p, *before = in_p;
                out_left = (size_t)atol(tok);
                errno = 0;
                size_t r = iconv(cd, &in_p, &in_left, &out_p, &out_left);
                printf(" [r=%ld %s consumed=%ld out=", (long)r, r == (size_t)-1 ? err(errno) : "-", (long)(in_p - before));
                hex((unsigned char *)start, out_p - start);
                printf("]");
            }
        } else if (strcmp(mode, "one") == 0 || strcmp(mode, "flush") == 0) {
            errno = 0;
            size_t r = iconv(cd, &in_p, &in_left, &out_p, &out_left);
            printf(" r=%ld %s consumed=%ld out=", (long)r, r == (size_t)-1 ? err(errno) : "-", (long)(in_p - (char *)in));
            hex(out, out_p - (char *)out);
            if (strcmp(mode, "flush") == 0) {
                char *start = out_p;
                errno = 0;
                r = iconv(cd, NULL, NULL, &out_p, &out_left);
                printf(" flush r=%ld %s out=", (long)r, r == (size_t)-1 ? err(errno) : "-");
                hex((unsigned char *)start, out_p - start);
            }
        } else {
            int ignore = strlen(to) >= 9 && strcmp(to + strlen(to) - 8, "//IGNORE") == 0, failed = 0;
            while (in_left > 0) {
                char *before = in_p;
                errno = 0;
                size_t r = iconv(cd, &in_p, &in_left, &out_p, &out_left);
                printf(" [r=%ld %s consumed=%ld left=%ld]", (long)r, r == (size_t)-1 ? err(errno) : "-",
                       (long)(in_p - before), (long)in_left);
                if (r != (size_t)-1) {
                    break;
                }
                if (ignore && errno == EILSEQ) {
                    if (in_left <= 1) {
                        break;
                    }
                    in_p++;
                    in_left--;
                    continue;
                }
                if (errno == E2BIG && in_left > 0) {
                    size_t used = out_p - (char *)out;
                    size += len;
                    out_left = size - used;
                    continue;
                }
                printf(" FAIL");
                failed = 1;
                break;
            }
            while (!failed) {
                errno = 0;
                size_t r = iconv(cd, NULL, NULL, &out_p, &out_left);
                printf(" [flush r=%ld %s]", (long)r, r == (size_t)-1 ? err(errno) : "-");
                if (r != (size_t)-1 || errno != E2BIG) {
                    break;
                }
                size_t used = out_p - (char *)out;
                size += 16;
                out_left = size - used;
            }
            printf(" out=");
            hex(out, out_p - (char *)out);
        }
        printf("\n");
        iconv_close(cd);
        free(out);
    }
    return 0;
}

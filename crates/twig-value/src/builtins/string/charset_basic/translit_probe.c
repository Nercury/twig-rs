/*
 * Black-box measurement of glibc's //TRANSLIT at the level of single iconv() calls, for translit.gen.php and the
 * call vectors (tests/vectors/string/charset_basic/calls.gen.php), through the public iconv_open()/iconv() interface
 * only (UCS-4BE input). Every test runs on a descriptor of its own.
 *
 *   translit_probe entries
 *       The characters ISO-2022-KR cannot represent for which glibc has alternatives, all of which fail: converting
 *       U+0061 and the character with 9 bytes of output space (the header, "a" and the header glibc writes again
 *       before a replacement during the encoder's first run), glibc stops with E2BIG after "a" when it tries an
 *       alternative (it wants room for a byte before each character of it) and keeps the second header when it goes
 *       straight to its default "?" (an empty alternative fits). One line per character with alternatives: its code
 *       point (hex).
 *   translit_probe kmin TARGET
 *       For every character TARGET cannot represent, the least output space in which one call converts it: "CP K",
 *       printed when K is more than one byte (one unit for TARGET UCS-2*).
 *   translit_probe whole
 *       The characters ISO-2022-JP cannot represent of which, after U+3042 (ESC $ B and two bytes), a call with three
 *       bytes of output space writes nothing (where glibc's default "?" leaves the escape sequence of ASCII): one line
 *       per character, its code point (hex).
 *   translit_probe stale TARGET
 *       Lines "X P W" (UCS-4BE hex, "-" for none): converts X, then P, then W with one call each and flushes; prints
 *       what X, W and the flush wrote ("-" for nothing), discarding what P wrote (P stands for the attempts that fail
 *       partway, whose designations stay).
 *   translit_probe seq
 *       Lines "FROM TO INPUT SIZES" (names and input in hex, SIZES comma separated): successive calls with that much
 *       output space each ("f": the flush with 64 bytes). Prints the line's head and, per call,
 *       " [STATUS c=CONSUMED out=HEX]" or " [flush STATUS out=HEX]", STATUS being OK, E2BIG, EILSEQ or EINVAL.
 *
 * Built and run by its users in the oracle image (gcc -O2).
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
    if (n == 0) {
        printf("-");
    }
    for (size_t i = 0; i < n; i++) {
        printf("%02x", p[i]);
    }
}

static const char *status(size_t r)
{
    if (r != (size_t)-1) {
        return "OK";
    }
    return errno == EILSEQ ? "EILSEQ" : errno == EINVAL ? "EINVAL" : errno == E2BIG ? "E2BIG" : "OTHER";
}

static void ucs4(unsigned c, unsigned char *p)
{
    p[0] = (unsigned char)(c >> 24);
    p[1] = (unsigned char)(c >> 16);
    p[2] = (unsigned char)(c >> 8);
    p[3] = (unsigned char)c;
}

/* One call converting n bytes of UCS-4BE into to with space bytes of output: the bytes consumed. */
static size_t call(const char *to, const unsigned char *in, size_t n, size_t space, size_t *written)
{
    static unsigned char out[256];
    iconv_t cd = iconv_open(to, "UCS-4BE");
    if (cd == (iconv_t)-1) {
        fprintf(stderr, "cannot open %s\n", to);
        exit(1);
    }
    char *ip = (char *)in, *op = (char *)out;
    size_t il = n, ol = space;
    iconv(cd, &ip, &il, &op, &ol);
    iconv_close(cd);
    *written = (size_t)(op - (char *)out);
    return n - il;
}

static int skipped(unsigned c)
{
    return (c >= 0xD800 && c <= 0xDFFF) || (c >= 0xE0000 && c <= 0xE007F);
}

static void entries(void)
{
    for (unsigned c = 0x80; c <= 0x10FFFF; c++) {
        unsigned char in[8];
        size_t written;
        if (skipped(c)) {
            continue;
        }
        ucs4(c, in);
        if (call("ISO-2022-KR", in, 4, 64, &written) == 4) {
            continue;
        }
        ucs4(0x61, in);
        ucs4(c, in + 4);
        size_t consumed = call("ISO-2022-KR//TRANSLIT", in, 8, 9, &written);
        if ((consumed == 4 && written == 5) || (consumed == 8 && written == 9)) {
            /* (or an empty alternative fits) */
            printf("%X\n", c);
        } else if (consumed != 4 || written != 9) {
            fprintf(stderr, "U+%04X: unexpected %zu %zu\n", c, consumed, written);
            exit(1);
        }
    }
}

static void kmin(const char *target)
{
    char translit[128];
    size_t unit = strncmp(target, "UCS-2", 5) == 0 ? 2 : 1;
    snprintf(translit, sizeof translit, "%s//TRANSLIT", target);
    for (unsigned c = 0x80; c <= 0x10FFFF; c++) {
        unsigned char in[4];
        size_t written;
        if (skipped(c)) {
            continue;
        }
        ucs4(c, in);
        if (call(target, in, 4, 64, &written) == 4) {
            continue;
        }
        size_t k = unit;
        while (call(translit, in, 4, k, &written) != 4) {
            k++;
            if (k > 64) {
                fprintf(stderr, "U+%04X: no room is enough\n", c);
                exit(1);
            }
        }
        if (k > unit) {
            printf("%X %zu\n", c, k);
        }
    }
}

static void whole(void)
{
    for (unsigned c = 0x80; c <= 0x10FFFF; c++) {
        static unsigned char out[64];
        unsigned char in[8];
        size_t written;
        if (skipped(c)) {
            continue;
        }
        ucs4(c, in);
        if (call("ISO-2022-JP", in, 4, 64, &written) == 4) {
            continue;
        }
        ucs4(0x3042, in);
        ucs4(c, in + 4);
        iconv_t cd = iconv_open("ISO-2022-JP//TRANSLIT", "UCS-4BE");
        char *ip = (char *)in, *op = (char *)out;
        size_t il = 8, ol = 5;
        iconv(cd, &ip, &il, &op, &ol);
        char *start = op;
        ol = 3;
        iconv(cd, &ip, &il, &op, &ol);
        if (op == start && il == 4) {
            printf("%X\n", c);
        }
        iconv_close(cd);
    }
}

static char line[1 << 16], a[1 << 14], b[1 << 14], d[1 << 14], e[1 << 14];
static unsigned char in[1 << 13], out[1 << 14];

static void stale(const char *target)
{
    while (fgets(line, sizeof line, stdin)) {
        if (sscanf(line, "%16383s %16383s %16383s", a, b, d) != 3) {
            continue;
        }
        iconv_t cd = iconv_open(target, "UCS-4BE");
        const char *parts[3] = {a, b, d};
        for (int i = 0; i < 3; i++) {
            size_t n = unhex(parts[i], in);
            char *ip = (char *)in, *op = (char *)out;
            size_t il = n, ol = sizeof out;
            if (iconv(cd, &ip, &il, &op, &ol) == (size_t)-1) {
                fprintf(stderr, "%s: %s does not convert\n", target, parts[i]);
                exit(1);
            }
            if (i != 1) {
                hex(out, (size_t)(op - (char *)out));
                printf(" ");
            }
        }
        char *op = (char *)out;
        size_t ol = sizeof out;
        iconv(cd, NULL, NULL, &op, &ol);
        hex(out, (size_t)(op - (char *)out));
        printf("\n");
        iconv_close(cd);
    }
}

static void seq(void)
{
    while (fgets(line, sizeof line, stdin)) {
        char from[256], to[256];
        if (sscanf(line, "%16383s %16383s %16383s %16383s", a, b, d, e) != 4) {
            continue;
        }
        from[unhex(a, (unsigned char *)from)] = 0;
        to[unhex(b, (unsigned char *)to)] = 0;
        size_t len = unhex(d, in);
        printf("%s %s %s %s:", a, b, d, e);
        iconv_t cd = iconv_open(to, from);
        if (cd == (iconv_t)-1) {
            printf(" open-failed\n");
            continue;
        }
        char *ip = (char *)in;
        size_t il = len;
        for (char *tok = strtok(e, ","); tok; tok = strtok(NULL, ",")) {
            char *op = (char *)out, *before = ip;
            if (tok[0] == 'f') {
                size_t ol = 64;
                errno = 0;
                size_t r = iconv(cd, NULL, NULL, &op, &ol);
                printf(" [flush %s out=", status(r));
            } else {
                size_t ol = (size_t)atol(tok);
                errno = 0;
                size_t r = iconv(cd, &ip, &il, &op, &ol);
                printf(" [%s c=%ld out=", status(r), (long)(ip - before));
            }
            for (char *p = (char *)out; p < op; p++) {
                printf("%02x", (unsigned char)*p);
            }
            printf("]");
        }
        printf("\n");
        iconv_close(cd);
    }
}

int main(int argc, char **argv)
{
    setlocale(LC_CTYPE, "C.UTF-8"); /* what PHP runs with in the oracle */
    if (argc == 2 && strcmp(argv[1], "entries") == 0) {
        entries();
    } else if (argc == 2 && strcmp(argv[1], "whole") == 0) {
        whole();
    } else if (argc == 3 && strcmp(argv[1], "kmin") == 0) {
        kmin(argv[2]);
    } else if (argc == 3 && strcmp(argv[1], "stale") == 0) {
        stale(argv[2]);
    } else if (argc == 2 && strcmp(argv[1], "seq") == 0) {
        seq();
    } else {
        fprintf(stderr, "usage: translit_probe entries | kmin TARGET | whole | stale TARGET | seq\n");
        return 1;
    }
    return 0;
}

/*
 * Black-box measurement of glibc's multi-byte charsets for multibyte.gen.php, through the public iconv_open()/iconv()
 * interface only. Every conversion is one iconv() call and the flush on a reset descriptor, which is what PHP's
 * iconv() does with a descriptor of its own (PHP has no way to look at a call without its flush; the generator
 * checks what it learns here against PHP's iconv() too).
 *
 *   multibyte_probe dec CHARSET [PREFIX [SAMPLE]]
 *       Every byte sequence the charset's structure allows, after the bytes PREFIX (hex, "-" for none: a prefix sets
 *       a state, for example an ISO-2022 designation, and must convert to nothing): breadth first over the sequences
 *       that are incomplete on their own (EINVAL), up to four bytes. One line per sequence:
 *         HEX = CP,CP,...   it decodes to these code points (none: nothing)
 *         HEX I             it is incomplete
 *         HEX X K1-K2 OUT   EILSEQ. With //IGNORE glibc skips K1 to K2 bytes and goes on: every skip length in that
 *                           range gives the same as the real conversion for every follower tried (the end of the
 *                           input, "!!!!", and each byte followed by "!!!!"; beyond two bytes a smaller set), lengths
 *                           beyond the sequence taking bytes of the follower; 0-0 when none does. OUT is what
 *                           //IGNORE makes of the sequence alone: "=CP,..." or "I" (EINVAL)
 *       SAMPLE 1 (the four-byte space of GB18030): four-byte sequences are only printed when they decode, or when
 *       they are invalid and their first and third bytes are 1 modulo 16 (the others behave alike, which the
 *       generator's checks and the vectors test).
 *   multibyte_probe enc CHARSET
 *       Every code point U+0000..U+10FFFF from UCS-4BE: "CP HEX" for those that convert (HEX may be empty).
 *
 * Built and run by multibyte.gen.php in the oracle image (gcc -O2).
 */
#include <errno.h>
#include <iconv.h>
#include <locale.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static unsigned char prefix_bytes[64];
static size_t prefix_len;

/* prefix + s converted (call and flush) on a reset descriptor: 0, EILSEQ, EINVAL or E2BIG, and the output */
static int conv(iconv_t cd, const unsigned char *s, size_t n, unsigned char *out, size_t cap, size_t *olen)
{
    static unsigned char in[4096];
    memcpy(in, prefix_bytes, prefix_len);
    memcpy(in + prefix_len, s, n);
    iconv(cd, NULL, NULL, NULL, NULL);
    char *ip = (char *)in, *op = (char *)out;
    size_t il = prefix_len + n, ol = cap;
    int st = 0;
    errno = 0;
    if (iconv(cd, &ip, &il, &op, &ol) == (size_t)-1) {
        st = errno;
    }
    if (st == 0) {
        errno = 0;
        if (iconv(cd, NULL, NULL, &op, &ol) == (size_t)-1) {
            st = errno;
        }
    }
    *olen = op - (char *)out;
    return st;
}

static void print_cps(const unsigned char *o, size_t n)
{
    for (size_t i = 0; i + 3 < n; i += 4) {
        printf("%s%x", i ? "," : "", (unsigned)o[i] << 24 | o[i + 1] << 16 | o[i + 2] << 8 | o[i + 3]);
    }
}

/* whether two inputs give the same under //IGNORE: the output and whether it ends with EINVAL */
static int same(iconv_t cd, const unsigned char *a, size_t an, const unsigned char *b, size_t bn)
{
    static unsigned char o1[4096], o2[4096];
    size_t l1, l2;
    int s1 = conv(cd, a, an, o1, sizeof o1, &l1) == EINVAL;
    int s2 = conv(cd, b, bn, o2, sizeof o2, &l2) == EINVAL;
    return s1 == s2 && l1 == l2 && memcmp(o1, o2, l1) == 0;
}

static size_t unhex(const char *h, unsigned char *out)
{
    size_t n = 0;
    if (strcmp(h, "-") == 0) {
        return 0;
    }
    for (; h[0] && h[1]; h += 2) {
        unsigned v;
        sscanf(h, "%2x", &v);
        out[n++] = (unsigned char)v;
    }
    return n;
}

static unsigned char queue[5 << 20];
static unsigned char out[1 << 16];

int main(int argc, char **argv)
{
    setlocale(LC_CTYPE, "C.UTF-8"); /* what PHP runs with in the oracle */
    if (argc < 3) {
        return 2;
    }
    const char *cs = argv[2];
    size_t olen;
    if (strcmp(argv[1], "enc") == 0) {
        iconv_t cd = iconv_open(cs, "UCS-4BE");
        if (cd == (iconv_t)-1) {
            return 1;
        }
        for (uint32_t cp = 0; cp < 0x110000; cp++) {
            unsigned char in[4] = {cp >> 24, cp >> 16, cp >> 8, cp};
            if (conv(cd, in, 4, out, sizeof out, &olen) == 0) {
                printf("%x ", cp);
                for (size_t i = 0; i < olen; i++) {
                    printf("%02x", out[i]);
                }
                printf("\n");
            }
        }
        return 0;
    }
    if (argc > 3) {
        prefix_len = unhex(argv[3], prefix_bytes);
    }
    int sample = argc > 4 && strcmp(argv[4], "1") == 0;
    iconv_t plain = iconv_open("UCS-4BE", cs), ignore = iconv_open("UCS-4BE//IGNORE", cs);
    if (plain == (iconv_t)-1 || ignore == (iconv_t)-1) {
        return 1;
    }
    static const unsigned char deep[] = {0x00, 0x21, 0x30, 0x39, 0x40, 0x41, 0x7f, 0x80, 0x81, 0x8e, 0x8f, 0xa0, 0xa1,
                                         0xb0, 0xfe, 0xff};
    size_t qh = 0, qt = 5;
    queue[0] = 0;
    while (qh < qt) {
        size_t plen = queue[qh];
        unsigned char prefix[4];
        memcpy(prefix, queue + qh + 1, 4);
        qh += 5;
        for (int b = 0; b < 256; b++) {
            unsigned char s[8];
            memcpy(s, prefix, plen);
            s[plen] = (unsigned char)b;
            size_t n = plen + 1;
            int st = conv(plain, s, n, out, sizeof out, &olen);
            if (st == EINVAL) {
                for (size_t i = 0; i < n; i++) {
                    printf("%02x", s[i]);
                }
                printf(" I\n");
                if (n < 4) {
                    queue[qt] = (unsigned char)n;
                    memcpy(queue + qt + 1, s, n);
                    qt += 5;
                }
                continue;
            }
            if (st == EILSEQ && sample && n == 4 && !(s[0] % 16 == 1 && s[2] % 16 == 1)) {
                continue;
            }
            for (size_t i = 0; i < n; i++) {
                printf("%02x", s[i]);
            }
            if (st == 0) {
                printf(" = ");
                print_cps(out, olen);
                printf("\n");
                continue;
            }
            if (st != EILSEQ) {
                printf(" F %d\n", st);
                continue;
            }
            /* candidate skip lengths 1..n+2 (beyond n: bytes of the follower) */
            int cand[8] = {0};
            size_t kmax = n + 2;
            for (size_t k = 1; k <= kmax; k++) {
                cand[k] = 1;
            }
            size_t nf = n <= 2 ? 258 : 2 + sizeof deep;
            for (size_t f = 0; f < nf; f++) {
                unsigned char t[16];
                size_t tn = n;
                memcpy(t, s, n);
                if (f == 1) {
                    memcpy(t + tn, "!!!!", 4);
                    tn += 4;
                } else if (f >= 2) {
                    t[tn++] = n <= 2 ? (unsigned char)(f - 2) : deep[f - 2];
                    memcpy(t + tn, "!!!!", 4);
                    tn += 4;
                }
                for (size_t k = 1; k <= kmax; k++) {
                    if (cand[k] && k <= tn && !same(ignore, t, tn, t + k, tn - k)) {
                        cand[k] = 0;
                    }
                }
            }
            int k1 = 0, k2 = 0;
            for (size_t k = 1; k <= kmax; k++) {
                if (cand[k]) {
                    if (!k1) {
                        k1 = (int)k;
                    }
                    k2 = (int)k;
                }
            }
            int ist = conv(ignore, s, n, out, sizeof out, &olen);
            printf(" X %d-%d ", k1, k2);
            if (ist == EINVAL) {
                printf("I");
            } else {
                printf("=");
                print_cps(out, olen);
            }
            printf("\n");
        }
    }
    return 0;
}

/*
 * glibc __isoc23_* compatibility shims for the Inflect-Micro-v2 TTS engine.
 *
 * Mirrors `voxctrl-inference/src/isoc23_compat.c`. Both crates link a prebuilt
 * ONNX Runtime (via `ort` + `download-binaries`) built against glibc >= 2.38,
 * which references __isoc23_strtol/strtoll/... ; linking that archive on an
 * older-glibc host (the ubuntu-22.04 runners ship glibc 2.35) fails with
 * "undefined symbol". This crate needs its own copy because its test binary
 * links ONNX Runtime without voxctrl-inference, so that crate's shim is not in
 * scope.
 *
 * The definitions here are weak, so when both crates end up in one binary the
 * strong definitions from voxctrl-inference win and the duplicate is harmless.
 *
 * Compiled without standard headers on purpose: glibc >= 2.38's <stdlib.h>
 * inline-redirects strtol/strtoul back to __isoc23_*, which would make these
 * definitions recurse into themselves. Declaring the prototypes by hand keeps
 * the calls bound to the classic symbols (e.g. strtol@GLIBC_2.2.5).
 */

long strtol(const char *nptr, char **endptr, int base);
long long strtoll(const char *nptr, char **endptr, int base);
unsigned long strtoul(const char *nptr, char **endptr, int base);
unsigned long long strtoull(const char *nptr, char **endptr, int base);

typedef long int intmax_t;
typedef unsigned long int uintmax_t;
intmax_t strtoimax(const char *nptr, char **endptr, int base);
uintmax_t strtoumax(const char *nptr, char **endptr, int base);

__attribute__((weak)) long __isoc23_strtol(const char *nptr, char **endptr, int base) {
    return strtol(nptr, endptr, base);
}

__attribute__((weak)) long long __isoc23_strtoll(const char *nptr, char **endptr, int base) {
    return strtoll(nptr, endptr, base);
}

__attribute__((weak)) unsigned long __isoc23_strtoul(const char *nptr, char **endptr, int base) {
    return strtoul(nptr, endptr, base);
}

__attribute__((weak)) unsigned long long __isoc23_strtoull(const char *nptr, char **endptr, int base) {
    return strtoull(nptr, endptr, base);
}

__attribute__((weak)) intmax_t __isoc23_strtoimax(const char *nptr, char **endptr, int base) {
    return strtoimax(nptr, endptr, base);
}

__attribute__((weak)) uintmax_t __isoc23_strtoumax(const char *nptr, char **endptr, int base) {
    return strtoumax(nptr, endptr, base);
}

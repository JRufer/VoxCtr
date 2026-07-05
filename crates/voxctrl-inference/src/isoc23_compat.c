/*
 * glibc __isoc23_* compatibility shims for the Moonshine backend.
 *
 * The prebuilt ONNX Runtime static library (pyke, pulled in by `ort` with
 * `download-binaries`) is compiled against glibc >= 2.38. Under C23, glibc
 * redirects strtol/strtoul/... to __isoc23_* symbols, so the static archive
 * references e.g. __isoc23_strtol. Our release runners are ubuntu-22.04
 * (glibc 2.35), which has no such symbols, and the static link fails with
 * "undefined symbol: __isoc23_strtol".
 *
 * These forwarders provide the symbols by calling the classic libc functions,
 * so the binary links and runs on older glibc while keeping the AppImage's low
 * glibc floor. The only C23 difference is that base 0 also accepts a "0b"
 * binary prefix; ONNX Runtime's integer config parsing does not depend on that,
 * so forwarding to the classic functions is safe.
 *
 * Compiled and linked only for the `moonshine` feature on Linux (see build.rs).
 */

#include <stdlib.h>
#include <inttypes.h>

long __isoc23_strtol(const char *nptr, char **endptr, int base) {
    return strtol(nptr, endptr, base);
}

long long __isoc23_strtoll(const char *nptr, char **endptr, int base) {
    return strtoll(nptr, endptr, base);
}

unsigned long __isoc23_strtoul(const char *nptr, char **endptr, int base) {
    return strtoul(nptr, endptr, base);
}

unsigned long long __isoc23_strtoull(const char *nptr, char **endptr, int base) {
    return strtoull(nptr, endptr, base);
}

intmax_t __isoc23_strtoimax(const char *nptr, char **endptr, int base) {
    return strtoimax(nptr, endptr, base);
}

uintmax_t __isoc23_strtoumax(const char *nptr, char **endptr, int base) {
    return strtoumax(nptr, endptr, base);
}

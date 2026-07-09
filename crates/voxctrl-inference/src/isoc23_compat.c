/*
 * glibc __isoc23_* compatibility shims for the Moonshine backend.
 *
 * This compatibility layer is compiled without standard headers (like stdlib.h)
 * to prevent glibc >= 2.38 headers from inline-redirecting the calls to strtol/strtoul
 * back to __isoc23_* (which would create an infinite recursion loop at compile-time).
 *
 * Declaring the prototypes manually ensures the compiled code calls the real external
 * classic functions (e.g. strtol@GLIBC_2.2.5), making the binary portable and stable.
 */

long strtol(const char *nptr, char **endptr, int base);
long long strtoll(const char *nptr, char **endptr, int base);
unsigned long strtoul(const char *nptr, char **endptr, int base);
unsigned long long strtoull(const char *nptr, char **endptr, int base);

typedef long int intmax_t;
typedef unsigned long int uintmax_t;
intmax_t strtoimax(const char *nptr, char **endptr, int base);
uintmax_t strtoumax(const char *nptr, char **endptr, int base);

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

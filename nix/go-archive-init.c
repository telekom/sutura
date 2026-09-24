/*
 * Starts the Go runtime of the ADBC driver `c-archive` with an argv it can walk.
 *
 * Go's archive registers `_rt0_<arch>_linux_lib` in `.init_array` and reads argc/argv from the
 * first two argument registers. glibc calls init_array entries with (argc, argv, envp); musl calls
 * them with none, so the runtime read whatever the registers held, walked it as argv and died on
 * SIGSEGV before `main` - every static musl artefact, with no output at all. Measured on Go 1.27.1
 * against musl: static, static-pie and dynamic links all exit 139; with this entry all exit 0. Go
 * has no guard for it (its `sysargs` trusts argv unconditionally).
 *
 * `nix/bigquery-adbc.nix` strips the archive's own `.init_array` entry and links this one into
 * the same object, so the runtime starts exactly once, on every libc, from here.
 *
 * The argv handed over is argc 0, then the process environment, then an empty auxv: the runtime
 * reads its environment from after argv (the driver's credential discovery needs it) and, finding
 * no auxv, falls back to /proc/self/auxv as it does when loaded as a library elsewhere. The array
 * is never freed because the runtime keeps pointing into it.
 */
#include <stdlib.h>
#include <string.h>

extern char **environ;

#if defined(__x86_64__)
#define GO_RT0_LIB _rt0_amd64_linux_lib
#elif defined(__aarch64__)
#define GO_RT0_LIB _rt0_arm64_linux_lib
#else
#error "no Go library entrypoint known for this architecture"
#endif

extern void GO_RT0_LIB(int argc, char **argv);

__attribute__((constructor)) static void sutura_go_runtime_init(void) {
  size_t envc = 0;
  while (environ != NULL && environ[envc] != NULL) envc++;
  /* argv terminator, the environment, its terminator, and one AT_NULL auxv pair. */
  char **argv = calloc(envc + 4, sizeof *argv);
  if (argv == NULL) abort();
  if (envc > 0) memcpy(argv + 1, environ, envc * sizeof *argv);
  GO_RT0_LIB(0, argv);
}

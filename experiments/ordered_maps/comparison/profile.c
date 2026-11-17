// Single-threaded arm64 macOS measurement probe. Resolve sampled PCs after the
// run.
#include <dlfcn.h>
#include <mach-o/dyld.h>
#include <pthread.h>
#include <ptrauth.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include <sys/ucontext.h>
#include <unistd.h>
#define WRAP(old, new)                                                         \
  __attribute__((used, section("__DATA,__interpose"))) static const struct {   \
    const void *a, *b;                                                         \
  } pair_##old = {(const void *)new, (const void *)old};
static _Atomic unsigned long long allocs, frees, resized, requested;
static void *cmalloc(size_t n) {
  allocs++;
  requested += n;
  return malloc(n);
}
static void *ccalloc(size_t n, size_t m) {
  allocs++;
  requested += n * m;
  return calloc(n, m);
}
static void *crealloc(void *p, size_t n) {
  resized++;
  requested += n;
  return realloc(p, n);
}
static void cfree(void *p) {
  frees += p != NULL;
  free(p);
}
#ifndef VN_CPU_ONLY
WRAP(malloc, cmalloc)
WRAP(calloc, ccalloc)
WRAP(realloc, crealloc)
WRAP(free, cfree)
#endif
#define MAX_SAMPLES 12000
#define DEPTH 24
static uintptr_t traces[MAX_SAMPLES][DEPTH];
static volatile sig_atomic_t samples;
static uintptr_t low, high;
static void sample(int sig, siginfo_t *info, void *arg) {
  ucontext_t *u = arg;
  if (samples >= MAX_SAMPLES)
    return;
  uintptr_t *out = traces[samples++];
  out[0] = u->uc_mcontext->__ss.__pc;
  uintptr_t fp = u->uc_mcontext->__ss.__fp;
  for (int i = 1; i < DEPTH && fp >= low && fp <= high - 16 && !(fp % 16);
       i++) {
    uintptr_t *frame = (uintptr_t *)fp;
    out[i] =
        (uintptr_t)ptrauth_strip((void *)frame[1], ptrauth_key_return_address);
    if (frame[0] <= fp)
      break;
    fp = frame[0];
  }
}
__attribute__((constructor)) static void begin(void) {
  if (!getenv("VN_SAMPLE"))
    return;
  pthread_t t = pthread_self();
  high = (uintptr_t)pthread_get_stackaddr_np(t);
  low = high - pthread_get_stacksize_np(t);
  struct sigaction a = {0};
  a.sa_sigaction = sample;
  a.sa_flags = SA_SIGINFO | SA_RESTART;
  sigaction(SIGPROF, &a, 0);
  struct itimerval it = {{0, 500}, {0, 500}};
  setitimer(ITIMER_PROF, &it, NULL);
}
__attribute__((destructor)) static void finish(void) {
  struct itimerval stop = {0};
  setitimer(ITIMER_PROF, &stop, NULL);
  fprintf(stderr, "heap %llu %llu %llu %llu\n", atomic_load(&allocs),
          atomic_load(&resized), atomic_load(&frees), atomic_load(&requested));
  for (int i = 0; i < samples; i++) {
    for (int j = 0; j < DEPTH && traces[i][j]; j++) {
      Dl_info d = {0};
      if (!dladdr((void *)traces[i][j], &d))
        break;
      const char *f = strrchr(d.dli_fname, '/');
      f = f ? f + 1 : d.dli_fname;
      fprintf(stderr, "%s@%lx:%s ", f,
              (unsigned long)(traces[i][j] - (uintptr_t)d.dli_fbase),
              d.dli_sname ? d.dli_sname : "?");
    }
    fputc('\n', stderr);
  }
}

// Single-threaded search fixture. Count each request in the innermost ABI call.
#include "probe.h"
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
  size_t calls, allocations, reallocations, bytes;
} Counts;
static Counts counts[BUCKETS];
static int current = -1;

int probe_enter(int next) {
  int previous = current;
  if (previous >= 0) {
    current = next;
    counts[next].calls++;
  }
  return previous;
}
void probe_leave(int previous) { current = previous; }
void probe_begin(void) {
  memset(counts, 0, sizeof counts);
  current = 0;
  counts[0].calls = 1;
}
void probe_end(void) {
  current = -1;
  fputs("query\n", stderr);
  for (int i = 0; i < BUCKETS; i++) {
    Counts c = counts[i];
    if (c.calls)
      fprintf(stderr, "%d %zu %zu %zu %zu\n", i, c.calls, c.allocations,
              c.reallocations, c.bytes);
  }
}
static void *count_malloc(size_t size) {
  if (pthread_main_np() && current >= 0) {
    counts[current].allocations++;
    counts[current].bytes += size;
  }
  return malloc(size);
}
static void *count_calloc(size_t count, size_t size) {
  if (pthread_main_np() && current >= 0) {
    counts[current].allocations++;
    counts[current].bytes += count * size;
  }
  return calloc(count, size);
}
static void *count_realloc(void *p, size_t size) {
  if (pthread_main_np() && current >= 0) {
    counts[current].reallocations++;
    counts[current].bytes += size;
  }
  return realloc(p, size);
}
#define INTERPOSE(old, new)                                                    \
  __attribute__((used, section("__DATA,__interpose"))) static const struct {   \
    const void *replacement, *original;                                        \
  } wrap_##old = {(const void *)new, (const void *)old};
INTERPOSE(malloc, count_malloc)
INTERPOSE(calloc, count_calloc)
INTERPOSE(realloc, count_realloc)

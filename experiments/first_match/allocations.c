// Single-threaded search fixture. Count each request in the innermost ABI call.
#include "probe.h"
#include <malloc/malloc.h>
#include <stddef.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
  size_t calls, allocations, reallocations, bytes;
} Counts;
static Counts counts[BUCKETS];
static int current = -1;
static int peak_enabled;
static ptrdiff_t live_delta, peak_delta;
static void memory_change(size_t before, size_t after) {
  if (peak_enabled && pthread_main_np() && current >= 0) {
    live_delta += (ptrdiff_t)after - (ptrdiff_t)before;
    if (live_delta > peak_delta) peak_delta = live_delta;
  }
}

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
  peak_enabled = getenv("VN_ALLOC_PEAK") != NULL;
  live_delta = peak_delta = 0;
  memset(counts, 0, sizeof counts);
  current = 0;
  counts[0].calls = 1;
}
void probe_end(void) {
  current = -1;
  fputs("query\n", stderr);
  if (peak_enabled)
    fprintf(stderr, "peak %td\n", peak_delta);
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
  void *result = malloc(size);
  if (peak_enabled && result) memory_change(0, malloc_size(result));
  return result;
}
static void *count_calloc(size_t count, size_t size) {
  if (pthread_main_np() && current >= 0) {
    counts[current].allocations++;
    counts[current].bytes += count * size;
  }
  void *result = calloc(count, size);
  if (peak_enabled && result) memory_change(0, malloc_size(result));
  return result;
}
static void *count_realloc(void *p, size_t size) {
  if (pthread_main_np() && current >= 0) {
    counts[current].reallocations++;
    counts[current].bytes += size;
  }
  size_t before = peak_enabled && p ? malloc_size(p) : 0;
  void *result = realloc(p, size);
  if (peak_enabled && result) memory_change(before, malloc_size(result));
  return result;
}
static void count_free(void *p) {
  if (peak_enabled && p) memory_change(malloc_size(p), 0);
  free(p);
}
#define INTERPOSE(old, new)                                                    \
  __attribute__((used, section("__DATA,__interpose"))) static const struct {   \
    const void *replacement, *original;                                        \
  } wrap_##old = {(const void *)new, (const void *)old};
INTERPOSE(malloc, count_malloc)
INTERPOSE(calloc, count_calloc)
INTERPOSE(realloc, count_realloc)
INTERPOSE(free, count_free)

// Wrap only the outer search call. The predicate and each checked step run normally.
#include <stdint.h>
#include <stdio.h>
#include <time.h>

extern uint64_t tellur_rt_v1_call(void *, void *, const char *, size_t,
                                  const uint64_t *, size_t);

uint64_t time_first(void *ctx, void *control, const char *name, size_t length,
                    const uint64_t *args, size_t count) {
    uint64_t start = clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
    uint64_t result = tellur_rt_v1_call(ctx, control, name, length, args, count);
    uint64_t elapsed = clock_gettime_nsec_np(CLOCK_UPTIME_RAW) - start;
    fprintf(stderr, "%llu\n", (unsigned long long)elapsed);
    return result;
}

// Time the complete summarized call with the same boundary as ordinary calls.
#include <stdint.h>
#include <stdio.h>
#include <time.h>

extern uint64_t tellur_rt_v1_join_call(void *, void *, const char *, size_t,
    const uint64_t *, size_t, size_t, const char *, size_t);
uint64_t time_first_join(void *ctx, void *control, const char *name, size_t length,
    const uint64_t *args, size_t count, size_t destination,
    const char *outcome, size_t outcome_length) {
    uint64_t start = clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
    uint64_t result = tellur_rt_v1_join_call(ctx, control, name, length, args,
        count, destination, outcome, outcome_length);
    uint64_t elapsed = clock_gettime_nsec_np(CLOCK_UPTIME_RAW) - start;
    fprintf(stderr, "%llu\n", (unsigned long long)elapsed);
    return result;
}

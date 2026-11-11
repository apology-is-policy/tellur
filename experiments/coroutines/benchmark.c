// One-process measurement driver: link with a specimen whose main is renamed
// specimen_main. Parsing/warmup/output are outside the measured interval.
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

int specimen_main(int argc, char **argv);
void rt_benchmark(int enabled);
uint64_t rt_checksum(void);

static uint64_t nanos(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) exit(72);
    return (uint64_t)t.tv_sec * 1000000000 + (uint64_t)t.tv_nsec;
}
int main(int argc, char **argv) {
    if (argc != 3) return 64;
    char *end;
    long iterations = strtol(argv[2], &end, 10);
    if (*end || iterations < 1 || iterations > 1000000) return 64;
    rt_benchmark(1);
    int expected = specimen_main(argc, argv);
    for (int i = 0; i < 1000; ++i)
        if (specimen_main(argc, argv) != expected) return 71;
    uint64_t start = nanos();
    for (long i = 0; i < iterations; ++i)
        if (specimen_main(argc, argv) != expected) return 71;
    uint64_t elapsed = nanos() - start;
    printf("{\"iterations\":%ld,\"elapsed_ns\":%" PRIu64 ",\"checksum\":%" PRIu64
           ",\"status\":%d}\n", iterations, elapsed, rt_checksum(), expected);
    return 0;
}

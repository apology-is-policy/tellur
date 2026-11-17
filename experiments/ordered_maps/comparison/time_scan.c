// macOS benchmark interposer for a single compiled call in the emitted object.
#include <stdint.h>
#include <stdio.h>
#include <time.h>

#ifdef BENCH_DRAIN
extern void tellur_rt_v1_map_drain(void *, uint64_t, size_t, const uint8_t *,
                                   size_t);
void bench_map_drain(void *ctx, uint64_t owner, size_t ticks,
                     const uint8_t *projections, size_t count) {
  uint64_t start = clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
  tellur_rt_v1_map_drain(ctx, owner, ticks, projections, count);
#else
extern void tellur_rt_v1_map_scan(void *, size_t, uint32_t, uint64_t, size_t,
                                  uint32_t, const uint8_t *, size_t);

void bench_map_scan(void *ctx, size_t slot, uint32_t indirect,
                    uint64_t position, size_t ticks, uint32_t wrapped,
                    const uint8_t *projections, size_t count) {
  uint64_t start = clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
  tellur_rt_v1_map_scan(ctx, slot, indirect, position, ticks, wrapped,
                        projections, count);
#endif
  uint64_t elapsed = clock_gettime_nsec_np(CLOCK_UPTIME_RAW) - start;
  fprintf(stderr, "%llu\n", (unsigned long long)elapsed);
}

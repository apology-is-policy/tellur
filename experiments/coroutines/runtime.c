// E2.1 experiment runtime for hand-lowered LLVM coroutines (see README.md).
//
// A single-thread cooperative executor with the reference executor's observable
// rules: FIFO runnable tasks; scripted host operations consumed when they start;
// a pending operation completes after `waits` idle signals, the oldest first;
// sticky cancellation inherited by later descendants; cancellation observed only
// by cancellable operations, which then stop without an event; joins never
// interrupt. The runtime owns scheduling, readiness and cancellation; frames
// own only the control state of their Tellur control.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Defined in intrinsics.ll.
void coro_resume(void *frame);
void coro_destroy(void *frame);
int coro_done(void *frame);

#define MAX_TASKS 64
#define MAX_CALLS 64
#define MAX_RESOURCES 16
#define PENDING (-1)
#define CANCELLED 99

static void fail(const char *message, const char *detail) {
    fprintf(stderr, "coroutine runtime: %s%s%s\n", message, detail ? ": " : "", detail ? detail : "");
    exit(70);
}

// ---- scenario script --------------------------------------------------------
struct call {
    char operation[32];
    char outcome[32];
    int waits;
    char payload[128];
};
static struct call calls[MAX_CALLS];
static int call_count, next_call;

static void load_script(const char *path) {
    FILE *file = fopen(path, "r");
    if (!file) fail("cannot read script", path);
    char line[256];
    while (fgets(line, sizeof line, file)) {
        struct call *c = &calls[call_count];
        c->payload[0] = 0;
        int fields = sscanf(line, "%31s %31s %d %127[^\n]", c->operation, c->outcome, &c->waits, c->payload);
        if (fields < 3) continue;
        if (++call_count == MAX_CALLS) fail("script too long", NULL);
    }
    fclose(file);
}
static struct call *consume(const char *operation) {
    if (next_call == call_count) fail("script exhausted before", operation);
    struct call *c = &calls[next_call++];
    if (strcmp(c->operation, operation)) fail("script expects another operation than", operation);
    return c;
}
static int outcome_index(const struct call *c, const char *const *names) {
    for (int i = 0; names[i]; i++)
        if (!strcmp(c->outcome, names[i])) return i;
    fail("unknown outcome", c->outcome);
    return 0;
}
static void event(const char *text) { printf("%s\n", text); }

// ---- frames -------------------------------------------------------------------
// Coroutine frames are allocated here so that the runtime can prove every frame
// is released exactly once, including frames destroyed while suspended.
static int live_frames;
void *rt_frame_alloc(long size) {
    live_frames++;
    return malloc(size);
}
void rt_frame_free(void *frame) {
    if (live_frames-- == 0) fail("frame released twice", NULL);
    free(frame);
}

// ---- resources and loans ------------------------------------------------------
static int resources, borrows[MAX_RESOURCES];

// ---- tasks ------------------------------------------------------------------
struct task {
    void *frame;        // top-level coroutine of the task
    int owner;          // spawning task; -1 for the root
    int resource;       // borrowed resource held until the join, or -1
    int cancelled, done, joined, destroyed;
    int outcome;
    char payload[128];
    int waiter;         // task blocked in a join on this one, or -1
    int set;            // owning task set, or -1
    int finished_at;    // completion order, for `await next`
    // pending host operation
    int pending, cancellable, op_outcome, op_seq;
    char op_payload[128];
    const struct call *op_call;
};
static struct task tasks[MAX_TASKS];
static int task_count;
static int runnable[MAX_TASKS * 4], run_head, run_tail;
static int op_clock;

static void push(int task) {
    for (int i = run_head; i < run_tail; i++)
        if (runnable[i] == task) return;
    runnable[run_tail++] = task;
}
static int cancelled(int task) {
    // Sticky and structured: a request on any owner reaches the task.
    for (int t = task; t >= 0; t = tasks[t].owner)
        if (tasks[t].cancelled) return 1;
    return 0;
}

int rt_task_new(int owner, int resource) {
    if (task_count == MAX_TASKS) fail("too many tasks", NULL);
    struct task *t = &tasks[task_count];
    memset(t, 0, sizeof *t);
    t->owner = owner;
    t->resource = resource;
    t->waiter = -1;
    t->set = -1;
    if (resource >= 0) borrows[resource]++;
    return task_count++;
}
void rt_task_start(int task, void *frame) {
    tasks[task].frame = frame;
    push(task);
}
void rt_cancel(int task) {
    tasks[task].cancelled = 1;
    // Wake every pending cancellable operation that now observes a request, FIFO.
    for (;;) {
        int best = -1;
        for (int t = 0; t < task_count; t++)
            if (tasks[t].pending == 1 && tasks[t].cancellable && cancelled(t) &&
                (best < 0 || tasks[t].op_seq < tasks[best].op_seq))
                best = t;
        if (best < 0) break;
        tasks[best].pending = 2; // woken for cancellation; observed at its poll
        push(best);
    }
}
static int completions;
struct set {
    int members[MAX_TASKS], count;
    int waiter;
};
static struct set sets[8];
static int set_count;

void rt_complete(int task, int outcome, const char *payload) {
    struct task *t = &tasks[task];
    t->done = 1;
    t->finished_at = completions++;
    if (t->set >= 0 && sets[t->set].waiter >= 0) {
        push(sets[t->set].waiter);
        sets[t->set].waiter = -1;
    }
    t->outcome = outcome;
    snprintf(t->payload, sizeof t->payload, "%s", payload ? payload : "");
    if (t->waiter >= 0) push(t->waiter);
}
// Consuming join: never interrupted. The loan ends and the frame is destroyed only
// after the child completed and was joined.
int rt_join(int task, int child) {
    struct task *c = &tasks[child];
    if (c->joined) fail("task joined twice", NULL);
    if (!c->done) {
        c->waiter = task;
        return PENDING;
    }
    c->joined = 1;
    if (c->resource >= 0) borrows[c->resource]--;
    if (!coro_done(c->frame)) fail("completed frame not at its final suspend", NULL);
    coro_destroy(c->frame);
    c->destroyed = 1;
    return c->outcome;
}
const char *rt_payload(int task) { return tasks[task].payload; }
// `yield;` and the handoff before a host operation starts: requeue behind the
// runnable tasks, then the frame suspends.
void rt_yield(int task) { push(task); }

// ---- faults -------------------------------------------------------------------
static int fault_task = -1;
static char fault_message[128];
// `fault "message";`: recorded here; the frame then suspends and the driver
// settles the owning domain before reporting.
void rt_fault(int task, const char *message) {
    fault_task = task;
    snprintf(fault_message, sizeof fault_message, "%s", message);
}
int rt_faulted(void) { return fault_task >= 0; }
// Owning settlement for the root domain: pending operations stop without events,
// then every frame is destroyed, borrowers (children) before their owners.
static void settle(int root) {
    for (int t = 0; t < task_count; t++) tasks[t].pending = 0;
    for (int t = task_count - 1; t >= 0; t--) {
        struct task *k = &tasks[t];
        if (t == root || k->destroyed || !k->frame) continue;
        if (k->resource >= 0 && !k->joined) borrows[k->resource]--;
        coro_destroy(k->frame);
        k->destroyed = 1;
    }
    coro_destroy(tasks[root].frame);
    tasks[root].destroyed = 1;
}
int rt_outcome(int task) { return tasks[task].outcome; }

// ---- task sets ------------------------------------------------------------------
int rt_set_new(void) {
    if (set_count == 8) fail("too many task sets", NULL);
    sets[set_count].waiter = -1;
    return set_count++;
}
void rt_set_add(int set, int member) {
    struct set *s = &sets[set];
    s->members[s->count++] = member;
    tasks[member].set = set;
}
// `cancel set;` reaches the current members only.
void rt_set_cancel(int set) {
    for (int i = 0; i < sets[set].count; i++) rt_cancel(sets[set].members[i]);
}
// `await next set`: joins the earliest completed member, -2 when empty, or
// PENDING after registering the caller. Never interrupted.
int rt_set_next(int task, int set) {
    struct set *s = &sets[set];
    if (s->count == 0) return -2;
    int best = -1;
    for (int i = 0; i < s->count; i++) {
        struct task *m = &tasks[s->members[i]];
        if (m->done && (best < 0 || m->finished_at < tasks[s->members[best]].finished_at)) best = i;
    }
    if (best < 0) {
        s->waiter = task;
        return PENDING;
    }
    int member = s->members[best];
    s->members[best] = s->members[--s->count];
    int outcome = rt_join(task, member);
    (void)outcome;
    return member;
}

// ---- host operations ----------------------------------------------------------
static const char *const receive_outcomes[] = {"data", "closed", "failed", NULL};
static const char *const tick_outcomes[] = {"ticked", NULL};

// Starts a scripted async operation. Cancelled before starting: no script entry
// is consumed and no event published.
static int op_start(int task, const char *operation, const char *const *outcomes, int cancellable) {
    struct task *t = &tasks[task];
    if (cancellable && cancelled(task)) return CANCELLED;
    const struct call *c = consume(operation);
    t->op_outcome = outcome_index(c, outcomes);
    snprintf(t->op_payload, sizeof t->op_payload, "%s", c->payload);
    if (c->waits == 0) {
        event(c->operation);
        return t->op_outcome;
    }
    t->pending = 1;
    t->cancellable = cancellable;
    t->op_seq = op_clock++;
    t->op_call = c;
    // Remaining idle signals before completion.
    t->op_outcome |= c->waits << 8;
    return PENDING;
}
int rt_receive_start(int task, int resource, int cancellable) {
    if (resource < 0 || resource >= resources) fail("receive of an unknown resource", NULL);
    return op_start(task, "receive", receive_outcomes, cancellable);
}
int rt_tick_start(int task, const char *label, int cancellable) {
    (void)label; // consumed by the operation; the scripted result carries the label
    return op_start(task, "tick", tick_outcomes, cancellable);
}
int rt_op_poll(int task) {
    struct task *t = &tasks[task];
    if (t->pending && t->cancellable && cancelled(task)) {
        t->pending = 0; // stopped: no completion event
        return CANCELLED;
    }
    if (t->pending) return PENDING;
    return t->op_outcome & 0xff;
}
const char *rt_op_payload(int task) { return tasks[task].op_payload; }

// Idle point: signal the oldest pending operation once. Like the reference, a
// signalled operation that is still pending parks again behind the others.
static int signal_oldest(void) {
    int best = -1;
    for (int t = 0; t < task_count; t++)
        if (tasks[t].pending == 1 && (best < 0 || tasks[t].op_seq < tasks[best].op_seq)) best = t;
    if (best < 0) return 0;
    struct task *t = &tasks[best];
    int waits = (t->op_outcome >> 8) - 1;
    t->op_outcome = (t->op_outcome & 0xff) | (waits << 8);
    if (waits == 0) {
        t->pending = 0;
        event(t->op_call->operation);
        push(best);
    } else {
        t->op_seq = op_clock++;
    }
    return 1;
}

// ---- synchronous primitives ------------------------------------------------------
static const char *const open_outcomes[] = {"opened", "not_found", "denied", NULL};
static const char *const close_outcomes[] = {"closed", "failed", NULL};
static const char *const decode_outcomes[] = {"text", "invalid", NULL};
static char text[128];

int rt_open(void) {
    const struct call *c = consume("open");
    event("open");
    int o = outcome_index(c, open_outcomes);
    if (o == 0) {
        if (resources == MAX_RESOURCES) fail("too many resources", NULL);
        return resources++;
    }
    return -1 - o;
}
int rt_close(int resource) {
    // The checker forbids consuming a borrowed owner; the runtime re-checks it.
    if (borrows[resource]) fail("loan violated: close while borrowed", NULL);
    const struct call *c = consume("close");
    event("close");
    return outcome_index(c, close_outcomes);
}
int rt_decode(const char *bytes) {
    (void)bytes;
    const struct call *c = consume("decode_utf8");
    event("decode_utf8");
    snprintf(text, sizeof text, "%s", c->payload);
    return outcome_index(c, decode_outcomes);
}
const char *rt_text(void) { return text; }
void rt_emit(const char *message) { event(message); }

// ---- driver -------------------------------------------------------------------
// Runs until the root task completes. Resuming a task resumes its top-level frame.
void rt_run(int root) {
    for (;;) {
        if (tasks[root].done) break;
        if (fault_task >= 0) {
            settle(root);
            break;
        }
        if (run_head == run_tail) {
            if (signal_oldest()) continue;
            fail("deadlock: no runnable task and no pending operation", NULL);
        }
        int task = runnable[run_head++];
        struct task *t = &tasks[task];
        if (t->done || !t->frame) continue;
        coro_resume(t->frame);
    }
}
void rt_init(const char *script) { load_script(script); }
int rt_finish(int status) {
    if (live_frames) fail("frames still allocated", NULL);
    if (fault_task >= 0) {
        // The launcher reports a fault on stderr with status 1.
        fprintf(stderr, "fault: %s\n", fault_message);
        fflush(stdout);
        return 1;
    }
    if (next_call != call_count) fail("unconsumed script entries", NULL);
    for (int t = 0; t < task_count; t++)
        if (tasks[t].owner >= 0 && !tasks[t].destroyed) fail("abandoned child frame", NULL);
    fflush(stdout);
    return status;
}

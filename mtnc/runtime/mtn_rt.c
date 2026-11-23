/* Mountain runtime (Phase 12 M3; Document 12 §11.1-11.3, §11.9; Document 17 §11.6).
 *
 * Linked only into programs that use `spawn` / `thread::spawnOS` / `pinToCore`.
 * Compiled by `mtnc` at link time with `cc -O2 -pthread -Wall -Wextra -Werror`.
 *
 * Model
 *  - A task is a closure `{code, env}` run by `thunk(code, env, out)` (generated per result type;
 *    it calls the closure, stores the result in `out` and drops the closure's environment, so the
 *    environment is dropped exactly once, on the thread that ran the task).
 *  - Workers: `MTN_WORKERS` threads (default: online CPUs, minimum 1), each with its own deque.
 *    A worker pops the BACK of its own deque and steals from the FRONT of the others'.
 *    All deques are guarded by one mutex (`g_mu`); idle workers sleep on a condition variable.
 *  - A `join` made by a worker does not block the worker: it runs other queued tasks until the
 *    awaited one is done ("helping join"), so nested `spawn` + `join` cannot starve the pool.
 *  - Worker and OS-thread stack size: 8 MiB (set explicitly, not left to the platform default).
 *  - At exit (`__mtn_rt_shutdown`) the runtime waits for every spawned task (including tasks
 *    spawned by tasks) and every OS thread, then joins and frees everything.
 *  - A panic anywhere exits the whole process with code 101 (done by `__mtn_panic`, not here).
 *
 * Futures (Phase 12 M4; Document 12 §3, §11.4)
 *  - An `async fn` call is a heap coroutine frame (LLVM switched-resume). `spawn f()` / `async fn main`
 *    make it a FUTURE task: a worker *polls* it (`poll` resumes the frame; the compiler-generated
 *    `await` loops resume sub-futures down the chain) until it is done.
 *  - A task whose poll returns "not done" is IDLE until something wakes it: `await` of a join handle
 *    (the target wakes its waiters when it finishes), `sleepMs` (timer thread), `yieldNow` (requeued
 *    at once behind the other ready tasks). A wake that arrives while the task is still being polled
 *    is remembered (NOTIFIED) and the task is requeued when the poll returns, so wakes are never lost.
 *    No worker ever blocks on an `await`; an idle worker sleeps on the condition variable.
 *  - One timer thread (started on the first timer) keeps a min-heap of (deadline, task).
 *
 * Actors (Phase 12 M5; Document 12 §8, §11.5)
 *  - An actor is a state block plus a FIFO mailbox (messages are copies of the compiler's message
 *    value, a malloc'd node each). `send` appends under `g_mu` (it never blocks on a handler) and, if the
 *    actor is not already scheduled, makes the actor's own embedded task runnable. A worker running
 *    that task takes messages one at a time from the front and calls the compiler-generated
 *    `dispatch(state, msg)` (which owns the message and runs the handler): an actor therefore never runs two
 *    handlers at once, and messages from one sender are handled in the order they were sent.
 *  - After ACTOR_BATCH messages an actor requeues itself behind the other ready tasks (fairness).
 *  - A scheduled actor counts in `g_pending`, so `__mtn_rt_shutdown` (called after `main` returns) waits
 *    until every mailbox is empty and no handler is running; only then are the actors freed: the state's
 *    drop glue runs once, then the state block and the actor are released.
 *  - Stats line: every message is counted as one `spawned`/`completed` task (the line is unchanged).
 */
/* Locks (Phase 13 M2; Document 14 §6, §7, §9.1, §9.5; Document 12 §7)
 *  - `Spinlock`: one flag word, test-and-test-and-set with a CPU relax hint (a CAS retry loop, no parking;
 *    `sched_yield` once in a long while so a descheduled holder on a busy machine can run).
 *  - `Mutex` / `RwLock`: a TICKET lock (a monotonic ticket counter, served in strict arrival order, so a waiter
 *    is never overtaken and nobody starves). A waiter spins a bounded number of rounds, then PARKS on a global
 *    parking lot: 64 buckets, each a pthread mutex + condition variable, chosen by the lock's address. A lock
 *    itself is only 32 bytes of zero-initialised atomics (a `static` lock needs no run-time set-up).
 *  - `RwLock` = the ticket lock as a GATE + a reader count. A reader passes through the gate (in ticket order),
 *    counts itself in and leaves the gate; a writer holds the gate and waits for the count to reach 0. Readers
 *    that arrive while a writer waits queue behind it at the gate, so a stream of readers cannot starve a writer,
 *    and a stream of writers cannot starve a reader.
 *  - A thread that holds a lock never runs OTHER queued tasks from a helping `join` (see __mtn_rt_join): such a
 *    task could block on that very lock, below the holder on the same stack, forever.
 *  - Not re-entrant: locking a lock the thread already holds deadlocks (Document 14 §9.5).
 *  - A waiter blocks its thread (a pool worker included); critical sections should be short (Doc 14 §7).
 */
#define _GNU_SOURCE
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define STACK_BYTES ((size_t)8 * 1024 * 1024) /* 8 MiB per worker / OS thread */

typedef void (*thunk_fn)(void *code, void *env, void *out);
typedef void (*drop_fn)(void *slot);
typedef int (*poll_fn)(void *hdl);                  /* resumes the frame; returns 1 when it is done */
typedef void (*take_fn)(void *hdl, void *out);      /* moves the frame's result out */
typedef void (*destroy_fn)(void *hdl);              /* frees the (finished) frame */

enum { K_SYNC = 0, K_FUTURE = 1, K_OS = 2, K_ACTOR = 3 };
#define ACTOR_BATCH 64
enum { S_QUEUED = 0, S_RUNNING = 1, S_NOTIFIED = 2, S_IDLE = 3 };

typedef struct Task {
    int kind;
    thunk_fn thunk;   /* K_SYNC / K_OS */
    void *code, *env;
    void *hdl;        /* K_FUTURE: the coroutine frame */
    poll_fn poll;
    take_fn take;
    destroy_fn destroy;
    void *res;        /* result slot (res_size bytes) */
    size_t res_size;
    drop_fn res_drop; /* drops an untaken result (NULL if nothing to drop) */
    int done;
    int handle_alive; /* the JoinHandle still exists and has not joined */
    int is_os;
    int counted;      /* counted in the stats line (spawned tasks, not the async `main`) */
    int state;        /* K_FUTURE scheduling state */
    int yielded;      /* requeue behind the other ready tasks after this poll */
    struct Task *waiters;     /* tasks suspended in `await` of this one */
    struct Task *wait_next;   /* link in the target's waiter list */
    struct Task *waiting_on;
    struct Chan *chan_wait;   /* channel whose wait list holds this task (async send/recv), or NULL */
    struct Task *chan_next;   /* link in that wait list */
    pthread_t thread; /* is_os only */
    struct Task *next, *prev; /* deque links */
    struct Task *os_next;     /* list of OS tasks */
} Task;

typedef struct {
    uint64_t when;
    Task *t;
} TimerEnt;

typedef struct {
    Task *head, *tail; /* head = front (stolen from), tail = back (owner pops) */
} Deque;

static pthread_mutex_t g_mu = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t g_cv = PTHREAD_COND_INITIALIZER;
static Deque *g_dq;
static pthread_t *g_threads;
static int g_nworkers;
static int g_queued;     /* tasks sitting in deques */
static long g_pending;   /* tasks not yet finished (queued, running or waiting) */
static long g_os_live;   /* OS threads not yet finished */
static int g_shutdown;
static int g_rr;         /* round-robin target for pushes from non-worker threads */
static Task *g_os_list;
static unsigned long g_spawned, g_completed, g_live, g_peak;

/* timers (one thread, started on first use) */
static pthread_cond_t g_timer_cv;
static pthread_t g_timer_thread;
static int g_timer_started, g_timer_stop;
static TimerEnt *g_heap;
static size_t g_nheap, g_capheap;

typedef void (*dispatch_fn)(void *state, void *msg);

typedef struct Msg {
    struct Msg *next;
    /* the payload (msg_size bytes) starts 16 bytes after the node */
} Msg;

typedef struct Actor {
    Task task; /* first member: the actor is scheduled through its own task */
    void *state;
    dispatch_fn dispatch;
    drop_fn drop_state;
    size_t msg_size;
    Msg *head, *tail;
    int scheduled;
    struct Actor *all_next;
} Actor;

static Actor *g_actors;

static __thread int t_worker = -1;
static __thread Task *t_cur; /* the future task being polled on this thread */
static __thread int t_locks_held; /* locks (guards) this thread holds right now: see the lock section */
static __thread int t_is_pool;    /* this thread is a pool worker (original or compensating): see the channel section */
static int g_blocked;             /* pool workers currently blocked in a channel wait */
static int g_extra;               /* compensating workers started so far (never exit before shutdown) */
static pthread_t *g_xthreads;
static int g_capx;
#define MAX_EXTRA 2048

static void rt_die(const char *msg) {
    (void)!write(2, msg, strlen(msg));
    _exit(101);
}

static uint64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static void dq_push_back(Deque *d, Task *t) {
    t->next = NULL;
    t->prev = d->tail;
    if (d->tail) d->tail->next = t; else d->head = t;
    d->tail = t;
}
static void dq_push_front(Deque *d, Task *t) {
    t->prev = NULL;
    t->next = d->head;
    if (d->head) d->head->prev = t; else d->tail = t;
    d->head = t;
}
static Task *dq_pop_back(Deque *d) {
    Task *t = d->tail;
    if (!t) return NULL;
    d->tail = t->prev;
    if (d->tail) d->tail->next = NULL; else d->head = NULL;
    return t;
}
static Task *dq_pop_front(Deque *d) {
    Task *t = d->head;
    if (!t) return NULL;
    d->head = t->next;
    if (d->head) d->head->prev = NULL; else d->tail = NULL;
    return t;
}

/* Queues a ready task (caller holds g_mu): own deque when called from a worker, else round-robin. */
static void enqueue(Task *t, int front) {
    int target = t_worker >= 0 ? t_worker : (g_rr++ % g_nworkers);
    t->state = S_QUEUED;
    if (front) dq_push_front(&g_dq[target], t); else dq_push_back(&g_dq[target], t);
    g_queued++;
    pthread_cond_broadcast(&g_cv);
}

/* Takes one queued task (own deque first, then steal). Caller holds g_mu. */
static Task *take_task(int me) {
    if (g_queued == 0) return NULL;
    Task *t = NULL;
    if (me >= 0) t = dq_pop_back(&g_dq[me]);
    for (int i = 0; !t && i < g_nworkers; i++) t = dq_pop_front(&g_dq[(me < 0 ? 0 : me + 1 + i) % g_nworkers]);
    if (t) {
        g_queued--;
        t->state = S_RUNNING;
    }
    return t;
}

/* Takes exactly `t` out of whichever deque holds it, if it is still queued. Caller holds g_mu. */
static Task *take_specific(Task *t) {
    if (t->done || t->state != S_QUEUED || g_queued == 0) return NULL;
    for (int i = 0; i < g_nworkers; i++) {
        Deque *d = &g_dq[i];
        int in = 0;
        for (Task *x = d->head; x; x = x->next) if (x == t) { in = 1; break; }
        if (!in) continue;
        if (t->prev) t->prev->next = t->next; else d->head = t->next;
        if (t->next) t->next->prev = t->prev; else d->tail = t->prev;
        t->next = t->prev = NULL;
        g_queued--;
        t->state = S_RUNNING;
        return t;
    }
    return NULL;
}

static void task_free(Task *t) {
    free(t->res);
    free(t);
}

/* A suspended task becomes runnable (caller holds g_mu). A wake that arrives while the task is
   still being polled is remembered and acted on when the poll returns. */
static void wake(Task *t) {
    if (t->done) return;
    if (t->state == S_RUNNING) t->state = S_NOTIFIED;
    else if (t->state == S_IDLE) enqueue(t, 0);
}

/* Publishes the result of `t` (caller holds g_mu); returns 1 if nobody will take the result. */
static int finish_locked(Task *t) {
    t->done = 1;
    if (t->counted) {
        g_completed++;
        g_live--;
    }
    if (t->kind != K_OS) g_pending--; /* OS threads are tracked by g_os_live */
    for (Task *w = t->waiters; w;) {
        Task *n = w->wait_next;
        w->waiting_on = NULL;
        w->wait_next = NULL;
        wake(w);
        w = n;
    }
    t->waiters = NULL;
    pthread_cond_broadcast(&g_cv);
    return !t->handle_alive;
}

/* Runs / polls `t` once (not holding g_mu). */
static void run_actor(Task *t) {
    Actor *a = (Actor *)t;
    for (int n = 0; n < ACTOR_BATCH; n++) {
        pthread_mutex_lock(&g_mu);
        Msg *m = a->head;
        if (!m) { /* mailbox empty and no handler running: the actor goes idle */
            a->scheduled = 0;
            g_pending--;
            pthread_cond_broadcast(&g_cv);
            pthread_mutex_unlock(&g_mu);
            return;
        }
        a->head = m->next;
        if (!a->head) a->tail = NULL;
        pthread_mutex_unlock(&g_mu);
        Task *saved = t_cur;
        t_cur = NULL;
        a->dispatch(a->state, (char *)m + 16); /* the handler owns the message */
        t_cur = saved;
        free(m);
        pthread_mutex_lock(&g_mu);
        g_completed++;
        g_live--;
        pthread_mutex_unlock(&g_mu);
    }
    pthread_mutex_lock(&g_mu);
    if (a->head) {
        enqueue(t, 0); /* more messages: behind the other ready tasks */
    } else {
        a->scheduled = 0;
        g_pending--;
        pthread_cond_broadcast(&g_cv);
    }
    pthread_mutex_unlock(&g_mu);
}

static void run_task(Task *t) {
    Task *saved = t_cur;
    if (t->kind == K_ACTOR) {
        run_actor(t);
        return;
    }
    if (t->kind == K_SYNC) {
        t_cur = NULL;
        t->thunk(t->code, t->env, t->res);
        t_cur = saved;
    } else {
        t_cur = t;
        int done = t->poll(t->hdl);
        t_cur = saved;
        if (!done) {
            pthread_mutex_lock(&g_mu);
            if (t->state == S_NOTIFIED) {
                int y = t->yielded;
                t->yielded = 0;
                enqueue(t, y); /* a yielding task goes behind the other ready tasks */
            } else {
                t->state = S_IDLE;
            }
            pthread_mutex_unlock(&g_mu);
            return;
        }
        if (t->res_size) t->take(t->hdl, t->res);
        t->destroy(t->hdl);
        t->hdl = NULL;
    }
    pthread_mutex_lock(&g_mu);
    int orphan = finish_locked(t);
    pthread_mutex_unlock(&g_mu);
    if (orphan) { /* the handle was dropped without joining: nobody will take the result */
        if (t->res_drop) t->res_drop(t->res);
        task_free(t);
    }
}

static void *worker_main(void *arg) {
    t_worker = (int)(intptr_t)arg;
    t_is_pool = 1;
    for (;;) {
        pthread_mutex_lock(&g_mu);
        Task *t;
        while (!(t = take_task(t_worker)) && !g_shutdown) pthread_cond_wait(&g_cv, &g_mu);
        pthread_mutex_unlock(&g_mu);
        if (!t) return NULL;
        run_task(t);
    }
}

static int online_cpus(void) {
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    return n < 1 ? 1 : (int)n;
}

static void spawn_thread(pthread_t *th, void *(*fn)(void *), void *arg) {
    pthread_attr_t at;
    pthread_attr_init(&at);
    pthread_attr_setstacksize(&at, STACK_BYTES);
    int rc = pthread_create(th, &at, fn, arg);
    pthread_attr_destroy(&at);
    if (rc != 0) rt_die("runtime: could not create a thread\n");
}

void __mtn_rt_init(void) {
    int n = online_cpus();
    const char *e = getenv("MTN_WORKERS");
    if (e && *e) {
        long v = strtol(e, NULL, 10);
        if (v >= 1 && v <= 4096) n = (int)v;
    }
    g_nworkers = n;
    g_dq = calloc((size_t)n, sizeof(Deque));
    g_threads = calloc((size_t)n, sizeof(pthread_t));
    if (!g_dq || !g_threads) rt_die("runtime: out of memory\n");
    pthread_condattr_t ca;
    pthread_condattr_init(&ca);
    pthread_condattr_setclock(&ca, CLOCK_MONOTONIC);
    pthread_cond_init(&g_timer_cv, &ca);
    pthread_condattr_destroy(&ca);
    for (int i = 0; i < n; i++) spawn_thread(&g_threads[i], worker_main, (void *)(intptr_t)i);
}

static Task *task_new(int kind, size_t res_size, drop_fn res_drop) {
    Task *t = calloc(1, sizeof(Task));
    if (!t) rt_die("runtime: out of memory\n");
    t->res = calloc(1, res_size ? res_size : 1);
    if (!t->res) rt_die("runtime: out of memory\n");
    t->kind = kind;
    t->res_size = res_size;
    t->res_drop = res_drop;
    t->handle_alive = 1;
    return t;
}

static void register_task(Task *t) {
    pthread_mutex_lock(&g_mu);
    if (t->counted) {
        g_spawned++;
        if (++g_live > g_peak) g_peak = g_live;
    }
    g_pending++;
    enqueue(t, 0);
    pthread_mutex_unlock(&g_mu);
}

void *__mtn_rt_spawn(thunk_fn thunk, void *code, void *env, size_t res_size, drop_fn res_drop) {
    Task *t = task_new(K_SYNC, res_size, res_drop);
    t->thunk = thunk;
    t->code = code;
    t->env = env;
    t->counted = 1;
    register_task(t);
    return t;
}

static Task *spawn_future_impl(void *hdl, poll_fn poll, take_fn take, destroy_fn destroy, size_t res_size, drop_fn res_drop, int counted) {
    Task *t = task_new(K_FUTURE, res_size, res_drop);
    t->hdl = hdl;
    t->poll = poll;
    t->take = take;
    t->destroy = destroy;
    t->counted = counted;
    register_task(t);
    return t;
}

/* `spawn f()` where `f` is an `async fn`: the frame becomes a task. */
void *__mtn_rt_spawn_future(void *hdl, poll_fn poll, take_fn take, destroy_fn destroy, size_t res_size, drop_fn res_drop) {
    return spawn_future_impl(hdl, poll, take, destroy, res_size, res_drop, 1);
}

static void *os_main(void *arg) {
    Task *t = arg;
    t->thunk(t->code, t->env, t->res);
    pthread_mutex_lock(&g_mu);
    g_os_live--;
    int orphan = finish_locked(t);
    pthread_mutex_unlock(&g_mu);
    if (orphan && t->res_drop) t->res_drop(t->res);
    return NULL;
}

void *__mtn_rt_spawn_os(thunk_fn thunk, void *code, void *env, size_t res_size, drop_fn res_drop) {
    Task *t = task_new(K_OS, res_size, res_drop);
    t->thunk = thunk;
    t->code = code;
    t->env = env;
    t->is_os = 1;
    pthread_mutex_lock(&g_mu);
    g_os_live++;
    t->os_next = g_os_list;
    g_os_list = t;
    spawn_thread(&t->thread, os_main, t);
    pthread_mutex_unlock(&g_mu);
    return t;
}

/* `A::spawn()`: a new actor with a zeroed state block of `state_size` bytes (the caller stores the
   initial state into `__mtn_rt_actor_state(a)`). The returned pointer is the `ActorRef`. */
void *__mtn_rt_actor_new(dispatch_fn dispatch, drop_fn drop_state, size_t state_size, size_t msg_size) {
    Actor *a = calloc(1, sizeof(Actor));
    if (!a) rt_die("runtime: out of memory\n");
    a->state = calloc(1, state_size ? state_size : 1);
    if (!a->state) rt_die("runtime: out of memory\n");
    a->dispatch = dispatch;
    a->drop_state = drop_state;
    a->msg_size = msg_size;
    a->task.kind = K_ACTOR;
    pthread_mutex_lock(&g_mu);
    a->all_next = g_actors;
    g_actors = a;
    pthread_mutex_unlock(&g_mu);
    return a;
}

void *__mtn_rt_actor_state(void *actor) {
    return ((Actor *)actor)->state;
}

/* `r.send(msg)`: copies the message into the mailbox and schedules the actor; never blocks. */
void __mtn_rt_actor_send(void *actor, void *msg) {
    Actor *a = actor;
    Msg *m = malloc(16 + a->msg_size);
    if (!m) rt_die("runtime: out of memory\n");
    m->next = NULL;
    memcpy((char *)m + 16, msg, a->msg_size);
    pthread_mutex_lock(&g_mu);
    if (a->tail) a->tail->next = m; else a->head = m;
    a->tail = m;
    g_spawned++;
    if (++g_live > g_peak) g_peak = g_live;
    if (!a->scheduled) {
        a->scheduled = 1;
        g_pending++;
        enqueue(&a->task, 0);
    }
    pthread_mutex_unlock(&g_mu);
}

/* `h.join()`: waits for the task, moves its result into `out`, releases the handle. */
void __mtn_rt_join(void *h, void *out) {
    Task *t = h;
    pthread_mutex_lock(&g_mu);
    while (!t->done) {
        /* A worker that holds a lock may only run the awaited task itself: any other task could block on
           that lock and never return to the frame that holds it. */
        Task *other = t_worker < 0 ? NULL : (t_locks_held == 0 ? take_task(t_worker) : take_specific(t));
        if (other) {
            pthread_mutex_unlock(&g_mu);
            run_task(other);
            pthread_mutex_lock(&g_mu);
        } else {
            pthread_cond_wait(&g_cv, &g_mu);
        }
    }
    t->handle_alive = 0;
    pthread_mutex_unlock(&g_mu);
    if (t->res_size) memcpy(out, t->res, t->res_size);
    if (!t->is_os) task_free(t); /* OS tasks are freed at shutdown (their thread is joined there) */
}

/* `await h` inside an async frame: 1 = done (result moved to `out`, task released);
   0 = not done: the current task is registered as a waiter and the frame must suspend. */
int __mtn_rt_await_join(void *h, void *out) {
    Task *t = h;
    pthread_mutex_lock(&g_mu);
    if (t->done) {
        t->handle_alive = 0;
        pthread_mutex_unlock(&g_mu);
        if (t->res_size) memcpy(out, t->res, t->res_size);
        if (!t->is_os) task_free(t);
        return 1;
    }
    Task *me = t_cur;
    if (!me) rt_die("runtime: `await` outside a task\n");
    if (me->waiting_on != t) {
        me->waiting_on = t;
        me->wait_next = t->waiters;
        t->waiters = me;
    }
    pthread_mutex_unlock(&g_mu);
    return 0;
}

/* `yieldNow()`: ask to be requeued behind the other ready tasks once the current poll returns. */
void __mtn_rt_yield(void) {
    Task *me = t_cur;
    if (!me) rt_die("runtime: `yieldNow` outside a task\n");
    pthread_mutex_lock(&g_mu);
    me->yielded = 1;
    me->state = S_NOTIFIED;
    pthread_mutex_unlock(&g_mu);
}

uint64_t __mtn_rt_deadline_ns(uint64_t ms) {
    uint64_t now = now_ns();
    uint64_t add = ms > (UINT64_MAX - now) / 1000000ull ? UINT64_MAX - now : ms * 1000000ull;
    return now + add;
}

/* min-heap of timers (caller holds g_mu) */
static void heap_push(uint64_t when, Task *t) {
    if (g_nheap == g_capheap) {
        g_capheap = g_capheap ? g_capheap * 2 : 64;
        g_heap = realloc(g_heap, g_capheap * sizeof(TimerEnt));
        if (!g_heap) rt_die("runtime: out of memory\n");
    }
    size_t i = g_nheap++;
    while (i > 0 && g_heap[(i - 1) / 2].when > when) {
        g_heap[i] = g_heap[(i - 1) / 2];
        i = (i - 1) / 2;
    }
    g_heap[i].when = when;
    g_heap[i].t = t;
}
static TimerEnt heap_pop(void) {
    TimerEnt top = g_heap[0];
    TimerEnt last = g_heap[--g_nheap];
    size_t i = 0;
    for (;;) {
        size_t c = 2 * i + 1;
        if (c >= g_nheap) break;
        if (c + 1 < g_nheap && g_heap[c + 1].when < g_heap[c].when) c++;
        if (g_heap[c].when >= last.when) break;
        g_heap[i] = g_heap[c];
        i = c;
    }
    if (g_nheap) g_heap[i] = last;
    return top;
}

static void *timer_main(void *arg) {
    (void)arg;
    pthread_mutex_lock(&g_mu);
    while (!g_timer_stop) {
        if (g_nheap == 0) {
            pthread_cond_wait(&g_timer_cv, &g_mu);
            continue;
        }
        uint64_t now = now_ns();
        if (g_heap[0].when <= now) {
            TimerEnt e = heap_pop();
            wake(e.t);
            continue;
        }
        uint64_t when = g_heap[0].when;
        struct timespec ts;
        ts.tv_sec = (time_t)(when / 1000000000ull);
        ts.tv_nsec = (long)(when % 1000000000ull);
        pthread_cond_timedwait(&g_timer_cv, &g_mu, &ts);
    }
    pthread_mutex_unlock(&g_mu);
    return NULL;
}

/* `sleepMs`: 1 once `deadline` has passed; otherwise registers a timer for the current task (once,
   `*registered`) and returns 0 so the frame suspends. The worker is not blocked. */
int __mtn_rt_sleep_wait(uint64_t deadline, unsigned char *registered) {
    if (now_ns() >= deadline) return 1;
    if (!*registered) {
        Task *me = t_cur;
        if (!me) rt_die("runtime: `sleepMs` outside a task\n");
        *registered = 1;
        pthread_mutex_lock(&g_mu);
        heap_push(deadline, me);
        if (!g_timer_started) {
            g_timer_started = 1;
            spawn_thread(&g_timer_thread, timer_main, NULL);
        } else {
            pthread_cond_signal(&g_timer_cv);
        }
        pthread_mutex_unlock(&g_mu);
    }
    return 0;
}

/* A dropped, unjoined handle detaches: the task still runs and the runtime waits for it at exit. */
void __mtn_rt_handle_drop(void *h) {
    Task *t = h;
    if (!t) return;
    pthread_mutex_lock(&g_mu);
    t->handle_alive = 0;
    int done = t->done;
    pthread_mutex_unlock(&g_mu);
    if (done && !t->is_os) { /* finished before the drop: nobody else will free it */
        if (t->res_drop) t->res_drop(t->res);
        task_free(t);
    }
    /* not done: run_task / os_main see the cleared flag and release the result themselves;
       OS tasks are freed at shutdown */
}

/* `async fn main()`: the executor runs the frame as a task (not counted in the stats), the main
   thread waits for it and takes its result. */
void __mtn_rt_block_on(void *hdl, poll_fn poll, take_fn take, destroy_fn destroy, size_t res_size, drop_fn res_drop, void *out) {
    Task *t = spawn_future_impl(hdl, poll, take, destroy, res_size, res_drop, 0);
    __mtn_rt_join(t, out);
}

void __mtn_rt_pin_to_core(size_t n) {
#ifdef __linux__
    cpu_set_t set;
    if (n >= CPU_SETSIZE) return;
    CPU_ZERO(&set);
    CPU_SET(n, &set);
    (void)sched_setaffinity(0, sizeof(set), &set); /* no-op on failure */
#else
    (void)n;
#endif
}

/* Called by the program's `main` wrapper after `main` returned. */
void __mtn_rt_shutdown(void) {
    pthread_mutex_lock(&g_mu);
    while (g_pending > 0 || g_os_live > 0) pthread_cond_wait(&g_cv, &g_mu);
    g_shutdown = 1;
    g_timer_stop = 1;
    pthread_cond_broadcast(&g_cv);
    pthread_cond_broadcast(&g_timer_cv);
    Task *os = g_os_list;
    g_os_list = NULL;
    int timer = g_timer_started;
    pthread_mutex_unlock(&g_mu);
    for (int i = 0; i < g_nworkers; i++) pthread_join(g_threads[i], NULL);
    for (int i = 0; i < g_extra; i++) pthread_join(g_xthreads[i], NULL); /* compensating workers (channel section) */
    free(g_xthreads);
    if (timer) pthread_join(g_timer_thread, NULL);
    while (os) {
        Task *n = os->os_next;
        pthread_join(os->thread, NULL);
        task_free(os);
        os = n;
    }
    /* every mailbox is empty and no handler runs: drop each actor's state once, then free it */
    while (g_actors) {
        Actor *a = g_actors;
        g_actors = a->all_next;
        if (a->drop_state) a->drop_state(a->state);
        free(a->state);
        free(a);
    }
    const char *st = getenv("MTN_RT_STATS");
    if (st && st[0] == '1' && st[1] == 0)
        fprintf(stderr, "rt: workers=%d spawned=%lu completed=%lu peak_live=%lu\n", g_nworkers, g_spawned, g_completed, g_peak);
    pthread_cond_destroy(&g_timer_cv);
    free(g_heap);
    free(g_dq);
    free(g_threads);
}


/* ================================================================================================
 * Locks (Phase 13 M2).  See the header comment.  Exported names are `__mtn_lock_*`.
 * Memory orders: every choice is explained where it is made.
 * ================================================================================================ */

/* 32 bytes, 4-byte aligned, all zero = unlocked; the compiler lays a lock out as `[8 x i32]`. */
typedef struct {
    _Atomic uint32_t next;     /* next ticket to hand out */
    _Atomic uint32_t serving;  /* the ticket that owns the lock now */
    _Atomic uint32_t parked;   /* waiters asleep (or about to sleep) on `serving` */
    _Atomic uint32_t readers;  /* RwLock: readers inside */
    _Atomic uint32_t wparked;  /* RwLock: a writer asleep (or about to sleep) waiting for `readers == 0` */
    uint32_t pad[3];
} mtn_lk;
_Static_assert(sizeof(mtn_lk) == 32, "the compiler lays a lock out as [8 x i32]");

#define LK_SPIN 300 /* rounds of spinning before a waiter parks */
#define PL_BUCKETS 64

typedef struct {
    pthread_mutex_t mu;
    pthread_cond_t cv;
} PlBucket;
static PlBucket g_pl[PL_BUCKETS] = {[0 ... PL_BUCKETS - 1] = {PTHREAD_MUTEX_INITIALIZER, PTHREAD_COND_INITIALIZER}};

static PlBucket *pl_bucket(const void *addr) {
    return &g_pl[(((uintptr_t)addr >> 5) * 0x9E3779B97F4A7C15ull) >> 58]; /* top 6 bits of a multiplicative hash */
}

static inline void cpu_relax(void) {
#if defined(__x86_64__) || defined(__i386__)
    __builtin_ia32_pause();
#elif defined(__aarch64__) || defined(__arm__)
    __asm__ __volatile__("yield");
#endif
}

/* Wakes everything parked in the bucket of `addr`. The caller has ALREADY changed the state the waiters
   test; taking the bucket mutex here orders this broadcast after any waiter that is between "I tested the
   state" and "I am in cond_wait" (it holds the mutex for that whole span), so a wakeup cannot be lost. */
static void pl_wake(const void *addr) {
    PlBucket *b = pl_bucket(addr);
    pthread_mutex_lock(&b->mu);
    pthread_cond_broadcast(&b->cv);
    pthread_mutex_unlock(&b->mu);
}

/* ---- ticket gate (used by Mutex directly and by RwLock as its entry gate) ---- */

static void gate_lock(mtn_lk *l) {
    /* relaxed: the ticket counter only has to hand out distinct numbers; the data of the critical section is
       ordered by the acquire load of `serving` below, which pairs with the release store in gate_unlock */
    uint32_t t = atomic_fetch_add_explicit(&l->next, 1, memory_order_relaxed);
    for (int i = 0; i < LK_SPIN; i++) {
        /* acquire: if it is our turn, everything the previous holder wrote before its release is visible */
        if (atomic_load_explicit(&l->serving, memory_order_acquire) == t) return;
        cpu_relax();
    }
    PlBucket *b = pl_bucket(l);
    pthread_mutex_lock(&b->mu);
    /* seq_cst on `parked` (RMW) and on the `serving` loads/stores: this is a Dekker-style handshake with
       gate_unlock (store `serving`, then read `parked`). In the single total order of seq_cst operations either
       the waiter sees the new `serving` (it does not sleep) or the unlocker sees `parked > 0` (it wakes us). */
    atomic_fetch_add_explicit(&l->parked, 1, memory_order_seq_cst);
    while (atomic_load_explicit(&l->serving, memory_order_seq_cst) != t) pthread_cond_wait(&b->cv, &b->mu);
    atomic_fetch_sub_explicit(&l->parked, 1, memory_order_relaxed); /* under the bucket mutex; only a hint for the unlocker */
    pthread_mutex_unlock(&b->mu);
}

static int gate_try(mtn_lk *l) {
    /* acquire: pairs with the release (seq_cst) store of `serving` made by the last unlock */
    uint32_t s = atomic_load_explicit(&l->serving, memory_order_acquire);
    uint32_t exp = s;
    /* `next == serving` means nobody holds the lock and nobody waits; taking ticket `s` makes us the holder.
       Success needs no ordering of its own (the acquire above did it); failure changes nothing. */
    return atomic_compare_exchange_strong_explicit(&l->next, &exp, s + 1, memory_order_relaxed, memory_order_relaxed);
}

static void gate_unlock(mtn_lk *l) {
    /* only the holder writes `serving`, so a relaxed read of our own last value is exact */
    uint32_t s = atomic_load_explicit(&l->serving, memory_order_relaxed);
    /* seq_cst store: release for the critical section's writes, and the first half of the Dekker handshake */
    atomic_store_explicit(&l->serving, s + 1, memory_order_seq_cst);
    if (atomic_load_explicit(&l->parked, memory_order_seq_cst) != 0) pl_wake(l);
}

void __mtn_lock_mutex_lock(void *p) {
    gate_lock(p);
    t_locks_held++;
}
int __mtn_lock_mutex_trylock(void *p) {
    if (!gate_try(p)) return 0;
    t_locks_held++;
    return 1;
}
void __mtn_lock_mutex_unlock(void *p) {
    t_locks_held--;
    gate_unlock(p);
}

/* ---- RwLock ---- */

void __mtn_lock_rw_write(void *p) {
    mtn_lk *l = p;
    gate_lock(l); /* we are first in line: no new reader can enter while we wait for the old ones */
    for (int i = 0; i < LK_SPIN; i++) {
        /* acquire: the readers' critical sections (their release in rw_read_unlock) happen before our writes */
        if (atomic_load_explicit(&l->readers, memory_order_acquire) == 0) goto held;
        cpu_relax();
    }
    {
        PlBucket *b = pl_bucket(&l->readers);
        pthread_mutex_lock(&b->mu);
        /* Dekker with rw_read_unlock (fetch_sub `readers`, then read `wparked`): all four operations seq_cst */
        atomic_fetch_add_explicit(&l->wparked, 1, memory_order_seq_cst);
        while (atomic_load_explicit(&l->readers, memory_order_seq_cst) != 0) pthread_cond_wait(&b->cv, &b->mu);
        atomic_fetch_sub_explicit(&l->wparked, 1, memory_order_relaxed);
        pthread_mutex_unlock(&b->mu);
    }
held:
    t_locks_held++;
}
int __mtn_lock_rw_try_write(void *p) {
    mtn_lk *l = p;
    if (!gate_try(l)) return 0;
    if (atomic_load_explicit(&l->readers, memory_order_acquire) != 0) { /* readers inside: give the gate back */
        gate_unlock(l);
        return 0;
    }
    t_locks_held++;
    return 1;
}
void __mtn_lock_rw_write_unlock(void *p) {
    t_locks_held--;
    gate_unlock(p);
}

void __mtn_lock_rw_read(void *p) {
    mtn_lk *l = p;
    gate_lock(l); /* in ticket order behind any waiting writer */
    /* acquire: the previous writer's writes (released by its gate_unlock) are visible to this reader */
    atomic_fetch_add_explicit(&l->readers, 1, memory_order_acquire);
    gate_unlock(l);
    t_locks_held++;
}
int __mtn_lock_rw_try_read(void *p) {
    mtn_lk *l = p;
    if (!gate_try(l)) return 0; /* a writer holds or awaits the gate: do not overtake it */
    atomic_fetch_add_explicit(&l->readers, 1, memory_order_acquire);
    gate_unlock(l);
    t_locks_held++;
    return 1;
}
void __mtn_lock_rw_read_unlock(void *p) {
    mtn_lk *l = p;
    t_locks_held--;
    /* seq_cst: release for the reader's accesses (a writer's acquire load of `readers` pairs with it) and the
       first half of the Dekker handshake with the parking writer */
    uint32_t old = atomic_fetch_sub_explicit(&l->readers, 1, memory_order_seq_cst);
    if (old == 1 && atomic_load_explicit(&l->wparked, memory_order_seq_cst) != 0) pl_wake(&l->readers);
}

/* ---- Spinlock ---- */

void __mtn_lock_spin_acquire(void *p) {
    _Atomic uint32_t *f = p;
    for (unsigned long i = 1;; i++) {
        /* test-and-test-and-set: spin on a plain load (no cache-line ping-pong), then try the exchange;
           acquire on the exchange pairs with the release store in spin_release */
        if (atomic_load_explicit(f, memory_order_relaxed) == 0 && atomic_exchange_explicit(f, 1, memory_order_acquire) == 0) break;
        cpu_relax();
        if ((i & 0xFFFFul) == 0) sched_yield(); /* still no parking: just let a descheduled holder run */
    }
    t_locks_held++;
}
int __mtn_lock_spin_try(void *p) {
    _Atomic uint32_t *f = p;
    if (atomic_exchange_explicit(f, 1, memory_order_acquire) != 0) return 0;
    t_locks_held++;
    return 1;
}
void __mtn_lock_spin_release(void *p) {
    t_locks_held--;
    /* release: publishes the critical section to the next acquirer */
    atomic_store_explicit((_Atomic uint32_t *)p, 0, memory_order_release);
}


/* ================================================================================================
 * Channels (Phase 13 M3; Document 12 §6, §11.13; Document 14 §3, §9.6).  Exported names: `__mtn_chan_*`.
 *
 *  - Bounded channel  = Vyukov bounded MPMC ring buffer (a sequence number per slot, positions claimed by CAS).
 *    Capacity is EXACTLY `cap` slots (any `cap >= 1`; slots are found with `pos % cap`, not a mask).
 *  - Unbounded channel = Michael-Scott queue (1996), including the "help the lagging tail" step.  Nodes are
 *    reclaimed with HAZARD POINTERS (Michael 2004), the "equivalent hazard-pointer scheme" of Document 14 §5:
 *    a node is freed only when no thread has published it, so a recycled address can never be mistaken for the
 *    old node (no ABA).  Nodes are plain malloc'd blocks and are never reused while published.
 *  - A message is a byte copy of the compiler's value (the sender gives up ownership when `send` succeeds).
 *    Values still queued when the last handle goes away are dropped once, with the channel's drop glue.
 *  - Handles are counted: `senders`, `receivers` (closing) and `refs` (memory: one per handle).
 *    `send` fails when `receivers == 0`; `recv` reports "closed" only when the queue is empty AND `senders == 0`.
 *  - Waiting.  A blocked thread SPINS briefly, then registers as a waiter and sleeps on the channel's condition
 *    variable.  An async task registers itself in the channel's wait list and suspends (its worker is free).
 *    Wake-ups are lost-wakeup free: see `ch_notify_*` (a store-load handshake, all four accesses seq_cst).
 *  - A POOL WORKER THAT BLOCKS IN A SYNCHRONOUS send/recv IS COMPENSATED: the pool starts one extra worker
 *    thread so that at least `MTN_WORKERS` workers can still run tasks (see comp_enter).  "Helping" (running
 *    another queued task on top of the blocked frame, as `join` does) is NOT used for channels: the helped task
 *    could be the very consumer (or producer) the blocked frame is waiting for and, sitting above it on the same
 *    stack, could never let it continue (stack inversion deadlock; PROGRESS register item 51).
 *  - Memory orders: every non-relaxed or relaxed choice is explained where it is made.
 * ================================================================================================ */

#define CH_SPIN 100
#define HP_PER 2
#define HP_SCAN 64

/* ---- hazard pointers --------------------------------------------------------------------------- */

typedef struct HpRec {
    _Atomic(void *) hp[HP_PER];
    struct HpRec *next;      /* registry link: set once before the record is published, then read-only */
    atomic_int active;       /* 1 while a live thread owns the record */
    void **retired;          /* nodes this record's owner has unlinked, not yet freed */
    size_t nret, capret;
} HpRec;

static _Atomic(HpRec *) g_hp_head;
static __thread HpRec *t_hp;
static pthread_key_t g_hp_key;
static pthread_once_t g_hp_once = PTHREAD_ONCE_INIT;

static void hp_thread_exit(void *p) {
    HpRec *r = p;
    for (int i = 0; i < HP_PER; i++) atomic_store_explicit(&r->hp[i], NULL, memory_order_release); /* unprotect */
    atomic_store_explicit(&r->active, 0, memory_order_release); /* the record (and its retired list) may be reused */
}

static void hp_free_all(void) { /* atexit: every other thread has finished (workers/OS threads are joined before) */
    HpRec *r = atomic_load_explicit(&g_hp_head, memory_order_acquire);
    atomic_store_explicit(&g_hp_head, NULL, memory_order_relaxed);
    while (r) {
        HpRec *n = r->next;
        for (size_t i = 0; i < r->nret; i++) free(r->retired[i]);
        free(r->retired);
        free(r);
        r = n;
    }
}

static void hp_once_init(void) {
    pthread_key_create(&g_hp_key, hp_thread_exit);
    atexit(hp_free_all);
}

static HpRec *hp_rec(void) {
    if (t_hp) return t_hp;
    pthread_once(&g_hp_once, hp_once_init);
    HpRec *r;
    /* reuse a record whose thread has exited: CAS 0 -> 1 (acquire: see the retired list its old owner left) */
    for (r = atomic_load_explicit(&g_hp_head, memory_order_acquire); r; r = r->next) {
        int z = 0;
        if (atomic_compare_exchange_strong_explicit(&r->active, &z, 1, memory_order_acquire, memory_order_relaxed)) break;
    }
    if (!r) {
        r = calloc(1, sizeof(HpRec));
        if (!r) rt_die("runtime: out of memory\n");
        atomic_init(&r->active, 1);
        r->next = atomic_load_explicit(&g_hp_head, memory_order_relaxed);
        /* publish with release: the fields above are visible to whoever walks the list (acquire) */
        while (!atomic_compare_exchange_weak_explicit(&g_hp_head, &r->next, r, memory_order_release, memory_order_relaxed)) {}
    }
    pthread_setspecific(g_hp_key, r); /* registers the destructor for this thread */
    t_hp = r;
    return r;
}

/* Loads `*src` and publishes it as hazard `i`, retrying until the published value is still current. */
static void *hp_protect(HpRec *r, int i, _Atomic(void *) *src) {
    void *p = atomic_load_explicit(src, memory_order_acquire); /* acquire: the node's fields are visible */
    for (;;) {
        /* seq_cst store: the hazard must be visible to a scanner BEFORE we re-validate (store-load order) */
        atomic_store_explicit(&r->hp[i], p, memory_order_seq_cst);
        void *q = atomic_load_explicit(src, memory_order_seq_cst); /* still the same node => not yet retired */
        if (q == p) return p;
        p = q;
    }
}

static void hp_clear(HpRec *r) {
    /* release: our reads of the node happen before a scanner may see the hazard gone and free it */
    for (int i = 0; i < HP_PER; i++) atomic_store_explicit(&r->hp[i], NULL, memory_order_release);
}

static void hp_scan(HpRec *self) {
    size_t cap = 16, n = 0;
    void **prot = malloc(cap * sizeof(void *));
    if (!prot) rt_die("runtime: out of memory\n");
    for (HpRec *r = atomic_load_explicit(&g_hp_head, memory_order_acquire); r; r = r->next)
        for (int i = 0; i < HP_PER; i++) {
            /* seq_cst: pairs with the seq_cst hazard store in hp_protect (we retired the node first, so a
               thread that published it later re-validates and sees it is no longer current) */
            void *p = atomic_load_explicit(&r->hp[i], memory_order_seq_cst);
            if (!p) continue;
            if (n == cap) {
                cap *= 2;
                prot = realloc(prot, cap * sizeof(void *));
                if (!prot) rt_die("runtime: out of memory\n");
            }
            prot[n++] = p;
        }
    size_t keep = 0;
    for (size_t i = 0; i < self->nret; i++) {
        void *node = self->retired[i];
        int hazard = 0;
        for (size_t j = 0; j < n; j++) if (prot[j] == node) { hazard = 1; break; }
        if (hazard) self->retired[keep++] = node; else free(node);
    }
    self->nret = keep;
    free(prot);
}

static void hp_retire(HpRec *r, void *node) {
    if (r->nret == r->capret) {
        r->capret = r->capret ? r->capret * 2 : 128;
        r->retired = realloc(r->retired, r->capret * sizeof(void *));
        if (!r->retired) rt_die("runtime: out of memory\n");
    }
    r->retired[r->nret++] = node;
    if (r->nret >= HP_SCAN) hp_scan(r);
}

/* ---- the channel -------------------------------------------------------------------------------- */

typedef struct ChNode {
    _Atomic(struct ChNode *) next;
    /* the payload starts at `doff` */
} ChNode;

typedef struct Chan {
    size_t esize, ealign, doff, cellsz, cap; /* cap = ring cells (at least 2) */
    size_t lim;                              /* the logical capacity the program asked for (1 <= lim <= cap) */
    drop_fn drop;
    int bounded;
    unsigned char *cells;                  /* bounded: cap cells of `cellsz` bytes: { atomic size_t seq; payload at doff } */
    _Alignas(64) atomic_size_t enq;        /* bounded: next position to write (producers claim it by CAS) */
    _Alignas(64) atomic_size_t deq;        /* bounded: next position to read */
    _Alignas(64) _Atomic(ChNode *) head;   /* unbounded: the dummy node; the oldest message is head->next */
    _Atomic(ChNode *) tail;                /* unbounded: last node, or one behind it ("lagging") */
    _Alignas(64) atomic_int senders, receivers, refs;
    atomic_int r_waiters, s_waiters;       /* threads sleeping in / tasks registered for recv resp. send */
    pthread_mutex_t mu;                    /* protects the wait lists and is held across cond_wait */
    pthread_cond_t cv_r, cv_s;
    Task *rq, *sq;                         /* async tasks waiting to receive / to send */
} Chan;

static size_t ch_align_up(size_t v, size_t a) { return (v + a - 1) / a * a; }

static void *ch_alloc(size_t align, size_t size) {
    void *p = NULL;
    if (align < 16) align = 16;
    if (posix_memalign(&p, align, ch_align_up(size ? size : 1, align)) != 0) rt_die("runtime: out of memory\n");
    return p;
}

/* A handle pair is created with one sender and one receiver. cap == 0 means unbounded. */
void *__mtn_chan_new(size_t esize, size_t ealign, drop_fn drop, size_t cap) {
    Chan *c = ch_alloc(64, sizeof(Chan));
    memset(c, 0, sizeof(Chan));
    if (ealign < 1) ealign = 1;
    c->esize = esize;
    c->ealign = ealign;
    c->drop = drop;
    c->doff = ch_align_up(sizeof(_Atomic(ChNode *)) > sizeof(size_t) ? sizeof(_Atomic(ChNode *)) : sizeof(size_t), ealign);
    c->bounded = cap != 0;
    c->lim = cap;
    cap = cap == 1 ? 2 : cap; /* Vyukov's ring needs >= 2 cells: with 1, `pos + cap` and `pos + 1` coincide */
    c->cap = cap;
    if (c->bounded) {
        c->cellsz = ch_align_up(c->doff + esize, ealign > 8 ? ealign : 8);
        c->cells = ch_alloc(c->cellsz > 64 ? 64 : 16, c->cellsz * cap);
        for (size_t i = 0; i < cap; i++) atomic_init((atomic_size_t *)(c->cells + i * c->cellsz), i); /* slot i is free for position i */
        atomic_init(&c->enq, 0);
        atomic_init(&c->deq, 0);
    } else {
        ChNode *dummy = ch_alloc(ealign, c->doff + esize);
        atomic_init(&dummy->next, NULL);
        atomic_init(&c->head, dummy);
        atomic_init(&c->tail, dummy);
    }
    atomic_init(&c->senders, 1);
    atomic_init(&c->receivers, 1);
    atomic_init(&c->refs, 2);
    atomic_init(&c->r_waiters, 0);
    atomic_init(&c->s_waiters, 0);
    pthread_mutex_init(&c->mu, NULL);
    pthread_cond_init(&c->cv_r, NULL);
    pthread_cond_init(&c->cv_s, NULL);
    return c;
}

/* ---- bounded queue: Vyukov -------------------------------------------------------------------- */

/* `sc` selects seq_cst loads (the waiter's re-check, see ch_notify_*); otherwise acquire. 1 = pushed, 0 = full. */
static int bq_push(Chan *c, const void *val, int sc) {
    memory_order lo = sc ? memory_order_seq_cst : memory_order_acquire;
    size_t pos = atomic_load_explicit(&c->enq, sc ? memory_order_seq_cst : memory_order_relaxed); /* relaxed: only a hint; the CAS and the slot's sequence decide */
    unsigned char *cell;
    for (;;) {
        if (c->lim < c->cap) {
            /* logical capacity 1 on a 2-cell ring: refuse when `lim` messages are unread. A stale `pos` can make
               the difference negative (signed compare: not full). A stale `deq` is ruled out for a sleeper's
               re-check because the consumer's claim of `deq` is a seq_cst CAS (see bq_pop) and this load is
               seq_cst: it cannot miss a claim that happens before the consumer's seq_cst notification. */
            intptr_t used = (intptr_t)(pos - atomic_load_explicit(&c->deq, memory_order_seq_cst));
            if (used >= (intptr_t)c->lim) return 0;
        }
        cell = c->cells + (pos % c->cap) * c->cellsz;
        /* acquire: pairs with the consumer's release/seq_cst store of `pos + cap` (the slot is free and the
           consumer has finished reading the old payload) */
        size_t seq = atomic_load_explicit((atomic_size_t *)cell, lo);
        intptr_t dif = (intptr_t)seq - (intptr_t)pos;
        if (dif == 0) {
            /* relaxed CAS: it only claims position `pos` among producers; the payload is handed over through
               the slot's sequence number, not through `enq` */
            if (atomic_compare_exchange_weak_explicit(&c->enq, &pos, pos + 1, memory_order_relaxed, memory_order_relaxed)) break;
        } else if (dif < 0) {
            return 0; /* the slot still holds an unread message from one lap ago: full */
        } else {
            pos = atomic_load_explicit(&c->enq, memory_order_relaxed); /* another producer got ahead: retry */
        }
    }
    memcpy(cell + c->doff, val, c->esize);
    /* seq_cst store: release (the payload above is visible to the consumer that acquire-loads this sequence) AND
       the first half of the store-load handshake with a receiver that is about to sleep (ch_notify_recv) */
    atomic_store_explicit((atomic_size_t *)cell, pos + 1, memory_order_seq_cst);
    return 1;
}

/* 1 = popped into `out`, 0 = empty. */
static int bq_pop(Chan *c, void *out, int sc) {
    memory_order lo = sc ? memory_order_seq_cst : memory_order_acquire;
    size_t pos = atomic_load_explicit(&c->deq, sc ? memory_order_seq_cst : memory_order_relaxed); /* relaxed: a hint */
    unsigned char *cell;
    for (;;) {
        cell = c->cells + (pos % c->cap) * c->cellsz;
        /* acquire: pairs with the producer's seq_cst store of `pos + 1` (the payload is complete) */
        size_t seq = atomic_load_explicit((atomic_size_t *)cell, lo);
        intptr_t dif = (intptr_t)seq - (intptr_t)(pos + 1);
        if (dif == 0) {
            /* seq_cst CAS: claims position `pos` among consumers (the payload comes through the sequence number);
               seq_cst because a capacity-1 sender's re-check reads `deq` (bq_push) and must not miss this claim */
            if (atomic_compare_exchange_weak_explicit(&c->deq, &pos, pos + 1, memory_order_seq_cst, memory_order_relaxed)) break;
        } else if (dif < 0) {
            return 0; /* nothing published at this position: empty */
        } else {
            pos = atomic_load_explicit(&c->deq, memory_order_relaxed);
        }
    }
    memcpy(out, cell + c->doff, c->esize);
    /* seq_cst store: release (our read of the payload is finished before a producer may overwrite the slot) AND
       the first half of the handshake with a sender that is about to sleep on a full channel (ch_notify_send) */
    atomic_store_explicit((atomic_size_t *)cell, pos + c->cap, memory_order_seq_cst);
    return 1;
}

/* ---- unbounded queue: Michael-Scott with hazard pointers --------------------------------------- */

static void uq_push(Chan *c, const void *val) {
    ChNode *n = ch_alloc(c->ealign, c->doff + c->esize);
    atomic_init(&n->next, NULL); /* not yet shared: a plain init */
    memcpy((unsigned char *)n + c->doff, val, c->esize);
    HpRec *r = hp_rec();
    for (;;) {
        ChNode *t = hp_protect(r, 0, (_Atomic(void *) *)&c->tail);
        ChNode *nx = atomic_load_explicit(&t->next, memory_order_acquire); /* acquire: nx's fields are visible */
        if (t != atomic_load_explicit(&c->tail, memory_order_acquire)) continue; /* the tail moved on: retry */
        if (nx) {
            /* the tail lags (another producer linked a node but did not swing the tail yet): HELP it forward.
               release: the node's contents are published to whoever loads the tail with acquire */
            ChNode *te = t;
            atomic_compare_exchange_strong_explicit(&c->tail, &te, nx, memory_order_release, memory_order_relaxed);
            continue;
        }
        ChNode *exp = NULL;
        /* seq_cst CAS: the linearisation point. Release (payload visible to the consumer that acquire-loads
           `next`) AND the first half of the handshake with a receiver about to sleep (ch_notify_recv) */
        if (atomic_compare_exchange_strong_explicit(&t->next, &exp, n, memory_order_seq_cst, memory_order_relaxed)) {
            ChNode *te = t;
            /* swing the tail; failure is fine (a helper already did it). release: see above */
            atomic_compare_exchange_strong_explicit(&c->tail, &te, n, memory_order_release, memory_order_relaxed);
            break;
        }
    }
    hp_clear(r);
}

static int uq_pop(Chan *c, void *out, int sc) {
    HpRec *r = hp_rec();
    for (;;) {
        ChNode *h = hp_protect(r, 0, (_Atomic(void *) *)&c->head);
        ChNode *t = atomic_load_explicit(&c->tail, memory_order_acquire);
        /* seq_cst when this is a waiter's re-check (second half of the handshake), else acquire (pairs with the
           producer's seq_cst CAS that linked the node) */
        ChNode *nx = atomic_load_explicit(&h->next, sc ? memory_order_seq_cst : memory_order_acquire);
        atomic_store_explicit(&r->hp[1], nx, memory_order_seq_cst); /* protect `nx` BEFORE validating (store-load order) */
        if (h != atomic_load_explicit(&c->head, memory_order_seq_cst)) continue; /* head moved: h and nx may be stale */
        if (!nx) {
            hp_clear(r);
            return 0; /* empty */
        }
        if (h == t) {
            /* the tail lags behind a linked node: help it forward before dequeuing past it */
            ChNode *te = t;
            atomic_compare_exchange_strong_explicit(&c->tail, &te, nx, memory_order_release, memory_order_relaxed);
            continue;
        }
        ChNode *he = h;
        /* acq_rel: acquire pairs with the producer's publication of nx; release orders our hazard reads before
           anyone who sees the new head */
        if (atomic_compare_exchange_strong_explicit(&c->head, &he, nx, memory_order_acq_rel, memory_order_relaxed)) {
            /* we won nx: it is the new dummy and its payload is ours alone. `nx` is still protected by hazard 1,
               so a concurrent dequeuer that retires it later cannot free it before we have copied. */
            memcpy(out, (unsigned char *)nx + c->doff, c->esize);
            hp_clear(r);
            hp_retire(r, h); /* the old dummy is unreachable now; freed once no hazard names it */
            return 1;
        }
    }
}

static int ch_push(Chan *c, const void *val, int sc) {
    if (c->bounded) return bq_push(c, val, sc);
    uq_push(c, val);
    return 1;
}

static int ch_pop(Chan *c, void *out, int sc) {
    return c->bounded ? bq_pop(c, out, sc) : uq_pop(c, out, sc);
}

/* ---- closing, counting, waking ----------------------------------------------------------------- */

static void ch_destroy(Chan *c) {
    /* no handle exists any more: nothing else touches the queue. Drop every message that is still queued, once. */
    if (c->bounded) {
        size_t d = atomic_load_explicit(&c->deq, memory_order_relaxed), e = atomic_load_explicit(&c->enq, memory_order_relaxed);
        for (size_t pos = d; pos != e; pos++) {
            unsigned char *cell = c->cells + (pos % c->cap) * c->cellsz;
            /* a claimed slot is published before its sender returns, and no sender is running now */
            if (atomic_load_explicit((atomic_size_t *)cell, memory_order_relaxed) == pos + 1 && c->drop) c->drop(cell + c->doff);
        }
        free(c->cells);
    } else {
        ChNode *n = atomic_load_explicit(&c->head, memory_order_relaxed); /* the dummy: its payload was already moved out */
        ChNode *nx = atomic_load_explicit(&n->next, memory_order_relaxed);
        free(n);
        while (nx) {
            n = nx;
            nx = atomic_load_explicit(&n->next, memory_order_relaxed);
            if (c->drop) c->drop((unsigned char *)n + c->doff);
            free(n);
        }
    }
    pthread_mutex_destroy(&c->mu);
    pthread_cond_destroy(&c->cv_r);
    pthread_cond_destroy(&c->cv_s);
    free(c);
}

static void ch_unref(Chan *c) {
    /* acq_rel: release our last use of the channel; the thread that frees it acquires everyone else's */
    if (atomic_fetch_sub_explicit(&c->refs, 1, memory_order_acq_rel) == 1) ch_destroy(c);
}

/* Wakes every async task in `*list`, clearing their registration (caller holds c->mu). */
static void ch_wake_list(Chan *c, Task **list, atomic_int *counter) {
    (void)c;
    Task *w = *list;
    *list = NULL;
    if (!w) return;
    pthread_mutex_lock(&g_mu); /* lock order: c->mu, then g_mu (never the other way round) */
    int n = 0;
    while (w) {
        Task *nx = w->chan_next;
        w->chan_next = NULL;
        w->chan_wait = NULL;
        wake(w);
        n++;
        w = nx;
    }
    pthread_mutex_unlock(&g_mu);
    atomic_fetch_sub_explicit(counter, n, memory_order_seq_cst);
}

/* LOST-WAKEUP PROTOCOL (receivers; `ch_notify_send` is the mirror image).
 *   notifier:  publish a message (seq_cst store / CAS)  ... then load `r_waiters` (seq_cst)
 *   waiter:    `r_waiters++` (seq_cst RMW)              ... then re-check the queue (seq_cst loads), then sleep
 * All four accesses are seq_cst, so they sit in one total order: either the notifier sees the waiter (and wakes
 * it, taking `mu`, which the waiter holds until it is really asleep in cond_wait) or the waiter sees the message
 * and does not sleep. Closing a side uses the same pattern (`senders` / `receivers` are seq_cst). */
static void ch_notify_recv(Chan *c) {
    if (atomic_load_explicit(&c->r_waiters, memory_order_seq_cst) == 0) return; /* fast path: nobody is waiting */
    pthread_mutex_lock(&c->mu);
    pthread_cond_broadcast(&c->cv_r);
    ch_wake_list(c, &c->rq, &c->r_waiters);
    pthread_mutex_unlock(&c->mu);
}

static void ch_notify_send(Chan *c) {
    if (atomic_load_explicit(&c->s_waiters, memory_order_seq_cst) == 0) return;
    pthread_mutex_lock(&c->mu);
    pthread_cond_broadcast(&c->cv_s);
    ch_wake_list(c, &c->sq, &c->s_waiters);
    pthread_mutex_unlock(&c->mu);
}

void *__mtn_chan_sender_clone(void *p) {
    Chan *c = p;
    /* relaxed: we already hold a handle, so the counts cannot reach zero while we increment */
    atomic_fetch_add_explicit(&c->senders, 1, memory_order_relaxed);
    atomic_fetch_add_explicit(&c->refs, 1, memory_order_relaxed);
    return c;
}

void *__mtn_chan_receiver_clone(void *p) {
    Chan *c = p;
    atomic_fetch_add_explicit(&c->receivers, 1, memory_order_relaxed);
    atomic_fetch_add_explicit(&c->refs, 1, memory_order_relaxed);
    return c;
}

void __mtn_chan_sender_drop(void *p) {
    Chan *c = p;
    if (!c) return;
    /* seq_cst RMW: all this handle's sends happen before the count shows the drop (release), and it is the
       first half of the handshake with a receiver about to sleep */
    if (atomic_fetch_sub_explicit(&c->senders, 1, memory_order_seq_cst) == 1) ch_notify_recv(c); /* the last sender: receivers see "closed" */
    ch_unref(c);
}

void __mtn_chan_receiver_drop(void *p) {
    Chan *c = p;
    if (!c) return;
    if (atomic_fetch_sub_explicit(&c->receivers, 1, memory_order_seq_cst) == 1) ch_notify_send(c); /* the last receiver: senders see "closed" */
    ch_unref(c);
}

static int ch_recv_closed(Chan *c) { return atomic_load_explicit(&c->senders, memory_order_seq_cst) == 0; }
static int ch_send_closed(Chan *c) { return atomic_load_explicit(&c->receivers, memory_order_seq_cst) == 0; }

/* ---- compensation for a pool worker that blocks -------------------------------------------------- */

/* Called (holding the channel's `mu`) just before a POOL worker sleeps in a synchronous channel wait. The pool
   keeps at least `g_nworkers` workers that are not blocked: while fewer are available it starts an extra worker
   thread (they stay until shutdown). This is what lets a producer task and a consumer task share a
   `channel(1)` with `MTN_WORKERS=1`: when the producer blocks on a full channel, an extra worker runs the
   consumer. At most MAX_EXTRA extras exist; beyond that a blocked worker simply sleeps. */
static void comp_enter(void) {
    if (!t_is_pool) return;
    pthread_mutex_lock(&g_mu);
    g_blocked++;
    while (g_extra < g_blocked && g_extra < MAX_EXTRA) {
        if (g_extra == g_capx) {
            g_capx = g_capx ? g_capx * 2 : 16;
            g_xthreads = realloc(g_xthreads, (size_t)g_capx * sizeof(pthread_t));
            if (!g_xthreads) rt_die("runtime: out of memory\n");
        }
        /* a compensating worker shares a deque index with an original worker (deques are guarded by g_mu) */
        spawn_thread(&g_xthreads[g_extra], worker_main, (void *)(intptr_t)(g_extra % g_nworkers));
        g_extra++;
    }
    pthread_mutex_unlock(&g_mu);
}

static void comp_leave(void) {
    if (!t_is_pool) return;
    pthread_mutex_lock(&g_mu);
    g_blocked--;
    pthread_mutex_unlock(&g_mu);
}

/* ---- operations ---------------------------------------------------------------------------------- */

/* Blocking send. 0 = sent (the value now belongs to the channel), 1 = every receiver is gone (the value stays
   with the caller). */
int __mtn_chan_send(void *p, void *val) {
    Chan *c = p;
    for (int spin = 0;;) {
        if (ch_send_closed(c)) return 1;
        if (ch_push(c, val, 0)) {
            ch_notify_recv(c);
            return 0;
        }
        if (spin < CH_SPIN) {
            spin++;
            cpu_relax();
            continue;
        }
        pthread_mutex_lock(&c->mu);
        atomic_fetch_add_explicit(&c->s_waiters, 1, memory_order_seq_cst); /* first half of the handshake */
        int st = 0;
        if (ch_send_closed(c)) st = 1;
        else if (ch_push(c, val, 1)) st = 2; /* second half: a seq_cst re-check */
        if (!st) {
            comp_enter();
            pthread_cond_wait(&c->cv_s, &c->mu);
            comp_leave();
        }
        atomic_fetch_sub_explicit(&c->s_waiters, 1, memory_order_seq_cst);
        pthread_mutex_unlock(&c->mu);
        if (st == 1) return 1;
        if (st == 2) {
            ch_notify_recv(c);
            return 0;
        }
    }
}

/* 0 = sent, 1 = closed (value stays with the caller), 2 = full (value stays with the caller). */
int __mtn_chan_try_send(void *p, void *val) {
    Chan *c = p;
    if (ch_send_closed(c)) return 1;
    if (!ch_push(c, val, 0)) return 2;
    ch_notify_recv(c);
    return 0;
}

/* Blocking receive. 1 = a value was moved into `out`, 0 = closed and empty. */
int __mtn_chan_recv(void *p, void *out) {
    Chan *c = p;
    for (int spin = 0;;) {
        if (ch_pop(c, out, 0)) {
            ch_notify_send(c);
            return 1;
        }
        if (ch_recv_closed(c)) { /* every sender is gone: anything they sent is visible now; take it first */
            if (ch_pop(c, out, 1)) {
                ch_notify_send(c);
                return 1;
            }
            return 0;
        }
        if (spin < CH_SPIN) {
            spin++;
            cpu_relax();
            continue;
        }
        pthread_mutex_lock(&c->mu);
        atomic_fetch_add_explicit(&c->r_waiters, 1, memory_order_seq_cst);
        int st = 0;
        if (ch_pop(c, out, 1)) st = 1;
        else if (ch_recv_closed(c)) st = ch_pop(c, out, 1) ? 1 : 2;
        if (!st) {
            comp_enter();
            pthread_cond_wait(&c->cv_r, &c->mu);
            comp_leave();
        }
        atomic_fetch_sub_explicit(&c->r_waiters, 1, memory_order_seq_cst);
        pthread_mutex_unlock(&c->mu);
        if (st == 1) {
            ch_notify_send(c);
            return 1;
        }
        if (st == 2) return 0;
    }
}

/* 1 = a value was moved into `out`, 0 = nothing available right now. */
int __mtn_chan_try_recv(void *p, void *out) {
    Chan *c = p;
    if (!ch_pop(c, out, 0)) return 0;
    ch_notify_send(c);
    return 1;
}

/* ---- async forms: the task registers a wake-up and the frame suspends ---------------------------- */

static void ch_unregister(Chan *c, Task *me, Task **list, atomic_int *counter) {
    if (me->chan_wait != c) return; /* a notifier already woke us and cleared the registration */
    for (Task **pp = list; *pp; pp = &(*pp)->chan_next)
        if (*pp == me) {
            *pp = me->chan_next;
            break;
        }
    me->chan_next = NULL;
    me->chan_wait = NULL;
    atomic_fetch_sub_explicit(counter, 1, memory_order_seq_cst);
}

/* `await tx.send(v)`: 0 = sent, 1 = closed (value stays), 2 = pending (the frame must suspend and call again). */
int __mtn_chan_send_poll(void *p, void *val) {
    Chan *c = p;
    Task *me = t_cur;
    if (!me) rt_die("runtime: `await` of a channel operation outside a task\n");
    pthread_mutex_lock(&c->mu);
    ch_unregister(c, me, &c->sq, &c->s_waiters); /* a re-poll starts clean */
    int st = 0;
    if (ch_send_closed(c)) st = 1;
    else if (ch_push(c, val, 0)) st = 2;
    if (!st) { /* register, then re-check (the handshake) */
        me->chan_wait = c;
        me->chan_next = c->sq;
        c->sq = me;
        atomic_fetch_add_explicit(&c->s_waiters, 1, memory_order_seq_cst);
        if (ch_send_closed(c)) st = 1;
        else if (ch_push(c, val, 1)) st = 2;
        if (st) ch_unregister(c, me, &c->sq, &c->s_waiters);
    }
    pthread_mutex_unlock(&c->mu);
    if (st == 2) {
        ch_notify_recv(c);
        return 0;
    }
    return st == 1 ? 1 : 2;
}

/* `await rx.recv()`: 1 = a value was moved into `out`, 0 = closed and empty, 2 = pending. */
int __mtn_chan_recv_poll(void *p, void *out) {
    Chan *c = p;
    Task *me = t_cur;
    if (!me) rt_die("runtime: `await` of a channel operation outside a task\n");
    pthread_mutex_lock(&c->mu);
    ch_unregister(c, me, &c->rq, &c->r_waiters);
    int st = 0; /* 1 = got a value, 3 = closed and empty */
    if (ch_pop(c, out, 0)) st = 1;
    else if (ch_recv_closed(c)) st = ch_pop(c, out, 1) ? 1 : 3;
    if (!st) {
        me->chan_wait = c;
        me->chan_next = c->rq;
        c->rq = me;
        atomic_fetch_add_explicit(&c->r_waiters, 1, memory_order_seq_cst);
        if (ch_pop(c, out, 1)) st = 1;
        else if (ch_recv_closed(c)) st = ch_pop(c, out, 1) ? 1 : 3;
        if (st) ch_unregister(c, me, &c->rq, &c->r_waiters);
    }
    pthread_mutex_unlock(&c->mu);
    if (st == 1) {
        ch_notify_send(c);
        return 1;
    }
    return st == 3 ? 0 : 2;
}

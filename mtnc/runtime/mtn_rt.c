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
 */
#define _GNU_SOURCE
#include <pthread.h>
#include <sched.h>
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

enum { K_SYNC = 0, K_FUTURE = 1, K_OS = 2 };
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

static __thread int t_worker = -1;
static __thread Task *t_cur; /* the future task being polled on this thread */

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
static void run_task(Task *t) {
    Task *saved = t_cur;
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

/* `h.join()`: waits for the task, moves its result into `out`, releases the handle. */
void __mtn_rt_join(void *h, void *out) {
    Task *t = h;
    pthread_mutex_lock(&g_mu);
    while (!t->done) {
        Task *other = t_worker >= 0 ? take_task(t_worker) : NULL;
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
    if (timer) pthread_join(g_timer_thread, NULL);
    while (os) {
        Task *n = os->os_next;
        pthread_join(os->thread, NULL);
        task_free(os);
        os = n;
    }
    const char *st = getenv("MTN_RT_STATS");
    if (st && st[0] == '1' && st[1] == 0)
        fprintf(stderr, "rt: workers=%d spawned=%lu completed=%lu peak_live=%lu\n", g_nworkers, g_spawned, g_completed, g_peak);
    pthread_cond_destroy(&g_timer_cv);
    free(g_heap);
    free(g_dq);
    free(g_threads);
}

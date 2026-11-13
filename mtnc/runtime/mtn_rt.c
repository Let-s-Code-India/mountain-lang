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
 */
#define _GNU_SOURCE
#include <pthread.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define STACK_BYTES ((size_t)8 * 1024 * 1024) /* 8 MiB per worker / OS thread */

typedef void (*thunk_fn)(void *code, void *env, void *out);
typedef void (*drop_fn)(void *slot);

typedef struct Task {
    thunk_fn thunk;
    void *code, *env;
    void *res;        /* result slot (res_size bytes) */
    size_t res_size;
    drop_fn res_drop; /* drops an untaken result (NULL if nothing to drop) */
    int done;
    int handle_alive; /* the JoinHandle still exists and has not joined */
    int is_os;
    pthread_t thread; /* is_os only */
    struct Task *next, *prev; /* deque links */
    struct Task *os_next;     /* list of OS tasks */
} Task;

typedef struct {
    Task *head, *tail; /* head = front (stolen from), tail = back (owner pops) */
} Deque;

static pthread_mutex_t g_mu = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t g_cv = PTHREAD_COND_INITIALIZER;
static Deque *g_dq;
static pthread_t *g_threads;
static int g_nworkers;
static int g_queued;     /* tasks sitting in deques */
static long g_pending;   /* spawned tasks not yet finished (queued or running) */
static long g_os_live;   /* OS threads not yet finished */
static int g_shutdown;
static int g_rr;         /* round-robin target for spawns from non-worker threads */
static Task *g_os_list;
static unsigned long g_spawned, g_completed, g_live, g_peak;

static __thread int t_worker = -1;

static void rt_die(const char *msg) {
    (void)!write(2, msg, strlen(msg));
    _exit(101);
}

static void dq_push_back(Deque *d, Task *t) {
    t->next = NULL;
    t->prev = d->tail;
    if (d->tail) d->tail->next = t; else d->head = t;
    d->tail = t;
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

/* Takes one queued task (own deque first, then steal). Caller holds g_mu. */
static Task *take_task(int me) {
    if (g_queued == 0) return NULL;
    Task *t = NULL;
    if (me >= 0) t = dq_pop_back(&g_dq[me]);
    for (int i = 0; !t && i < g_nworkers; i++) t = dq_pop_front(&g_dq[(me < 0 ? 0 : me + 1 + i) % g_nworkers]);
    if (t) g_queued--;
    return t;
}

static void task_free(Task *t) {
    free(t->res);
    free(t);
}

/* Runs `t` (not holding g_mu) and publishes the result. */
static void run_task(Task *t) {
    t->thunk(t->code, t->env, t->res);
    pthread_mutex_lock(&g_mu);
    t->done = 1;
    g_completed++;
    g_live--;
    g_pending--;
    int orphan = !t->handle_alive;
    pthread_cond_broadcast(&g_cv);
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
    for (int i = 0; i < n; i++) spawn_thread(&g_threads[i], worker_main, (void *)(intptr_t)i);
}

static Task *task_new(thunk_fn thunk, void *code, void *env, size_t res_size, drop_fn res_drop) {
    Task *t = calloc(1, sizeof(Task));
    if (!t) rt_die("runtime: out of memory\n");
    t->res = calloc(1, res_size ? res_size : 1);
    if (!t->res) rt_die("runtime: out of memory\n");
    t->thunk = thunk;
    t->code = code;
    t->env = env;
    t->res_size = res_size;
    t->res_drop = res_drop;
    t->handle_alive = 1;
    return t;
}

void *__mtn_rt_spawn(thunk_fn thunk, void *code, void *env, size_t res_size, drop_fn res_drop) {
    Task *t = task_new(thunk, code, env, res_size, res_drop);
    pthread_mutex_lock(&g_mu);
    g_spawned++;
    g_pending++;
    if (++g_live > g_peak) g_peak = g_live;
    int target = t_worker >= 0 ? t_worker : (g_rr++ % g_nworkers);
    dq_push_back(&g_dq[target], t);
    g_queued++;
    pthread_cond_broadcast(&g_cv);
    pthread_mutex_unlock(&g_mu);
    return t;
}

static void *os_main(void *arg) {
    Task *t = arg;
    t->thunk(t->code, t->env, t->res);
    pthread_mutex_lock(&g_mu);
    t->done = 1;
    g_os_live--;
    int orphan = !t->handle_alive;
    pthread_cond_broadcast(&g_cv);
    pthread_mutex_unlock(&g_mu);
    if (orphan && t->res_drop) t->res_drop(t->res);
    return NULL;
}

void *__mtn_rt_spawn_os(thunk_fn thunk, void *code, void *env, size_t res_size, drop_fn res_drop) {
    Task *t = task_new(thunk, code, env, res_size, res_drop);
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
    pthread_cond_broadcast(&g_cv);
    Task *os = g_os_list;
    g_os_list = NULL;
    pthread_mutex_unlock(&g_mu);
    for (int i = 0; i < g_nworkers; i++) pthread_join(g_threads[i], NULL);
    while (os) {
        Task *n = os->os_next;
        pthread_join(os->thread, NULL);
        task_free(os);
        os = n;
    }
    const char *st = getenv("MTN_RT_STATS");
    if (st && st[0] == '1' && st[1] == 0)
        fprintf(stderr, "rt: workers=%d spawned=%lu completed=%lu peak_live=%lu\n", g_nworkers, g_spawned, g_completed, g_peak);
    free(g_dq);
    free(g_threads);
}

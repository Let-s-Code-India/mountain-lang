/* ThreadSanitizer / AddressSanitizer harness for the C runtime's lock and channel code (Phase 13 M3).
 *
 * Build and run with `scripts/tsan_runtime.sh`. It links `runtime/mtn_rt.c` (compiled with the sanitizer)
 * and drives the exported `__mtn_lock_*` and `__mtn_chan_*` functions from several plain pthreads, the way
 * compiled Mountain programs do: exact counts, no loss, no duplicates, every queued value dropped once.
 * Any assertion failure aborts; any sanitizer finding is printed by the sanitizer and fails the script. */
#include <assert.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef void (*drop_fn)(void *slot);
void *__mtn_chan_new(size_t esize, size_t ealign, drop_fn drop, size_t cap);
void *__mtn_chan_sender_clone(void *);
void *__mtn_chan_receiver_clone(void *);
void __mtn_chan_sender_drop(void *);
void __mtn_chan_receiver_drop(void *);
int __mtn_chan_send(void *, void *);
int __mtn_chan_try_send(void *, void *);
int __mtn_chan_recv(void *, void *);
int __mtn_chan_try_recv(void *, void *);
int __mtn_chan_select(void **chs, int n, void **outs);
void __mtn_lock_mutex_lock(void *);
int __mtn_lock_mutex_trylock(void *);
void __mtn_lock_mutex_unlock(void *);
void __mtn_lock_rw_read(void *);
void __mtn_lock_rw_read_unlock(void *);
void __mtn_lock_rw_write(void *);
void __mtn_lock_rw_write_unlock(void *);
void __mtn_lock_spin_acquire(void *);
void __mtn_lock_spin_release(void *);

/* ---------------------------------------------------------------- channels */

typedef struct {
    void *tx, *rx;
    int id, per;
    uint64_t sum, count;
} Worker;

static void *producer(void *a) {
    Worker *w = a;
    for (int i = 0; i < w->per; i++) {
        uint64_t v = (uint64_t)w->id * 1000000u + (uint64_t)i + 1;
        int r = __mtn_chan_send(w->tx, &v);
        assert(r == 0);
    }
    __mtn_chan_sender_drop(w->tx);
    return NULL;
}

static void *consumer(void *a) {
    Worker *w = a;
    uint64_t v;
    while (__mtn_chan_recv(w->rx, &v)) {
        w->sum += v;
        w->count++;
    }
    __mtn_chan_receiver_drop(w->rx);
    return NULL;
}

static void run_mpmc(size_t cap, int np, int nc, int per) {
    void *c = __mtn_chan_new(8, 8, NULL, cap);
    Worker ps[16], cs[16];
    pthread_t pt[16], ct[16];
    for (int i = 0; i < np; i++) {
        ps[i] = (Worker){.tx = i == 0 ? c : __mtn_chan_sender_clone(c), .id = i, .per = per};
    }
    for (int i = 0; i < nc; i++) {
        cs[i] = (Worker){.rx = i == 0 ? c : __mtn_chan_receiver_clone(c)};
    }
    for (int i = 0; i < nc; i++) pthread_create(&ct[i], NULL, consumer, &cs[i]);
    for (int i = 0; i < np; i++) pthread_create(&pt[i], NULL, producer, &ps[i]);
    for (int i = 0; i < np; i++) pthread_join(pt[i], NULL);
    for (int i = 0; i < nc; i++) pthread_join(ct[i], NULL);
    uint64_t sum = 0, count = 0, want = 0;
    for (int i = 0; i < nc; i++) { sum += cs[i].sum; count += cs[i].count; }
    for (int i = 0; i < np; i++) for (int j = 0; j < per; j++) want += (uint64_t)i * 1000000u + (uint64_t)j + 1;
    assert(count == (uint64_t)np * (uint64_t)per);
    assert(sum == want);
}

static _Atomic int g_dropped;
static void count_drop(void *slot) {
    void *p;
    memcpy(&p, slot, sizeof p);
    free(p);
    atomic_fetch_add(&g_dropped, 1);
}

static void run_close_and_drop(size_t cap) {
    /* values left in the channel when the last handle goes are dropped exactly once */
    atomic_store(&g_dropped, 0);
    void *c = __mtn_chan_new(sizeof(void *), sizeof(void *), count_drop, cap);
    int sent = 0;
    for (int i = 0; i < 5; i++) {
        void *p = malloc(16);
        if (__mtn_chan_try_send(c, &p) == 0) sent++; else free(p); /* full: the value stays with us */
    }
    assert(sent >= 1);
    void *got;
    assert(__mtn_chan_recv(c, &got) == 1);
    free(got); /* the receiver owns what it takes out */
    __mtn_chan_sender_drop(c);
    __mtn_chan_receiver_drop(c); /* the rest is dropped by the glue, once each */
    assert(atomic_load(&g_dropped) == sent - 1);

    /* closing by dropping the receivers: send fails and the value stays with the caller */
    c = __mtn_chan_new(8, 8, NULL, cap);
    void *rx = c;
    void *tx = __mtn_chan_sender_clone(c); /* c is both: counts are per kind */
    __mtn_chan_receiver_drop(rx);
    uint64_t v = 77;
    assert(__mtn_chan_send(tx, &v) == 1 && v == 77);
    assert(__mtn_chan_try_send(tx, &v) == 1);
    __mtn_chan_sender_drop(tx);
    __mtn_chan_sender_drop(c);
}

static void run_try(void) {
    void *c = __mtn_chan_new(8, 8, NULL, 2);
    uint64_t v = 1, out;
    assert(__mtn_chan_try_recv(c, &out) == 0);
    assert(__mtn_chan_try_send(c, &v) == 0);
    v = 2;
    assert(__mtn_chan_try_send(c, &v) == 0);
    v = 3;
    assert(__mtn_chan_try_send(c, &v) == 2); /* full */
    assert(__mtn_chan_try_recv(c, &out) == 1 && out == 1);
    assert(__mtn_chan_try_recv(c, &out) == 1 && out == 2);
    assert(__mtn_chan_try_recv(c, &out) == 0);
    __mtn_chan_sender_drop(c);
    assert(__mtn_chan_recv(c, &out) == 0); /* closed and empty */
    __mtn_chan_receiver_drop(c);
}


/* ---------------------------------------------------------------- select (Phase 13 M4) */

typedef struct {
    void *rx[3];
    int n;
    uint64_t sum, count;
} SelConsumer;

static void *select_consumer(void *a) {
    SelConsumer *w = a;
    uint64_t slot[3];
    void *outs[3] = {&slot[0], &slot[1], &slot[2]};
    void *chs[3] = {w->rx[0], w->rx[1], w->rx[2]};
    for (;;) {
        int r = __mtn_chan_select(chs, w->n, outs);
        int idx = r >> 1, got = r & 1;
        if (got) {
            w->sum += slot[idx];
            w->count++;
            continue;
        }
        /* arm `idx` is closed and empty: drain the others (a closed arm stays ready, so stop selecting) */
        for (int j = 0; j < w->n; j++) {
            uint64_t v;
            if (j == idx) continue;
            while (__mtn_chan_recv(w->rx[j], &v)) { w->sum += v; w->count++; }
        }
        break;
    }
    for (int j = 0; j < w->n; j++) __mtn_chan_receiver_drop(w->rx[j]);
    return NULL;
}

static void run_select(size_t cap, int nchan, int np, int per) {
    void *c[3];
    Worker ps[3 * 8];
    pthread_t pt[3 * 8], ct;
    SelConsumer sc = {.n = nchan};
    for (int k = 0; k < nchan; k++) {
        c[k] = __mtn_chan_new(8, 8, NULL, cap);
        sc.rx[k] = c[k];
        for (int i = 0; i < np; i++) {
            ps[k * np + i] = (Worker){.tx = i == 0 ? c[k] : __mtn_chan_sender_clone(c[k]), .id = k * np + i, .per = per};
        }
    }
    pthread_create(&ct, NULL, select_consumer, &sc);
    for (int i = 0; i < nchan * np; i++) pthread_create(&pt[i], NULL, producer, &ps[i]);
    for (int i = 0; i < nchan * np; i++) pthread_join(pt[i], NULL);
    pthread_join(ct, NULL);
    uint64_t want = 0;
    for (int i = 0; i < nchan * np; i++) for (int j = 0; j < per; j++) want += (uint64_t)i * 1000000u + (uint64_t)j + 1;
    assert(sc.count == (uint64_t)nchan * (uint64_t)np * (uint64_t)per);
    assert(sc.sum == want);
}

/* the arm's value is never lost or duplicated, and arms that lose consume nothing */
static void run_select_order(void) {
    void *a = __mtn_chan_new(8, 8, NULL, 4), *b = __mtn_chan_new(8, 8, NULL, 4);
    uint64_t v = 10, s0 = 0, s1 = 0, out;
    void *chs[2] = {a, b}, *outs[2] = {&s0, &s1};
    assert(__mtn_chan_try_send(b, &v) == 0);
    v = 20;
    assert(__mtn_chan_try_send(a, &v) == 0);
    assert(__mtn_chan_select(chs, 2, outs) == 1 && s0 == 20); /* arm 0 first in source order */
    assert(__mtn_chan_select(chs, 2, outs) == 3 && s1 == 10);
    assert(__mtn_chan_try_recv(a, &out) == 0 && __mtn_chan_try_recv(b, &out) == 0);
    __mtn_chan_sender_drop(a);
    __mtn_chan_sender_drop(b);
    assert(__mtn_chan_select(chs, 2, outs) == 0); /* all closed: arm 0 with None */
    __mtn_chan_receiver_drop(a);
    __mtn_chan_receiver_drop(b);
}

/* ---------------------------------------------------------------- locks */

typedef struct {
    int lock[8];
    int spin;
    long counter;
    long rw_value;
} Locked;

static Locked g_l;

static void *mutex_worker(void *a) {
    (void)a;
    for (int i = 0; i < 20000; i++) {
        __mtn_lock_mutex_lock(g_l.lock);
        g_l.counter++;
        __mtn_lock_mutex_unlock(g_l.lock);
    }
    return NULL;
}

static void *spin_worker(void *a) {
    (void)a;
    for (int i = 0; i < 20000; i++) {
        __mtn_lock_spin_acquire(&g_l.spin);
        g_l.counter++;
        __mtn_lock_spin_release(&g_l.spin);
    }
    return NULL;
}

static void *rw_reader(void *a) {
    (void)a;
    for (int i = 0; i < 5000; i++) {
        __mtn_lock_rw_read(g_l.lock);
        long v = g_l.rw_value;
        assert(v % 2 == 0); /* a writer keeps the value even outside its critical section */
        __mtn_lock_rw_read_unlock(g_l.lock);
    }
    return NULL;
}

static void *rw_writer(void *a) {
    (void)a;
    for (int i = 0; i < 2000; i++) {
        __mtn_lock_rw_write(g_l.lock);
        g_l.rw_value++;
        g_l.rw_value++;
        __mtn_lock_rw_write_unlock(g_l.lock);
    }
    return NULL;
}

static void run_locks(void) {
    pthread_t t[8];
    memset(&g_l, 0, sizeof g_l);
    for (int i = 0; i < 8; i++) pthread_create(&t[i], NULL, mutex_worker, NULL);
    for (int i = 0; i < 8; i++) pthread_join(t[i], NULL);
    assert(g_l.counter == 8 * 20000);
    g_l.counter = 0;
    for (int i = 0; i < 4; i++) pthread_create(&t[i], NULL, spin_worker, NULL);
    for (int i = 0; i < 4; i++) pthread_join(t[i], NULL);
    assert(g_l.counter == 4 * 20000);
    memset(g_l.lock, 0, sizeof g_l.lock);
    for (int i = 0; i < 4; i++) pthread_create(&t[i], NULL, rw_reader, NULL);
    pthread_create(&t[4], NULL, rw_writer, NULL);
    pthread_create(&t[5], NULL, rw_writer, NULL);
    for (int i = 0; i < 6; i++) pthread_join(t[i], NULL);
    assert(g_l.rw_value == 2 * 2 * 2000);
    memset(g_l.lock, 0, sizeof g_l.lock);
    __mtn_lock_mutex_lock(g_l.lock);
    assert(__mtn_lock_mutex_trylock(g_l.lock) == 0);
    __mtn_lock_mutex_unlock(g_l.lock);
    assert(__mtn_lock_mutex_trylock(g_l.lock) == 1);
    __mtn_lock_mutex_unlock(g_l.lock);
}

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    run_locks();
    puts("locks ok");
    run_try();
    size_t caps[] = {1, 3, 64, 0};
    for (int k = 0; k < 4; k++) {
        run_mpmc(caps[k], 1, 1, 20000);
        run_mpmc(caps[k], 4, 4, 5000);
        run_mpmc(caps[k], 8, 8, 2000);
        run_close_and_drop(caps[k]);
    }
    puts("channels ok");
    run_select_order();
    for (int k = 0; k < 4; k++) {
        run_select(caps[k], 2, 1, 20000);
        run_select(caps[k], 2, 4, 3000);
        run_select(caps[k], 3, 4, 2000);
        run_select(caps[k], 3, 8, 1000);
    }
    puts("select ok");
    return 0;
}

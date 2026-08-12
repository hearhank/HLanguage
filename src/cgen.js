// H 语言编译后端（C 目标）——从 AST 生成 C 源码，经 zig cc / gcc 编译为原生二进制
// 支持：struct（块）/enum + match/动态数组 [T]/class（树：堆分配 + 生命周期 + move + 静态派发）
// 树在 C 中 = 指针（Type*）：构造 h_new_Type / 作用域退出 h_free_Type / 方法静态派发 Type_method(self,...)
// 不支持的（并发/error/ref 指针/ref 字段/顶层语句）编译时拒绝

const { parse } = require("./parser");

/* 并发运行时：Windows Fiber / POSIX ucontext 协程 + M:N worker 线程（H_THREADS=1 时与求值器单线程调度一致） */
const CONCURRENCY_RUNTIME = `
/* ---------- 并发运行时：跨平台协程 + M:N worker 线程 ---------- */
#ifdef _WIN32
#include <windows.h>
#else
#include <pthread.h>
#include <ucontext.h>
#include <unistd.h>
#endif
#ifndef H_THREADS
#define H_THREADS 1
#endif
#ifndef H_TASK_STACK
#define H_TASK_STACK (64 * 1024)
#endif
/* 平台抽象：锁 / 条件变量 / 原子计数 */
#ifdef _WIN32
#define h_lock CRITICAL_SECTION
#define h_cond CONDITION_VARIABLE
#define h_lock_init(l) InitializeCriticalSection(l)
#define h_lock_lock(l) EnterCriticalSection(l)
#define h_lock_unlock(l) LeaveCriticalSection(l)
#define h_cond_init(c) InitializeConditionVariable(c)
#define h_cond_wait(c, l) SleepConditionVariableCS(c, l, INFINITE)
#define h_cond_wake(c) WakeConditionVariable(c)
#define h_cond_wakeall(c) WakeAllConditionVariable(c)
#define h_atomic_inc(x) InterlockedIncrement(&(x))
#define h_atomic_dec(x) InterlockedDecrement(&(x))
#else
#define h_lock pthread_mutex_t
#define h_cond pthread_cond_t
#define h_lock_init(l) pthread_mutex_init(l, NULL)
#define h_lock_lock(l) pthread_mutex_lock(l)
#define h_lock_unlock(l) pthread_mutex_unlock(l)
#define h_cond_init(c) pthread_cond_init(c, NULL)
#define h_cond_wait(c, l) pthread_cond_wait(c, l)
#define h_cond_wake(c) pthread_cond_signal(c)
#define h_cond_wakeall(c) pthread_cond_broadcast(c)
#define h_atomic_inc(x) __atomic_add_fetch(&(x), 1, __ATOMIC_SEQ_CST)
#define h_atomic_dec(x) __atomic_sub_fetch(&(x), 1, __ATOMIC_SEQ_CST)
#endif
typedef struct h_task h_task;
struct h_task {
#ifdef _WIN32
  void* fiber;
#else
  ucontext_t ctx;
  void* stack;
#endif
  h_task* next;
  int suspended;
  int done;
  void* send_val;
  void* recv_val;
  void (*entry)(void*);
  void* arg;
  struct h_worker* owner;
};
typedef struct h_wait h_wait;
struct h_wait {
  int type;
  h_task* task;
  void* value;
  h_wait* next;
};
typedef struct h_spawn_req {
  void (*entry)(void*);
  void* arg;
  struct h_spawn_req* next;
} h_spawn_req;
typedef struct h_chan {
  int cap, count, head, tail;
  void** buf;
  h_lock lock;
  h_wait* waits;
} h_chan;
typedef struct h_worker {
  void* sched_ctx;
  void* thread;
  h_task* ready_head;
  h_task* ready_tail;
  struct h_spawn_req* spawn_head;
  struct h_spawn_req* spawn_tail;
  h_lock lock;
  h_cond cv;
  int shutdown;
} h_worker;

static h_lock g_out_lock;
static h_worker* g_workers = NULL;
static int g_nworkers = H_THREADS;
static volatile long g_active = 0;
static int g_spawn_round = 0;
#if H_THREADS == 1
static h_chan** g_chans = NULL;
static int g_chan_count = 0, g_chan_cap = 0;
#endif

/* 平台协程原语：当前任务 / 建删 / 切换（任务→调度器、调度器→任务）/ 主线程调度器初始化 */
#ifdef _WIN32
static void WINAPI h_task_enter(void* param);
static h_task* h_cur_task(void) { return (h_task*)GetFiberData(); }
static void h_ctx_create_task(h_task* t) { t->fiber = CreateFiber(0, h_task_enter, t); }
static void h_ctx_delete_task(h_task* t) { DeleteFiber(t->fiber); }
static void h_ctx_switch_to_sched(void) { SwitchToFiber(h_cur_task()->owner->sched_ctx); }
static void h_ctx_switch_to_task(h_worker* w, h_task* t) { SwitchToFiber(t->fiber); }
static void h_main_thread_ctx_init(h_worker* w) { w->sched_ctx = ConvertThreadToFiber(NULL); }
#else
static void h_task_enter(void);
static __thread h_task* g_cur = NULL;
static __thread h_worker* g_worker = NULL;
static h_task* h_cur_task(void) { return g_cur; }
static void h_ctx_create_task(h_task* t) {
  t->stack = malloc(H_TASK_STACK);
  getcontext(&t->ctx);
  t->ctx.uc_stack.ss_sp = t->stack;
  t->ctx.uc_stack.ss_size = H_TASK_STACK;
  t->ctx.uc_link = NULL;
  makecontext(&t->ctx, h_task_enter, 0);
}
static void h_ctx_delete_task(h_task* t) { free(t->stack); }
static void h_ctx_switch_to_sched(void) { h_task* t = g_cur; swapcontext(&t->ctx, (ucontext_t*)g_worker->sched_ctx); }
static void h_ctx_switch_to_task(h_worker* w, h_task* t) { g_cur = t; swapcontext((ucontext_t*)w->sched_ctx, &t->ctx); }
static void h_main_thread_ctx_init(h_worker* w) { w->sched_ctx = malloc(sizeof(ucontext_t)); getcontext((ucontext_t*)w->sched_ctx); }
#endif
#ifdef _WIN32
static void* h_thread_create(LPTHREAD_START_ROUTINE fn, void* arg) {
  return CreateThread(NULL, 0, fn, arg, 0, NULL);
}
#else
static void* h_thread_create(void* (*fn)(void*), void* arg) {
  pthread_t* t = (pthread_t*)malloc(sizeof(pthread_t));
  pthread_create(t, NULL, fn, arg);
  return (void*)t;
}
#endif
static void h_thread_join(void* t) {
#ifdef _WIN32
  WaitForSingleObject((HANDLE)t, INFINITE);
#else
  pthread_join(*(pthread_t*)t, NULL);
  free(t);
#endif
}

static void h_print_lock(void) { h_lock_lock(&g_out_lock); }
static void h_print_unlock(void) { h_lock_unlock(&g_out_lock); }
static void h_task_started(void) { h_atomic_inc(g_active); }
static void h_task_finished(void) {
  if (h_atomic_dec(g_active) == 0) {
    for (int i = 0; i < g_nworkers; i++) {
      h_lock_lock(&g_workers[i].lock);
      g_workers[i].shutdown = 1;
      h_lock_unlock(&g_workers[i].lock);
      h_cond_wakeall(&g_workers[i].cv);
    }
  }
}
static h_chan* h_chan_new(int cap) {
  h_chan* c = (h_chan*)malloc(sizeof(h_chan));
  c->cap = cap < 1 ? 1 : cap; c->count = 0; c->head = 0; c->tail = 0;
  c->buf = (void**)malloc(sizeof(void*) * c->cap);
  h_lock_init(&c->lock);
  c->waits = NULL;
#if H_THREADS == 1
  if (g_chan_count == g_chan_cap) {
    g_chan_cap = g_chan_cap ? g_chan_cap * 2 : 8;
    g_chans = (h_chan**)realloc(g_chans, g_chan_cap * sizeof(h_chan*));
  }
  g_chans[g_chan_count++] = c;
#endif
  return c;
}
/* 唤醒最早等待者（1=send 等待者被腾位，2=recv 等待者被供值）；须持 channel 锁调用 */
static void h_chan_wake(h_chan* c, int type) {
  h_wait** pp = &c->waits;
  while (*pp) {
    if ((*pp)->type == type) {
      h_wait* w = *pp;
      *pp = w->next;
      h_task* wt = w->task;
      if (type == 1) { c->buf[c->tail] = w->value; c->tail = (c->tail + 1) % c->cap; c->count++; }
      else { wt->recv_val = c->buf[c->head]; c->head = (c->head + 1) % c->cap; c->count--; }
      free(w);
      wt->suspended = 0;
      h_worker* wker = wt->owner;
      h_lock_lock(&wker->lock);
      wt->next = NULL;
      if (wker->ready_tail) wker->ready_tail->next = wt; else wker->ready_head = wt;
      wker->ready_tail = wt;
      h_lock_unlock(&wker->lock);
      h_cond_wake(&wker->cv);
      return;
    }
    pp = &(*pp)->next;
  }
}
#ifdef _WIN32
static void WINAPI h_task_enter(void* param) {
  h_task* t = (h_task*)param;
#else
static void h_task_enter(void) {
  h_task* t = g_cur;
#endif
  t->entry(t->arg);
  t->done = 1;
  h_task_finished();
  h_ctx_switch_to_sched();
}
static void h_yield(void) { h_ctx_switch_to_sched(); }
static int h_chan_has_wait(h_chan* c, int type) {
  for (h_wait* w = c->waits; w; w = w->next) if (w->type == type) return 1;
  return 0;
}
static int h_chan_send(h_chan* c, void* v) {
  h_task* t = h_cur_task();
  h_lock_lock(&c->lock);
  if (c->count < c->cap) {
    c->buf[c->tail] = v; c->tail = (c->tail + 1) % c->cap; c->count++;
#if H_THREADS > 1
    h_chan_wake(c, 2);
#endif
    h_lock_unlock(&c->lock);
    return 1;
  }
  h_wait* w = (h_wait*)malloc(sizeof(h_wait));
  w->type = 1; w->task = t; w->value = v; w->next = NULL;
  h_wait** pp = &c->waits;
  while (*pp) pp = &(*pp)->next;
  *pp = w;
  t->suspended = 1;
  h_lock_unlock(&c->lock);
  h_ctx_switch_to_sched();
  return 1;
}
static void* h_chan_recv(h_chan* c) {
  h_task* t = h_cur_task();
  h_lock_lock(&c->lock);
  if (c->count > 0) {
    void* v = c->buf[c->head]; c->head = (c->head + 1) % c->cap; c->count--;
#if H_THREADS > 1
    h_chan_wake(c, 1);
#endif
    h_lock_unlock(&c->lock);
    return v;
  }
  h_wait* w = (h_wait*)malloc(sizeof(h_wait));
  w->type = 2; w->task = t; w->value = NULL; w->next = NULL;
  h_wait** pp = &c->waits;
  while (*pp) pp = &(*pp)->next;
  *pp = w;
  t->suspended = 2;
  h_lock_unlock(&c->lock);
  h_ctx_switch_to_sched();
  return t->recv_val;
}
/* spawn：请求投递到 worker（round-robin）；协程须在创建线程运行，故由 worker 线程自建 */
static void h_spawn(void (*entry)(void*), void* arg) {
  h_worker* w = &g_workers[g_spawn_round++ % g_nworkers];
  h_spawn_req* r = (h_spawn_req*)malloc(sizeof(h_spawn_req));
  r->entry = entry; r->arg = arg; r->next = NULL;
  h_task_started();
  h_lock_lock(&w->lock);
  if (w->spawn_tail) w->spawn_tail->next = r; else w->spawn_head = r;
  w->spawn_tail = r;
  h_lock_unlock(&w->lock);
  h_cond_wake(&w->cv);
}
/* 调度循环：处理 spawn 请求（本线程建协程）→ 跑就绪协程；挂起者等唤醒、yield 者重入队 */
static void h_sched_loop(h_worker* w) {
#if H_THREADS == 1
  /* 单线程：与求值器单线程调度逐字一致（FIFO + 空转延迟唤醒） */
  while (1) {
    h_lock_lock(&w->lock);
    h_spawn_req* r = w->spawn_head;
    w->spawn_head = w->spawn_tail = NULL;
    h_task* newt = NULL, *newtail = NULL;
    while (r) {
      h_spawn_req* nx = r->next;
      h_task* t = (h_task*)malloc(sizeof(h_task));
      t->entry = r->entry; t->arg = r->arg; t->done = 0; t->suspended = 0; t->next = NULL;
      t->owner = w;
      h_ctx_create_task(t);
      if (newtail) newtail->next = t; else newt = t;
      newtail = t;
      free(r);
      r = nx;
    }
    if (w->ready_tail) w->ready_tail->next = newt; else w->ready_head = newt;
    if (newtail) w->ready_tail = newtail;
    h_lock_unlock(&w->lock);
    while (1) {
      h_lock_lock(&w->lock);
      h_task* t = w->ready_head;
      if (t) { w->ready_head = t->next; if (!w->ready_head) w->ready_tail = NULL; }
      h_lock_unlock(&w->lock);
      if (!t) break;
      h_ctx_switch_to_task(w, t);
      if (t->done) { h_ctx_delete_task(t); free(t); continue; }
      if (!t->suspended) {
        h_lock_lock(&w->lock);
        t->next = NULL;
        if (w->ready_tail) w->ready_tail->next = t; else w->ready_head = t;
        w->ready_tail = t;
        h_lock_unlock(&w->lock);
      }
    }
    /* 空转：延迟唤醒（旧版语义：recv 等待者先、send 等待者后） */
    int woke = 0;
    for (int i = 0; i < g_chan_count; i++) {
      h_chan* c = g_chans[i];
      while (h_chan_has_wait(c, 2) && c->count > 0) { h_chan_wake(c, 2); woke = 1; }
      while (h_chan_has_wait(c, 1) && c->count < c->cap) { h_chan_wake(c, 1); woke = 1; }
    }
    if (!woke && !w->spawn_head) break;
  }
#else
  /* 多线程：即时唤醒（send/recv 成功即唤醒跨线程等待者），空转 condvar 等待 */
  while (1) {
    h_lock_lock(&w->lock);
    while (!w->ready_head && !w->spawn_head && !w->shutdown) h_cond_wait(&w->cv, &w->lock);
    h_spawn_req* r = w->spawn_head;
    w->spawn_head = w->spawn_tail = NULL;
    h_task* newt = NULL, *newtail = NULL;
    while (r) {
      h_spawn_req* nx = r->next;
      h_task* t = (h_task*)malloc(sizeof(h_task));
      t->entry = r->entry; t->arg = r->arg; t->done = 0; t->suspended = 0; t->next = NULL;
      t->owner = w;
      h_ctx_create_task(t);
      if (newtail) newtail->next = t; else newt = t;
      newtail = t;
      free(r);
      r = nx;
    }
    if (w->ready_tail) w->ready_tail->next = newt; else w->ready_head = newt;
    if (newtail) w->ready_tail = newtail;
    if (!w->ready_head) { h_lock_unlock(&w->lock); break; }
    h_task* t = w->ready_head;
    w->ready_head = t->next;
    if (!w->ready_head) w->ready_tail = NULL;
    h_lock_unlock(&w->lock);
    h_ctx_switch_to_task(w, t);
    if (t->done) { h_ctx_delete_task(t); free(t); continue; }
    if (!t->suspended) {
      h_lock_lock(&w->lock);
      t->next = NULL;
      if (w->ready_tail) w->ready_tail->next = t; else w->ready_head = t;
      w->ready_tail = t;
      h_lock_unlock(&w->lock);
    }
  }
#endif
}
#ifdef _WIN32
static DWORD WINAPI h_worker_main(void* param) {
  h_worker* w = (h_worker*)param;
  w->sched_ctx = ConvertThreadToFiber(NULL);
  h_sched_loop(w);
  return 0;
}
#else
static void* h_worker_main(void* param) {
  h_worker* w = (h_worker*)param;
  g_worker = w;
  w->sched_ctx = malloc(sizeof(ucontext_t));
  getcontext((ucontext_t*)w->sched_ctx);
  h_sched_loop(w);
  free(w->sched_ctx);
  return NULL;
}
#endif
static void h_runtime_init(void) {
  h_lock_init(&g_out_lock);
  g_nworkers = H_THREADS < 1 ? 1 : H_THREADS;
  g_workers = (h_worker*)calloc(g_nworkers, sizeof(h_worker));
  for (int i = 0; i < g_nworkers; i++) {
    h_lock_init(&g_workers[i].lock);
    h_cond_init(&g_workers[i].cv);
    g_workers[i].thread = NULL;
  }
  for (int i = 1; i < g_nworkers; i++) {
    g_workers[i].thread = h_thread_create(h_worker_main, &g_workers[i]);
  }
}
static void h_runtime_join(void) {
  for (int i = 1; i < g_nworkers; i++) h_thread_join(g_workers[i].thread);
}
`;

/* 字节化（to_bytes/from_bytes）：可逆自描述 JSON（与求值器 JSON.stringify 逐字节一致） */
/* 字节化（to_bytes/from_bytes）：可逆自描述 JSON（与求值器 JSON.stringify 逐字节一致） */
const JSON_RUNTIME = `
/* ---------- 字节化运行时：字符串缓冲 + 最小 JSON 解析器（字符比较用十六进制，避免转义层叠） ---------- */
typedef struct { char* data; size_t len, cap; } h_strbuf;
static void h_sb_grow(h_strbuf* b, size_t need) {
  if (b->len + need + 1 <= b->cap) return;
  b->cap = b->cap ? b->cap * 2 : 64;
  while (b->len + need + 1 > b->cap) b->cap *= 2;
  b->data = (char*)realloc(b->data, b->cap);
}
static void h_sb_puts(h_strbuf* b, const char* s) {
  size_t n = strlen(s);
  h_sb_grow(b, n);
  memcpy(b->data + b->len, s, n);
  b->len += n;
}
static void h_sb_char(h_strbuf* b, char c) { h_sb_grow(b, 1); b->data[b->len++] = c; }
static void h_sb_num(h_strbuf* b, double d) {
  char buf[64];
  int prec;
  for (prec = 17; prec >= 1; prec--) {
    snprintf(buf, sizeof buf, "%.*g", prec, d);
    double back;
    if (sscanf(buf, "%lf", &back) == 1 && back == d) break;
  }
  h_sb_puts(b, buf);
}
static void h_sb_json_str(h_strbuf* b, const char* s) {
  h_sb_char(b, 0x22);
  for (const char* p = s; p && *p; p++) {
    unsigned char c = (unsigned char)*p;
    switch (c) {
      case 0x22: h_sb_puts(b, "\\x5c\\x22"); break;
      case 0x5c: h_sb_puts(b, "\\x5c\\x5c"); break;
      case 0x0a: h_sb_puts(b, "\\x5c\\x6e"); break;
      case 0x09: h_sb_puts(b, "\\x5c\\x74"); break;
      case 0x0d: h_sb_puts(b, "\\x5c\\x72"); break;
      case 0x08: h_sb_puts(b, "\\x5c\\x62"); break;
      case 0x0c: h_sb_puts(b, "\\x5c\\x66"); break;
      default:
        if (c < 0x20) { char u[8]; snprintf(u, sizeof u, "\\x5c\\x75%04x", c); h_sb_puts(b, u); }
        else h_sb_char(b, (char)c);
    }
  }
  h_sb_char(b, 0x22);
}
static char* h_sb_done(h_strbuf* b) { h_sb_grow(b, 1); b->data[b->len] = 0; return b->data; }

typedef struct h_json h_json;
typedef struct { char* key; h_json* val; } h_jpair;
struct h_json {
  int kind;
  double num;
  char* str;
  h_jpair* obj; int obj_n, obj_cap;
  h_json** arr; int arr_n, arr_cap;
};
static h_json* h_json_new(int kind) { h_json* j = (h_json*)calloc(1, sizeof(h_json)); j->kind = kind; return j; }
static void h_json_skip(const char** p) { while (**p == 0x20 || **p == 0x09 || **p == 0x0a || **p == 0x0d) (*p)++; }
static void h_json_parse_str_body(const char** p, char** out) {
  h_strbuf b = {0};
  while (**p && **p != 0x22) {
    if (**p == 0x5c) {
      (*p)++;
      char c = **p;
      if (c == 0x75) {
        unsigned code = 0;
        for (int i = 0; i < 4; i++) { (*p)++; char h = **p; code = code * 16 + (unsigned)((h >= 0x30 && h <= 0x39) ? h - 0x30 : (h >= 0x61 && h <= 0x66) ? h - 0x61 + 10 : h - 0x41 + 10); }
        if (code < 0x80) h_sb_char(&b, (char)code);
        else if (code < 0x800) { h_sb_char(&b, (char)(0xC0 | (code >> 6))); h_sb_char(&b, (char)(0x80 | (code & 0x3F))); }
        else { h_sb_char(&b, (char)(0xE0 | (code >> 12))); h_sb_char(&b, (char)(0x80 | ((code >> 6) & 0x3F))); h_sb_char(&b, (char)(0x80 | (code & 0x3F))); }
      } else {
        h_sb_char(&b, c == 0x6e ? 0x0a : c == 0x74 ? 0x09 : c == 0x72 ? 0x0d : c == 0x62 ? 0x08 : c == 0x66 ? 0x0c : c);
        (*p)++;
      }
    } else { h_sb_char(&b, **p); (*p)++; }
  }
  if (**p == 0x22) (*p)++;
  *out = h_sb_done(&b);
}
static h_json* h_json_parse_value(const char** p);
static h_json* h_json_parse_obj(const char** p) {
  h_json* j = h_json_new(0);
  (*p)++;
  while (1) {
    h_json_skip(p);
    if (**p == 0x7d) { (*p)++; return j; }
    if (**p != 0x22) return j;
    (*p)++;
    char* key = NULL;
    h_json_parse_str_body(p, &key);
    h_json_skip(p);
    if (**p == 0x3a) (*p)++;
    h_json_skip(p);
    if (j->obj_n == j->obj_cap) { j->obj_cap = j->obj_cap ? j->obj_cap * 2 : 4; j->obj = (h_jpair*)realloc(j->obj, j->obj_cap * sizeof(h_jpair)); }
    j->obj[j->obj_n].key = key;
    j->obj[j->obj_n].val = h_json_parse_value(p);
    j->obj_n++;
    h_json_skip(p);
    if (**p == 0x2c) { (*p)++; continue; }
    if (**p == 0x7d) { (*p)++; return j; }
    return j;
  }
}
static h_json* h_json_parse_arr(const char** p) {
  h_json* j = h_json_new(1);
  (*p)++;
  while (1) {
    h_json_skip(p);
    if (**p == 0x5d) { (*p)++; return j; }
    if (j->arr_n == j->arr_cap) { j->arr_cap = j->arr_cap ? j->arr_cap * 2 : 4; j->arr = (h_json**)realloc(j->arr, j->arr_cap * sizeof(h_json*)); }
    j->arr[j->arr_n++] = h_json_parse_value(p);
    h_json_skip(p);
    if (**p == 0x2c) { (*p)++; continue; }
    if (**p == 0x5d) { (*p)++; return j; }
    return j;
  }
}
static h_json* h_json_parse_value(const char** p) {
  h_json_skip(p);
  char c = **p;
  if (c == 0x7b) return h_json_parse_obj(p);
  if (c == 0x5b) return h_json_parse_arr(p);
  if (c == 0x22) { (*p)++; h_json* j = h_json_new(3); h_json_parse_str_body(p, &j->str); return j; }
  if (c == 0x74) { *p += 4; return h_json_new(4); }
  if (c == 0x66) { *p += 5; return h_json_new(5); }
  if (c == 0x6e) { *p += 4; return h_json_new(6); }
  { char* end = NULL; double d = strtod(*p, &end); h_json* j = h_json_new(2); j->num = d; *p = end; return j; }
}
static h_json* h_json_find(h_json* j, const char* key) {
  if (!j || j->kind != 0) return NULL;
  for (int i = 0; i < j->obj_n; i++) if (strcmp(j->obj[i].key, key) == 0) return j->obj[i].val;
  return NULL;
}
static double h_jnum(h_json* j, const char* key) { h_json* v = h_json_find(j, key); return v && v->kind == 2 ? v->num : 0; }
static const char* h_jstr(h_json* j, const char* key) { h_json* v = h_json_find(j, key); return v && v->kind == 3 ? v->str : NULL; }
static void h_json_free(h_json* j) {
  if (!j) return;
  for (int i = 0; i < j->obj_n; i++) { free(j->obj[i].key); h_json_free(j->obj[i].val); }
  free(j->obj);
  for (int i = 0; i < j->arr_n; i++) h_json_free(j->arr[i]);
  free(j->arr);
  free(j->str);
  free(j);
}
`;

function typeName(t) {
  if (t.type === "NamedType") return t.name;
  if (t.type === "ArrayType") return "[" + typeName(t.elem) + "]";
  if (t.type === "SliceType") return "[]" + typeName(t.elem);
  if (t.type === "TupleType") return t.named
    ? "(" + t.items.map(i => i.name + ": " + typeName(i.type)).join(", ") + ")"
    : "(" + t.items.map(i => typeName(i.type)).join(", ") + ")";
  if (t.type === "GenericType") return t.name;
  return "?";
}

/* 元组类型名解析："(u64, (f64, Str))" → ["u64", "(f64, Str)"]（跳过嵌套括号）
   命名元组首元素含 ": "；位置元组元素即类型名 */
function tupleElemTypes(tname) {
  const inner = tname.slice(1, -1);
  const out = [];
  let depth = 0, cur = "";
  for (const ch of inner) {
    if (ch === "(") depth++;
    if (ch === ")") depth--;
    if (ch === "," && depth === 0) { out.push(cur.trim()); cur = ""; }
    else cur += ch;
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
}
function tupleIsNamed(tname) {
  const first = tupleElemTypes(tname)[0] || "";
  return first.includes(": ");
}
function tupleCName(tname) {
  let h = 0;
  for (const ch of tname) h = (h * 31 + ch.charCodeAt(0)) | 0;
  return "tup_" + (h >>> 0).toString(36);
}

function cType(tname) {
  switch (tname) {
    case "u64": return "unsigned long long";
    case "f64": return "double";
    case "bool": return "bool";
    case "Str": return "const char*";
    case "void": return "void";
    case "Channel": return "h_chan*";
    default: {
      if (tname.startsWith("[") && tname.endsWith("]")) return shortName(tname.slice(1, -1)) + "_Array";
      if (tname.startsWith("[]")) return shortName(tname.slice(2)) + "_Slice";
      if (tname.startsWith("(") && tname.endsWith(")")) return tupleCName(tname);
      return tname;   // struct/class 类型名（typedef 已定义）
    }
  }
}
function shortName(tname) { return tname; }

/* 标量/复合值的打印语句（双后端一致：对齐求值器 valueToStr） */
function scalarPrint(t, e, ctx) {
  if (t === "f64") return `h_print_f64(${e});`;
  if (t === "u64") return `printf("%llu", ${e});`;
  if (t === "bool") return `printf("%s", (${e}) ? "true" : "false");`;
  if (t === "Str") return 'printf("\\\"%s\\\"", ' + e + ');';
  if (t.startsWith("[") && t.endsWith("]")) return `h_print_${shortName(t.slice(1, -1))}_Array(&${e});`;
  if (t.startsWith("[]")) return `h_print_${shortName(t.slice(2))}_Slice(&${e});`;
  if (ctx.classes[t]) return `if (${e}) { h_print_${t}(${e}); } else { printf("null"); }`;   // ref 字段可能为 NULL（目标已销毁被通知置空）
  if (ctx.structs[t]) return `h_print_${t}(&${e});`;
  return `printf("?");`;   // 枚举等：占位（示例避开）
}

/* 类方法表提升（与求值器一致的简化版：自己 + 导入深度传递，hide/alias） */
function computeClassMethods(classes) {
  const cache = {};
  const resolve = (clsName, visiting) => {
    if (cache[clsName]) return cache[clsName];
    if (visiting.has(clsName)) return cache[clsName] || {};
    visiting.add(clsName);
    const cls = classes[clsName];
    const table = {};
    for (const m of cls.methods) table[m.name] = { source: clsName, name: m.name };
    for (const imp of cls.imports) {
      const sub = resolve(imp.name, visiting);
      for (const [n, entry] of Object.entries(sub)) if (!table[n]) table[n] = entry;
    }
    for (const h of cls.hides) {
      if (h.path.parts.length >= 2) {
        const [src, mname] = h.path.parts;
        if (table[mname] && table[mname].source === src) delete table[mname];
      }
    }
    for (const al of cls.aliases) {
      const [src, mname] = al.path.parts;
      const sub = cache[src] || resolve(src, visiting);
      if (sub[mname]) table[al.alias] = sub[mname];
    }
    visiting.delete(clsName);
    cache[clsName] = table;
    return table;
  };
  const out = {};
  for (const n of Object.keys(classes)) out[n] = resolve(n, new Set());
  return out;
}

function genC(ast, threads) {
  const enums = {}, rets = {}, structFields = {}, classes = {}, arrayElems = new Set(), sliceElems = new Set(), tupleDefs = {}, paramKinds = {}, paramTypes = {}, retKinds = {}, globals = {};
  const spawns = [];
  const collectTypes = (t) => {
    if (!t) return;
    if (t.type === "ArrayType") { arrayElems.add(typeName(t.elem)); collectTypes(t.elem); }
    if (t.type === "SliceType") { sliceElems.add(typeName(t.elem)); collectTypes(t.elem); }
    if (t.type === "TupleType") {
      const tn = typeName(t);
      if (!(tn in tupleDefs)) tupleDefs[tn] = { cn: tupleCName(tn), tname: tn };
      t.items.forEach(i => collectTypes(i.type));
    }
  };
  const literalElemType = (items) => {
    for (const it of items) {
      if (it.type === "Literal") {
        if (typeof it.value === "number") return it.kind === "float" ? "f64" : "u64";
        if (typeof it.value === "string") return "Str";
      }
      if (it.type === "ArrayLiteral") return "[" + literalElemType(it.items) + "]";
    }
    return "u64";
  };
  // 字面量可静态推断的类型（仅 Literal/数组/元组；含表达式返回 null）
  const litTypeOf = (x) => {
    if (!x) return null;
    if (x.type === "Literal") {
      if (typeof x.value === "number") return x.kind === "float" ? "f64" : "u64";
      if (typeof x.value === "string") return "Str";
      if (typeof x.value === "boolean") return "bool";
    }
    if (x.type === "ArrayLiteral") return "[" + literalElemType(x.items) + "]";
    if (x.type === "TupleLit") return tupleLitType(x);
    return null;
  };
  const tupleLitType = (e) => {
    const ts = e.items.map(it => litTypeOf(it.expr));
    if (ts.some(x => x === null)) return null;
    return e.named ? "(" + e.items.map((it, i) => it.name + ": " + ts[i]).join(", ") + ")" : "(" + ts.join(", ") + ")";
  };
  const scanExpr = (e) => {
    if (!e || typeof e !== "object") return;
    if (e.type === "ArrayLiteral") { arrayElems.add(literalElemType(e.items)); e.items.forEach(scanExpr); return; }
    if (e.type === "TupleLit") {
      const t = tupleLitType(e);
      if (t && !(t in tupleDefs)) tupleDefs[t] = { cn: tupleCName(t), tname: t };
      e.items.forEach(i => scanExpr(i.expr));
      return;
    }
    for (const v of Object.values(e)) {
      if (Array.isArray(v)) v.forEach(scanExpr);
      else if (v && typeof v === "object" && v.type) scanExpr(v);
    }
  };
  for (const d of ast.decls) {
    if (d.type === "EnumDecl") enums[d.name] = d.variants;
    if (d.type === "FunDecl") {
      rets[d.name] = d.ret ? typeName(d.ret.rtype) : "void";
      paramKinds[d.name] = d.params.map(p => p.kind);
      paramTypes[d.name] = d.params.map(p => typeName(p.ptype));
      retKinds[d.name] = d.ret ? d.ret.kind : "val";
      collectTypes(d.ret && d.ret.rtype);
      for (const p of d.params) collectTypes(p.ptype);
      d.body.stmts.forEach(scanExpr);
    }
    if (d.type === "StructDecl") {
      structFields[d.name] = {};
      for (const f of d.fields) {
        structFields[d.name][f.name] = typeName(f.fieldType);
        collectTypes(f.fieldType);
      }
    }
    if (d.type === "ClassDecl") {
      const fields = [];
      for (const f of d.fields) { fields.push({ name: f.name, type: typeName(f.fieldType), fieldType: f.fieldType }); collectTypes(f.fieldType); }
      classes[d.name] = { fields, methods: d.methods, imports: d.imports, hides: d.hides, aliases: d.aliases };
      for (const m of d.methods) {
        paramKinds[d.name + "_" + m.name] = m.params.map(p => p.kind);
        paramTypes[d.name + "_" + m.name] = m.params.map(p => typeName(p.ptype));
        retKinds[d.name + "_" + m.name] = m.ret ? m.ret.kind : "val";
        collectTypes(m.ret && m.ret.rtype);
        for (const p of m.params) collectTypes(p.ptype);
        m.body.stmts.forEach(scanExpr);
      }
    }
  }
  const ctx = { enums, rets, structs: structFields, classes, classMethods: computeClassMethods(classes), paramKinds, paramTypes, retKinds, globals, tuples: tupleDefs };
  // error 返回类型结构体：struct h_err_T { ok, val, name }（未处理时调用点 fail-fast，与求值器一致）
  const errTypes = {};
  const collectErr = (ret) => {
    if (ret && ret.kind === "error") {
      const T = ret.rtype ? typeName(ret.rtype) : "void";
      if (!(T in errTypes)) errTypes[T] = T === "void" ? null : (ctx.classes[T] ? cType(T) + "*" : cType(T));
    }
  };
  for (const d of ast.decls) {
    if (d.type === "FunDecl") collectErr(d.ret);
    if (d.type === "ClassDecl") for (const m of d.methods) collectErr(m.ret);
  }
  const errorDefs = Object.keys(errTypes).map(T =>
    T === "void"
      ? "typedef struct { bool ok; const char* name; } h_err_void;"
      : `typedef struct { bool ok; ${errTypes[T]} val; const char* name; } h_err_${shortName(T)};`
  );
  const arrayDefs = [...arrayElems].map(e =>
    `typedef struct { unsigned long long len; ${cType(e)}* data; } ${shortName(e)}_Array;`
  ).join("\n");
  const sliceDefs = [...sliceElems].map(e =>
    `typedef struct { unsigned long long len; ${cType(e)}* data; } ${shortName(e)}_Slice;`
  ).join("\n");
  const tupleStructDefs = Object.values(tupleDefs).map(({ cn, tname }) => {
    const elems = tupleElemTypes(tname);
    const named = tupleIsNamed(tname);
    const names = named ? elems.map(x => x.split(": ")[0]) : elems.map((_, i) => "_" + i);
    const types = named ? elems.map(x => x.split(": ")[1]) : elems;
    const fields = types.map((t, i) => `  ${cType(t)} ${names[i]};`).join("\n");
    return `struct ${cn} {\n${fields}\n};`;
  }).join("\n");

  const structs = [], enumsDefs = [], classDefs = [], funcs = [];
  for (const d of ast.decls) {
    if (d.type === "StructDecl") structs.push(genStruct(d));
    else if (d.type === "EnumDecl") enumsDefs.push(genEnum(d, ctx));
    else if (d.type === "ClassDecl") classDefs.push(genClass(d, ctx));
    else if (d.type === "FunDecl") funcs.push(genFun(d, ctx, null));
    else if (d.type === "InterfaceDecl") { /* 契约，不生成 */ }
    else if (d.type === "GlobalDecl") {
      // 全局 Channel（原型：仅 Channel<T>；值暂只支持 u64）
      if (d.gtype.type === "GenericType" && d.gtype.name === "Channel") {
        const elem = d.gtype.args && d.gtype.args[0] ? typeName(d.gtype.args[0]) : "u64";
        let cap = 1;
        if (d.init && d.init.type === "CallExpr" && d.init.callee.type === "Ident" && d.init.callee.name === "Channel" && d.init.args[0] && d.init.args[0].type === "Literal") {
          cap = d.init.args[0].value;
        }
        globals[d.name] = { elem, cap };
      } else {
        throw new Error("C 后端暂不支持非 Channel 的 global/访问模式——请用 h run");
      }
    }
    else if (d.type === "SpawnStmt") {
      const callee = d.callee;
      if (callee.type !== "CallExpr" || callee.callee.type !== "Ident") throw new Error("C 后端暂不支持复杂 spawn——请用 h run");
      spawns.push({ fname: callee.callee.name === "main" ? "h_main" : callee.callee.name, args: callee.args });
    }
    else if (d.type === "ExprStmt") {
      const isMainCall = d.expr.type === "CallExpr" && d.expr.callee.type === "Ident" && d.expr.callee.name === "main";
      if (!isMainCall) throw new Error("C 后端暂不支持顶层语句——请用 h run");
    }
  }
  // 打印函数（方案 b：与解释器 valueToStr 逐字一致）——struct/数组/class 全量生成
  const printFns = [];
  for (const n of Object.keys(structFields)) printFns.push(genPrintStruct(n, ctx));
  for (const e of arrayElems) printFns.push(genPrintArray(e, ctx));
  for (const e of sliceElems) printFns.push(genPrintSlice(e, ctx));
  for (const e of sliceElems) printFns.push(genSliceClone(e, ctx));
  for (const { cn, tname } of Object.values(tupleDefs)) printFns.push(genPrintTuple(cn, tname, ctx));
  for (const n of Object.keys(classes)) printFns.push(genPrintClass(n, ctx));
  const printProtos = printFns.map(f => f.split("{")[0].trim() + ";");
  // 字节化：per-type 序列化/反序列化（h_tb_/h_jrev_/h_to_bytes_/h_from_bytes_）
  const bytesFns = [];
  for (const [n, fs] of Object.entries(structFields)) {
    const farr = Object.entries(fs).map(([k, v]) => ({ name: k, type: v }));
    bytesFns.push(genTB(n, "block", farr, ctx), genRevStruct(n, farr, ctx), genToBytesEntry(n, `const ${n}* p`), genFromBytes(n, n));
  }
  for (const e of arrayElems) {
    const an = shortName(e) + "_Array";
    bytesFns.push(genTBArray(e, ctx), genRevArray(e, ctx), genToBytesEntry(an, `const ${an}* p`), genFromBytes(an, an));
  }
  for (const { cn, tname } of Object.values(tupleDefs)) {
    bytesFns.push(genTBTuple(cn, tname, ctx), genToBytesEntry(cn, `const ${cn}* p`));
  }
  for (const [n, cls] of Object.entries(classes)) {
    bytesFns.push(genTB(n, "tree", cls.fields, ctx), genRevClass(n, cls.fields, ctx), genToBytesEntry(n, `const ${n}* p`), genFromBytes(n, n + "*"));
  }
  for (const [n, vs] of Object.entries(enums)) {
    bytesFns.push(genTBEnum(n, vs), genRevEnum(n, vs), genToBytesEntry(n, `${n} p`), genFromBytes(n, n));
  }
  // 类型注册表（类型标签注册机制）：类型名 → 元数据，运行时可见（互操作/诊断入口）
  const regEntries = [];
  for (const n of Object.keys(structFields)) regEntries.push(`  { \"${n}\", sizeof(${n}) }`);
  for (const e of arrayElems) { const an = shortName(e) + "_Array"; regEntries.push(`  { \"${an}\", sizeof(${an}) }`); }
  for (const n of Object.keys(classes)) regEntries.push(`  { \"${n}\", sizeof(${n}) }`);
  for (const n of Object.keys(enums)) regEntries.push(`  { \"${n}\", sizeof(${n}) }`);
  const registry = [
    "/* 类型注册表（类型标签注册机制）：类型名 → 元数据 */",
    "typedef struct { const char* name; size_t size; } h_type_entry;",
    "static h_type_entry h_type_registry[] = {",
    regEntries.join(",\n"),
    "};",
    "static const int h_type_count = (int)(sizeof(h_type_registry) / sizeof(h_type_registry[0]));",
    "static const char* h_type_lookup(const char* name) { for (int i = 0; i < h_type_count; i++) if (strcmp(h_type_registry[i].name, name) == 0) return h_type_registry[i].name; return NULL; }",
    "",
  ].join("\n");
  // 类型顺序：前向声明（struct/class tag）→ enum → 数组 → struct 定义 → class 定义 → 打印/方法/函数
  const fwdDecls = [];
  for (const n of Object.keys(structFields)) fwdDecls.push(`typedef struct ${n} ${n};`);
  for (const n of Object.keys(classes)) fwdDecls.push(`typedef struct ${n} ${n};`);
  for (const { cn } of Object.values(tupleDefs)) fwdDecls.push(`typedef struct ${cn} ${cn};`);
  const typeDefs = structs.concat(genClassDecls(ast, ctx));
  // 带参 spawn：参数打包结构体（按函数签名）+ Fiber 入口 trampoline（解包 + free）
  const topScope = new Scope(null, ctx);
  const ctypeOf = (t) => ctx.classes[t] ? cType(t) + "*" : cType(t);
  const spawnStructs = new Map();
  for (const s of spawns) {
    if (!spawnStructs.has(s.fname)) spawnStructs.set(s.fname, s.args.map(a => inferType(a, topScope)));
  }
  const spawnCtxDefs = [...spawnStructs].filter(([, ts]) => ts.length).map(([fname, ts]) =>
    `typedef struct { ${ts.map((t, i) => `${ctypeOf(t)} a${i};`).join(" ")} } h_sp_ctx_${fname};`
  ).join("\n");
  const spawnTramps = [...spawnStructs].map(([fname, ts]) => {
    if (!ts.length) return `static void h_task_${fname}(void* arg) { ${fname}(); }`;
    const unpack = ts.map((_, i) => `c->a${i}`).join(", ");
    return `static void h_task_${fname}(void* arg) { h_sp_ctx_${fname}* c = (h_sp_ctx_${fname}*)arg; ${fname}(${unpack}); free(c); }`;
  }).join("\n");
  const topSpawns = spawns.map((s, idx) => {
    const ts = spawnStructs.get(s.fname);
    if (!ts.length) return `  h_spawn(h_task_${s.fname}, NULL);`;
    const vn = `c${idx}`;
    const init = s.args.map((a, i) => `  ${vn}->a${i} = ${genExpr(a, topScope)};`).join("\n");
    return `  h_sp_ctx_${s.fname}* ${vn} = (h_sp_ctx_${s.fname}*)malloc(sizeof(h_sp_ctx_${s.fname}));\n${init}\n  h_spawn(h_task_${s.fname}, ${vn});`;
  }).join("\n");
  // 全局 Channel 声明 + 初始化
  const globalDecls = Object.keys(globals).map(n => `static h_chan* ${n};`).join("\n");
  const globalInitFun = ["static void h_global_init(void) {",
    ...Object.keys(globals).map(n => `  ${n} = h_chan_new(${globals[n].cap});`), "}", ""].join("\n");
  const body = [
    threads && threads > 1 ? `#define H_THREADS ${threads}` : "",
    '#include <stdio.h>',
    '#include <stdbool.h>',
    '#include <stdint.h>',
    '#include <stdlib.h>',
    '#include <string.h>',
    '',
    '/* 双向引用通知：树对象内嵌被引用链表，销毁时通知所有 ref 字段置 NULL */',
    'struct h_ref_link { void** pslot; struct h_ref_link* next; };',
    'static void h_ref_detach(struct h_ref_link** head, struct h_ref_link* ln) {',
    '  struct h_ref_link** p = head;',
    '  while (*p && *p != ln) p = &(*p)->next;',
    '  if (*p) *p = ln->next;',
    '  ln->next = NULL;',
    '}',
    '',
    'static void h_print_f64(double d) {',
    '  char buf[64];',
    '  int prec;',
    '  for (prec = 17; prec >= 1; prec--) {',
    '    snprintf(buf, sizeof buf, "%.*g", prec, d);',
    '    double back;',
    '    if (sscanf(buf, "%lf", &back) == 1 && back == d) break;',
    '  }',
    '  printf("%s", buf);',
    '}',
    '',
    CONCURRENCY_RUNTIME,
    JSON_RUNTIME,
  ];
  const hasMain = ctx.retKinds["main"] !== undefined;
  const mainErrT = hasMain && ctx.retKinds["main"] === "error" ? shortName(ctx.rets["main"] || "void") : null;
  const mainEntry = [
    `static void h_task_main_entry(void* arg) {`,
    hasMain ? (mainErrT
      ? `  h_err_${mainErrT} _he = h_main(); if (!_he.ok) { fprintf(stderr, "\\u274c error.%s（未处理）\\n", _he.name); h_task_finished(); exit(1); }`
      : "  h_main();")
      : "",
    topSpawns,
    `  h_task_finished();`,
    `}`,
  ].join("\n");
  const spawnGlue = [spawnCtxDefs, spawnTramps].filter(Boolean).join("\n");
  return body.concat(globalDecls ? [globalDecls, ""] : [], fwdDecls, [""], enumsDefs, arrayDefs ? [arrayDefs, ""] : [], sliceDefs ? [sliceDefs, ""] : [], tupleStructDefs ? [tupleStructDefs, ""] : [], typeDefs, [""],
    errorDefs, [""], printProtos, [""], printFns, [""], classDefs,
    registry ? [registry] : [],
    bytesFns ? [bytesFns.join("\n"), ""] : [], funcs,
    spawnGlue ? [spawnGlue, ""] : [], mainEntry ? [mainEntry, ""] : [], globalInitFun ? [globalInitFun] : [],
    ["int main(void) {",
      "  h_runtime_init();",
      "  h_global_init();",
      "  h_main_thread_ctx_init(&g_workers[0]);",
      "  h_spawn(h_task_main_entry, NULL);",
      "  h_sched_loop(&g_workers[0]);",
      "  h_runtime_join();",
      "  return 0;", "}", ""]).join("\n");
}

function genStruct(d) {
  const fields = d.fields.map(f => `  ${cType(typeName(f.fieldType))} ${f.name};`).join("\n");
  return `struct ${d.name} {\n${fields}\n};`;   // typedef 由 fwdDecls 提供（避免匿名 struct 重定义 typedef）
}
function genClassDecls(ast, ctx) {
  return ast.decls.filter(d => d.type === "ClassDecl").map(d => {
    const refs = d.fields.filter(f => f.fieldType.mutable);
    const fields = d.fields.map(f => {
      const ft = typeName(f.fieldType);
      const ct = f.fieldType.mutable ? cType(ft) + "*" : cType(ft);
      return `  ${ct} ${f.name};`;
    }).join("\n");
    const links = refs.map(f => `  struct h_ref_link _${f.name}_link;`).join("\n");
    return `struct ${d.name} {\n${fields}\n  struct h_ref_link* _refs;\n${links ? links + "\n" : ""}};`;
  });
}

/* ---------- 打印函数（与求值器 valueToStr 逐字一致） ---------- */
function genPrintStruct(name, ctx) {
  let body = "", first = true;
  for (const [fname, ftype] of Object.entries(ctx.structs[name])) {
    if (!first) body += `    printf(", ");\n`;
    first = false;
    body += `    printf("${fname}: ");\n    ` + scalarPrint(ftype, `p->${fname}`, ctx) + "\n";
  }
  return `static void h_print_${name}(const ${name}* p) {\n  printf("${name}{");\n${body}  printf("}");\n}`;
}
function genPrintClass(name, ctx) {
  let body = "", first = true;
  for (const f of ctx.classes[name].fields) {
    if (!first) body += `    printf(", ");\n`;
    first = false;
    body += `    printf("${f.name}: ");\n    ` + scalarPrint(f.type, `p->${f.name}`, ctx) + "\n";
  }
  return `static void h_print_${name}(const ${name}* p) {\n  printf("${name}{");\n${body}  printf("}");\n}`;
}
function genPrintArray(elem, ctx) {
  const an = shortName(elem) + "_Array";
  const el = scalarPrint(elem, "a->data[i]", ctx);
  return `static void h_print_${an}(const ${an}* a) {\n  printf("[");\n  for (unsigned long long i = 0; i < a->len; i++) {\n    if (i) printf(", ");\n    ${el}\n  }\n  printf("]");\n}`;
}
function genPrintSlice(elem, ctx) {
  const sn = shortName(elem) + "_Slice";
  const el = scalarPrint(elem, "s->data[i]", ctx);
  return `static void h_print_${sn}(const ${sn}* s) {\n  printf("[");\n  for (unsigned long long i = 0; i < s->len; i++) {\n    if (i) printf(", ");\n    ${el}\n  }\n  printf("]");\n}`;
}
function genPrintTuple(cn, tname, ctx) {
  const elems = tupleElemTypes(tname);
  const named = tupleIsNamed(tname);
  const names = named ? elems.map(x => x.split(": ")[0]) : elems.map((_, i) => "_" + i);
  const types = named ? elems.map(x => x.split(": ")[1]) : elems;
  const body = types.map((t, i) => {
    const pre = named ? `    printf("${names[i]}: ");\n    ` : "";
    return pre + scalarPrint(t, `p->${names[i]}`, ctx);
  }).join("\n    printf(\", \");\n");
  return `static void h_print_${cn}(const ${cn}* p) {\n  printf("(");\n${body}\n  printf(")");\n}`;
}
function genSliceClone(elem, ctx) {
  const sn = shortName(elem) + "_Slice";
  const an = shortName(elem) + "_Array";
  const et = cType(elem);
  const copy = elem === "Str"
    ? `r.data[i] = s->data[i] ? strdup(s->data[i]) : NULL;`
    : `r.data[i] = s->data[i];`;
  return `static ${an} h_slice_clone_${shortName(elem)}(const ${sn}* s) {\n  ${an} r = { .len = s->len, .data = s->len ? (${et}*)malloc(sizeof(${et}) * s->len) : NULL };\n  for (unsigned long long i = 0; i < s->len; i++) ${copy}\n  return r;\n}`;
}
function genEnum(d) {
  const names = d.variants.map(v => `  ${d.name}_${v}`).join(",\n");
  return `typedef enum {\n${names}\n} ${d.name};`;
}

/* ---------- 字节化（to_bytes/from_bytes）：per-type JSON 序列化/反序列化 ---------- */
function tbValue(t, e, ctx) {
  if (t === "f64" || t === "u64") return `h_sb_num(b, ${e});`;
  if (t === "bool") return `h_sb_puts(b, (${e}) ? \"true\" : \"false\");`;
  if (t === "Str") return `h_sb_json_str(b, ${e});`;
  if (t.startsWith("[") && t.endsWith("]")) return `h_tb_${shortName(t.slice(1, -1))}_Array(&${e}, b, 0);`;
  if (ctx.classes[t]) return `if (${e}) h_tb_${t}(${e}, b, 0); else h_sb_puts(b, \"null\");`;
  if (ctx.structs[t]) return `h_tb_${t}(&${e}, b, 0);`;
  if (ctx.enums[t]) return `h_tb_${t}(${e}, b, 0);`;
  return `h_sb_puts(b, \"null\");`;
}
function genTB(name, shape, fields, ctx) {
  const L = [];
  L.push(`static void h_tb_${name}(const ${name}* p, h_strbuf* b, int top) {`);
  L.push(`  h_sb_puts(b, top ? "{\\\"__ver\\\":1,\\\"__shape\\\":\\\"${shape}\\\",\\\"__type\\\":\\\"${name}\\\",\\\"__fields\\\":{" : "{\\\"__shape\\\":\\\"${shape}\\\",\\\"__type\\\":\\\"${name}\\\",\\\"__fields\\\":{");`);
  fields.forEach((f, i) => {
    if (i) L.push(`  h_sb_puts(b, \",\");`);
    L.push(`  h_sb_puts(b, \"\\\"${f.name}\\\":\");`);
    L.push(`  ${tbValue(f.type, `p->${f.name}`, ctx)}`);
  });
  L.push(`  h_sb_puts(b, \"}}\");`);
  L.push(`}`);
  return L.join("\n");
}
function genTBArray(elem, ctx) {
  const an = shortName(elem) + "_Array";
  const el = tbValue(elem, "a->data[i]", ctx);
  return `static void h_tb_${an}(const ${an}* a, h_strbuf* b, int top) {\n  h_sb_char(b, '[');\n  for (unsigned long long i = 0; i < a->len; i++) {\n    if (i) h_sb_char(b, ',');\n    ${el}\n  }\n  h_sb_char(b, ']');\n}`;
}
function genTBTuple(cn, tname, ctx) {
  const elems = tupleElemTypes(tname);
  const named = tupleIsNamed(tname);
  const names = named ? elems.map(x => x.split(": ")[0]) : elems.map((_, i) => "_" + i);
  const types = named ? elems.map(x => x.split(": ")[1]) : elems;
  const L = [];
  L.push(`static void h_tb_${cn}(const ${cn}* p, h_strbuf* b, int top) {`);
  if (named) {
    L.push(`  h_sb_puts(b, top ? "{\\\"__ver\\\":1,\\\"__shape\\\":\\\"block\\\",\\\"__fields\\\":{" : "{\\\"__shape\\\":\\\"block\\\",\\\"__fields\\\":{");`);
    types.forEach((t, i) => {
      if (i) L.push(`  h_sb_puts(b, ",");`);
      L.push(`  h_sb_puts(b, "\\\"${names[i]}\\\":");`);
      L.push(`  ${tbValue(t, `p->${names[i]}`, ctx)}`);
    });
  } else {
    L.push(`  h_sb_puts(b, top ? "{\\\"__ver\\\":1,\\\"__shape\\\":\\\"block\\\",\\\"__items\\\":[" : "{\\\"__shape\\\":\\\"block\\\",\\\"__items\\\":[");`);
    types.forEach((t, i) => {
      if (i) L.push(`  h_sb_char(b, ',');`);
      L.push(`  ${tbValue(t, `p->${names[i]}`, ctx)}`);
    });
  }
  L.push(`  h_sb_puts(b, ${named ? '"}}"' : '"]}"'});`);
  L.push(`}`);
  return L.join("\n");
}
function genTBEnum(name, variants) {
  const cases = variants.map(v => `    case ${name}_${v}: h_sb_puts(b, "${v}"); break;`).join("\n");
  return `static void h_tb_${name}(${name} v, h_strbuf* b, int top) {\n  h_sb_puts(b, \"{\\\"__shape\\\":\\\"enum\\\",\\\"__type\\\":\\\"${name}\\\",\\\"__variant\\\":\\\"\");\n  switch (v) {\n${cases}\n  }\n  h_sb_puts(b, \"\\\"}\");\n}`;
}
function genToBytesEntry(name, sig) {
  return `static char* h_to_bytes_${name}(${sig}) { h_strbuf b = {0}; h_tb_${name}(p, &b, 1); return h_sb_done(&b); }`;
}
function revValue(t, fname, ctx) {
  if (t === "f64") return `h_jnum(F, \"${fname}\")`;
  if (t === "u64") return `(unsigned long long)h_jnum(F, \"${fname}\")`;
  if (t === "bool") { const v = `h_json_find(F, \"${fname}\")`; return `(${v} && ${v}->kind == 4) ? true : false`; }
  if (t === "Str") { const v = `h_json_find(F, \"${fname}\")`; return `(${v} && ${v}->kind == 3 ? strdup(${v}->str) : \"\")`; }
  if (t.startsWith("[") && t.endsWith("]")) return `h_jrev_${shortName(t.slice(1, -1))}_Array(h_json_find(F, \"${fname}\"))`;
  if (ctx.classes[t]) return `h_jrev_${t}(h_json_find(F, \"${fname}\"))`;
  if (ctx.structs[t]) return `h_jrev_${t}(h_json_find(F, \"${fname}\"))`;
  if (ctx.enums[t]) return `h_jrev_${t}(h_json_find(F, \"${fname}\"))`;
  return "0";
}
function revElem(t, e, ctx) {
  if (t === "f64") return `${e}->kind == 2 ? ${e}->num : 0`;
  if (t === "u64") return `(${e}->kind == 2 ? (unsigned long long)${e}->num : 0)`;
  if (t === "bool") return `${e}->kind == 4 ? true : false`;
  if (t === "Str") return `${e}->kind == 3 ? strdup(${e}->str) : \"\"`;
  if (t.startsWith("[") && t.endsWith("]")) return `h_jrev_${shortName(t.slice(1, -1))}_Array(${e})`;
  if (ctx.classes[t] || ctx.structs[t] || ctx.enums[t]) return `h_jrev_${t}(${e})`;
  return "0";
}
function genRevStruct(name, fields, ctx) {
  const L = [];
  L.push(`static ${name} h_jrev_${name}(h_json* j) {`);
  L.push(`  ${name} r = {0};`);
  L.push(`  if (!j || j->kind != 0) return r;`);
  L.push(`  h_json* F = h_json_find(j, \"__fields\");`);
  fields.forEach(f => L.push(`  r.${f.name} = ${revValue(f.type, f.name, ctx)};`));
  L.push(`  return r;`);
  L.push(`}`);
  return L.join("\n");
}
function genRevClass(name, fields, ctx) {
  const L = [];
  L.push(`static ${name}* h_jrev_${name}(h_json* j) {`);
  L.push(`  if (!j || j->kind == 6) return NULL;`);
  L.push(`  h_json* F = h_json_find(j, \"__fields\");`);
  L.push(`  return h_new_${name}(${fields.map(f => revValue(f.type, f.name, ctx)).join(", ")});`);
  L.push(`}`);
  return L.join("\n");
}
function genRevArray(elem, ctx) {
  const an = shortName(elem) + "_Array";
  const el = revElem(elem, "j->arr[i]", ctx);
  return `static ${an} h_jrev_${an}(h_json* j) {\n  ${an} a = {0};\n  if (!j || j->kind != 1) return a;\n  a.len = j->arr_n;\n  a.data = a.len ? (${cType(elem)}*)malloc(sizeof(${cType(elem)}) * a.len) : NULL;\n  for (unsigned long long i = 0; i < a.len; i++) a.data[i] = ${el};\n  return a;\n}`;
}
function genRevEnum(name, variants) {
  const L = [];
  L.push(`static ${name} h_jrev_${name}(h_json* j) {`);
  L.push(`  const char* v = \"\";`);
  L.push(`  if (j && j->kind == 0) { h_json* vv = h_json_find(j, \"__variant\"); if (vv && vv->kind == 3) v = vv->str; }`);
  variants.forEach(v => L.push(`  if (strcmp(v, \"${v}\") == 0) return ${name}_${v};`));
  L.push(`  return ${name}_${variants[variants.length - 1]};`);
  L.push(`}`);
  return L.join("\n");
}
function genFromBytes(name, retT) {
  return `static ${retT} h_from_bytes_${name}(const char* s) {\n  const char* p = s;\n  h_json* j = h_json_parse_value(&p);\n  h_json* vv = h_json_find(j, \"__ver\");\n  if (vv && vv->kind == 2 && vv->num > 1) { fprintf(stderr, \"\\u274c 不支持的字节格式版本 %.0f\\n\", vv->num); exit(1); }\n  h_json* tt = h_json_find(j, \"__type\");\n  if (tt && tt->kind == 3 && strcmp(tt->str, \"${name}\") != 0) { fprintf(stderr, \"\\u274c 字节类型标签不匹配：期望 ${name}，实际 %s\\n\", tt->str); exit(1); }\n  ${retT} r = h_jrev_${name}(j);\n  h_json_free(j);\n  return r;\n}`;
}

/* ---------- class（树）：构造/释放/引用通知 + 方法（typedef 见 genClassDecls） ---------- */
function genClass(d, ctx) {
  const refFields = d.fields.filter(f => f.fieldType.mutable);
  // 构造：h_new_Type(字段按声明序)；数组字段深拷贝；ref 字段经 setter 注册（双向引用）
  const newArgs = d.fields.map(f => {
    const ft = typeName(f.fieldType);
    const ct = f.fieldType.mutable ? cType(ft) + "*" : cType(ft);
    return `${ct} ${f.name}_v`;
  }).join(", ");
  const newInit = d.fields.map(f => {
    const ft = typeName(f.fieldType);
    if (f.fieldType.mutable) return `  p->${f.name} = NULL;\n  h_set_${d.name}_${f.name}(p, ${f.name}_v);`;
    if (ft.startsWith("[") && ft.endsWith("]")) {
      const et = cType(ft.slice(1, -1));
      return `  p->${f.name} = ${f.name}_v;\n  p->${f.name}.data = (${et}*)malloc(sizeof(${et}) * ${f.name}_v.len);\n  memcpy(p->${f.name}.data, ${f.name}_v.data, sizeof(${et}) * ${f.name}_v.len);`;
    }
    return `  p->${f.name} = ${f.name}_v;`;
  }).join("\n");
  const ctor = `static ${d.name}* h_new_${d.name}(${newArgs || "void"}) {\n  ${d.name}* p = (${d.name}*)malloc(sizeof(${d.name}));\n  p->_refs = NULL;\n${newInit}\n  return p;\n}`;
  // setter：先注销旧目标，再注册到新目标（ref 字段双向引用）
  const setters = refFields.map(f => {
    const t = typeName(f.fieldType);
    return `static void h_set_${d.name}_${f.name}(${d.name}* h, ${t}* v) {\n  if (h->${f.name}) h_ref_detach(&h->${f.name}->_refs, &h->_${f.name}_link);\n  h->${f.name} = v;\n  if (v) { h->_${f.name}_link.pslot = (void**)&h->${f.name}; h->_${f.name}_link.next = v->_refs; v->_refs = &h->_${f.name}_link; }\n}`;
  }).join("\n");
  // 析构：通知所有指向本对象的 ref 字段置 NULL（防悬垂）；再注销本对象持有的 ref 字段
  const notify = `  struct h_ref_link* l = p->_refs;\n  while (l) { struct h_ref_link* nx = l->next; *(void**)l->pslot = NULL; l = nx; }`;
  const detaches = refFields.map(f => `  if (p->${f.name}) h_ref_detach(&p->${f.name}->_refs, &p->_${f.name}_link);`).join("\n");
  const dtorFrees = d.fields.map(f => {
    const ft = typeName(f.fieldType);
    return ft.startsWith("[") && ft.endsWith("]") ? `  free(p->${f.name}.data);` : "";
  }).filter(Boolean).join("\n");
  const dtor = `static void h_free_${d.name}(${d.name}* p) {\n${notify}\n${detaches}${detaches ? "\n" : ""}${dtorFrees}${dtorFrees ? "\n" : ""}  free(p);\n}`;
  const methods = d.methods.map(m => genFun(m, ctx, d.name)).join("\n");
  return (setters ? setters + "\n" : "") + ctor + "\n" + dtor + (methods ? "\n" + methods : "");
}

/* ---------- 函数 / 方法 ---------- */
function findRetType(scope) { let s = scope; while (s) { if (s.retType) return s.retType; s = s.parent; } return null; }
function resolveFname(callee, scope) {
  if (callee.type === "Ident") {
    if (callee.name === "print" || callee.name === "store" || callee.name === "load") return null;
    return callee.name === "main" ? "h_main" : callee.name;
  }
  if (callee.type === "MemberExpr") {
    const t = inferType(callee.obj, scope);
    const entry = scope.ctx.classMethods[t] && scope.ctx.classMethods[t][callee.prop];
    return entry ? entry.source + "_" + entry.name : null;
  }
  return null;
}
function genFun(d, ctx, className) {
  const scope = new Scope(null, ctx);
  if (className) scope.receiverType = className;
  if (d.ret && d.ret.kind === "error") scope.retType = { kind: "error", T: d.ret.rtype ? typeName(d.ret.rtype) : "void" };
  const params = d.params.map(p => {
    const t = typeName(p.ptype);
    let ct = ctx.classes[t] ? cType(t) + "*" : cType(t);
    if (p.kind === "ref") { scope.refParams.add(p.name); ct += "*"; }   // ref 参数 = 指向调用者变量的指针（写透别名）
    // 树参数语义：val/ref = 视图（不拥有，不销毁）；move = 拥有（函数退出时销毁）
    scope.declareType(p.name, t, p.kind === "move");
    return `${ct} ${p.name}`;
  });
  const allParams = (className ? [`${className}* self`] : []).concat(params);
  const body = d.body.stmts.map(s => genStmt(s, scope)).join("\n");
  // 函数最外层作用域：树变量销毁（move 后跳过——已从 trees 移除；视图参数从不进入）
  const frees = [...scope.trees].map(n => `  h_free_${scope.typeOf(n)}(${n});`).join("\n");
  const retT = d.ret ? typeName(d.ret.rtype) : "void";
  const ret = d.ret
    ? (d.ret.kind === "error"
      ? `h_err_${shortName(retT)}`
      : (ctx.classes[retT] ? cType(retT) + "*" : cType(retT)))
    : "void";
  let fname = className ? className + "_" + d.name : d.name;
  if (!className && fname === "main") fname = "h_main";
  return `${ret} ${fname}(${allParams.join(", ")}) {\n${body}${frees ? "\n" + frees : ""}\n}`;
}

class Scope {
  constructor(parent, ctx) {
    this.parent = parent || null; this.ctx = ctx;
    this.vars = new Set(); this.types = new Map(); this.trees = new Set(); this.receiverType = null; this.refParams = new Set(); this.retType = null;
  }
  declared(name) { let s = this; while (s) { if (s.vars.has(name)) return true; s = s.parent; } return false; }
  declareType(name, t, owned = true) { this.vars.add(name); this.types.set(name, t); if (owned && this.classType(t)) this.trees.add(name); }
  typeOf(name) { let s = this; while (s) { if (s.types.has(name)) return s.types.get(name); s = s.parent; } return null; }
  releaseTree(name) { let s = this; while (s) { if (s.trees.delete(name)) return; s = s.parent; } }
  classType(t) { return t && this.ctx.classes[t] ? t : null; }
}

function genStmt(st, scope) {
  switch (st.type) {
    case "VarDecl": {
      if (st.kind === "ref" || st.kind === "move") throw new Error("C 后端暂不支持 ref/move 参数——请用 h run");
      const init = genExpr(st.init, scope);
      if (scope.declared(st.name)) {
        // 覆盖声明 = 赋值；ref 参数写透调用者变量
        const tgt = scope.refParams.has(st.name) ? "(*" + st.name + ")" : st.name;
        return `  ${tgt} = ${init};`;
      }
      const t = inferType(st.init, scope) || "void";
      scope.declareType(st.name, t);
      const ct = scope.classType(t) ? cType(t) + "*" : cType(t);
      return `  ${ct} ${st.name} = ${init};`;
    }
    case "ReturnStmt": {
      const rt = findRetType(scope);
      if (rt) {
        // error 返回：return error.X / return 值（ok 包装）
        if (st.expr && st.expr.type === "ErrorLit") {
          return `  return (h_err_${shortName(rt.T)}){ .ok = false, .name = "${st.expr.name}" };`;
        }
        if (rt.T === "void") return "  return (h_err_void){ .ok = true };";
        if (st.expr && st.expr.type === "Ident") {
          const t = scope.typeOf(st.expr.name);
          if (scope.classType(t)) scope.releaseTree(st.expr.name);
        }
        return `  return (h_err_${shortName(rt.T)}){ .ok = true, .val = ${st.expr ? genExpr(st.expr, scope) : "0"} };`;
      }
      // 返回树（含 -> move T 的无 move 关键字逃逸）：所有权随返回值转移，函数退出不销毁
      if (st.expr && st.expr.type === "Ident") {
        const t = scope.typeOf(st.expr.name);
        if (scope.classType(t)) scope.releaseTree(st.expr.name);
      }
      return "  return " + (st.expr ? genExpr(st.expr, scope) : "") + ";";
    }
    case "IfStmt": {
      const c = genExpr(st.cond, scope);
      const t = genBlockInner(st.then, scope);
      if (st.els) {
        const e = st.els.type === "Block" ? genBlockInner(st.els, scope) : genStmt(st.els, scope);
        return `  if (${c}) {\n${t}\n  } else {\n${e}\n  }`;
      }
      return `  if (${c}) {\n${t}\n  }`;
    }
    case "ForStmt": {
      if (st.range.type !== "RangeExpr" || !st.range.end) throw new Error("for 的 in 必须是数字区间（0..n）");
      const start = genExpr(st.range.obj, scope);
      const end = genExpr(st.range.end, scope);
      const inner = new Scope(scope, scope.ctx);
      inner.receiverType = scope.receiverType;
      inner.declareType(st.varName, "u64", false);
      const body = st.body.stmts.map(s => genStmt(s, inner)).join("\n");
      const frees = [...inner.trees].map(n => `  h_free_${inner.typeOf(n)}(${n});`).join("\n");
      return `  for (unsigned long long ${st.varName} = (${start}); ${st.varName} < (${end}); ${st.varName}++) {\n${body}${frees ? "\n" + frees : ""}\n  }`;
    }
    case "WhileStmt": {
      const c = genExpr(st.cond, scope);
      const inner = new Scope(scope, scope.ctx);
      inner.receiverType = scope.receiverType;
      const body = st.body.stmts.map(s => genStmt(s, inner)).join("\n");
      const frees = [...inner.trees].map(n => `  h_free_${inner.typeOf(n)}(${n});`).join("\n");
      return `  while (${c}) {\n${body}${frees ? "\n" + frees : ""}\n  }`;
    }
    case "BreakStmt": return "  break;";
    case "ContinueStmt": return "  continue;";
    case "Block": return genBlockInner(st, scope);
    case "YieldStmt": return "  h_yield();";
    case "ExprStmt": {
      // 解构 (a, b) = f()：逐元素绑定（新变量声明/已有覆盖），不走普通赋值
      if (st.expr.type === "AssignExpr" && st.expr.left.type === "TupleLit") return genDestructure(st.expr, scope);
      return "  " + genExpr(st.expr, scope) + ";";
    }
    default:
      throw new Error("C 后端暂不支持语句 " + st.type);
  }
}
function genBlockInner(block, scope) {
  const inner = new Scope(scope, scope.ctx);
  inner.receiverType = scope.receiverType;   // 方法体内嵌套块仍可裸访问 self 字段
  const body = block.stmts.map(s => genStmt(s, inner)).join("\n");
  const frees = [...inner.trees].map(n => `  h_free_${inner.typeOf(n)}(${n});`).join("\n");
  return body + (frees ? "\n" + frees : "");
}

function genExpr(e, scope) {
  switch (e.type) {
    case "Literal": return cLiteral(e);
    case "Ident": {
      // ref 参数：解引用写透（调用者变量）
      if (scope.refParams.has(e.name)) return "(*" + e.name + ")";
      // 方法体内裸字段名 → self->field（变量优先）
      if (scope.receiverType && !scope.declared(e.name)) {
        const f = scope.ctx.classes[scope.receiverType].fields.find(x => x.name === e.name);
        if (f) return "self->" + e.name;
      }
      return e.name;
    }
    case "MemberExpr": {
      // 枚举值 Type.Variant → 常量名
      if (e.obj.type === "Ident" && !scope.declared(e.obj.name)) return e.obj.name + "_" + e.prop;
      const ot = inferType(e.obj, scope);
      // 数组/切片 .len → .len 字段
      if (ot && (ot.startsWith("[") || ot.startsWith("[]")) && e.prop === "len") return genExpr(e.obj, scope) + ".len";
      // 元组访问：命名 .x / 位置 .0（C 字段名 _0 不能数字开头）
      if (ot && ot.startsWith("(") && ot.endsWith(")")) {
        const named = tupleIsNamed(ot);
        const field = named ? e.prop : "_" + e.prop;
        return genExpr(e.obj, scope) + "." + field;
      }
      // class（树）字段 → 指针解引用
      if (scope.classType(ot)) return genExpr(e.obj, scope) + "->" + e.prop;
      return genExpr(e.obj, scope) + "." + e.prop;
    }
    case "CallExpr": {
      const code = genCall(e, scope);
      // error 函数调用：语句表达式包装——调用点 fail-fast（未处理即终止，对齐求值器）
      const fname = resolveFname(e.callee, scope);
      if (fname && scope.ctx.retKinds[fname] === "error") {
        const T = shortName(scope.ctx.rets[fname] || "void");
        const val = T === "void" ? "(void)0" : "_he.val";
        return `({ h_err_${T} _he = ${code}; if (!_he.ok) { fprintf(stderr, "\\u274c error.%s（未处理）\\n", _he.name); exit(1); } ${val}; })`;
      }
      return code;
    }
    case "BinExpr": {
      const l = genExpr(e.left, scope), r = genExpr(e.right, scope);
      return `(${l} ${e.op} ${r})`;
    }
    case "UnaryExpr": {
      // 负整数字面量：不能用 ULL（无符号取负变巨大数），直接 (-n) 转 double
      if (e.op === "-" && e.operand.type === "Literal" && Number.isInteger(e.operand.value)) return `(-${e.operand.value})`;
      return `(${e.op}${genExpr(e.operand, scope)})`;
    }
    case "AssignExpr": {
      // ref 字段赋值 → setter（先注销旧目标、再注册新目标，双向引用通知）
      if (e.op === "=" && e.left.type === "MemberExpr") {
        const ot = inferType(e.left.obj, scope);
        const fld = scope.ctx.classes[ot] && scope.ctx.classes[ot].fields.find(x => x.name === e.left.prop);
        if (fld && fld.fieldType.mutable) {
          return `h_set_${ot}_${e.left.prop}(${genExpr(e.left.obj, scope)}, ${genExpr(e.right, scope)});`;
        }
      }
      const target = e.left.type === "Ident" ? genExpr(e.left, scope) : genExpr(e.left, scope);
      return `${target} ${e.op === "=" ? "=" : e.op} ${genExpr(e.right, scope)}`;
    }
    case "ConstructExpr": return genConstruct(e, scope);
    case "MatchExpr": return genMatch(e, scope);
    case "MoveExpr": {
      // move：所有权转移——源从销毁表移除（视图参数不在表内，无操作）
      if (e.expr.type === "Ident") { scope.releaseTree(e.expr.name); return e.expr.name; }
      return genExpr(e.expr, scope);
    }
    case "MoveExpr": {
      if (e.expr.type === "Ident") { scope.releaseTree(e.expr.name); return e.expr.name; }
      return genExpr(e.expr, scope);
    }
    case "ErrorLit": throw new Error("C 后端暂不支持 error——请用 h run");
    case "ArrayLiteral": return genArrayLiteral(e, inferType(e, scope), scope);
    case "TupleLit": return genTupleLiteral(e, scope);
    case "RangeExpr": return genRange(e, scope);
    case "IndexExpr":
      return genExpr(e.obj, scope) + ".data[" + genExpr(e.index, scope) + "]";
    default: throw new Error("C 后端暂不支持表达式 " + e.type);
  }
}

function genConstruct(e, scope) {
  const cls = scope.ctx.classes[e.name];
  if (cls) {
    // 树构造 → h_new_Type(字段按声明序)
    const args = cls.fields.map(f => {
      const found = e.fields.find(x => x.name === f.name);
      if (found) {
        const ft = typeName(f.fieldType);
        // 空数组字面量无元素类型信息 → 强制按字段声明类型生成
        if (ft.startsWith("[") && ft.endsWith("]") && found.expr.type === "ArrayLiteral") {
          return genArrayLiteral(found.expr, ft, scope);
        }
        return genExpr(found.expr, scope);
      }
      return "0";
    });
    return `h_new_${e.name}(${args.join(", ")})`;
  }
  const fields = e.fields.map(f => `.${f.name} = ${genExpr(f.expr, scope)}`).join(", ");
  return `(${e.name}){ ${fields} }`;
}

function cLiteral(e) {
  if (e.kind === "string") return '"' + e.value.replace(/\\/g, "\\\\").replace(/"/g, '\\"') + '"';
  if (e.kind === "bool") return e.value ? "true" : "false";
  if (typeof e.value === "number") {
    if (e.kind === "float") return Number.isInteger(e.value) ? e.value + ".0" : e.value + "";   // 3.0 → 3.0（double），不是 3ULL
    if (Number.isInteger(e.value)) return e.value < 0 ? `(${e.value})` : e.value + "ULL";   // 负数不能用 ULL（无符号取负变巨大数）
    return e.value + "";
  }
  return String(e.value);
}

function genMatch(e, scope) {
  const enumName = inferEnumName(e.target, scope);
  const target = genExpr(e.target, scope);
  const retT = cType(inferType(e, scope) || "double");
  const cases = e.arms.map(arm => {
    const constName = enumName ? enumName + "_" + arm.variant : arm.variant;
    return `      case ${constName}: _h_m = ${genExpr(arm.expr, scope)}; break;`;
  }).join("\n");
  return `({ __typeof__(${target}) _h_t = (${target}); ${retT} _h_m = 0; switch (_h_t) {\n${cases}\n    default: break; } _h_m; })`;
}

function genCall(e, scope) {
  const callee = e.callee;
  // print 特殊：参数走 genPrint（to_str 剥皮/树/struct 整值打印），勿预先 genExpr
  if (callee.type === "Ident" && callee.name === "print") return genPrint(e.args, scope);
  let fname = null;
  if (callee.type === "Ident") {
    if (callee.name === "store" || callee.name === "load") throw new Error("C 后端暂不支持 store/load——请用 h run");
    if (callee.name === "Channel") {
      // Channel(n) → 全局/局部 channel 构造
      return `h_chan_new(${e.args[0] ? genExpr(e.args[0], scope) : "1"})`;
    }
    fname = callee.name === "main" ? "h_main" : callee.name;
  } else if (callee.type === "MemberExpr") {
    const ot = inferType(callee.obj, scope);
    if (ot === "Channel") {
      // 原型：Channel 值仅支持 u64（整数经指针槽传输）
      if (callee.prop === "send") {
        if (e.args.length !== 1 || inferType(e.args[0], scope) !== "u64") throw new Error("C 后端 Channel 暂只支持 send(u64)——请用 h run");
        return `h_chan_send(${genExpr(callee.obj, scope)}, (void*)(uintptr_t)${genExpr(e.args[0], scope)})`;
      }
      if (callee.prop === "recv") return `(unsigned long long)(uintptr_t)h_chan_recv(${genExpr(callee.obj, scope)})`;
      throw new Error("C 后端暂不支持 Channel 方法 " + callee.prop);
    }
    // 静态调用 Type.from_bytes(bytes)
    if (callee.prop === "from_bytes" && callee.obj.type === "Ident") {
      const tn = callee.obj.name;
      const arg = e.args[0] ? genExpr(e.args[0], scope) : "";
      if (scope.ctx.classes[tn]) return `h_from_bytes_${tn}(${arg})`;
      if (scope.ctx.structs[tn]) return `h_from_bytes_${tn}(${arg})`;
      if (scope.ctx.enums[tn]) return `h_from_bytes_${tn}(${arg})`;
    }
    // 字节化：x.to_bytes()
    if (callee.prop === "to_bytes") {
      const obj = genExpr(callee.obj, scope);
      if (scope.classType(ot)) return `h_to_bytes_${ot}(${obj})`;
      if (scope.ctx.structs[ot]) return `h_to_bytes_${ot}(&${obj})`;
      if (ot && ot.startsWith("[") && ot.endsWith("]")) return `h_to_bytes_${shortName(ot.slice(1, -1))}_Array(&${obj})`;
      if (scope.ctx.enums[ot]) return `h_to_bytes_${ot}(${obj})`;
      if (ot && ot.startsWith("(") && ot.endsWith(")")) return `h_to_bytes_${tupleCName(ot)}(&${obj})`;
      throw new Error("C 后端暂不支持该类型的 to_bytes——请用 h run");
    }
    // 切片 clone：深拷贝元素（标量/Str），返回独立数组
    if (ot && ot.startsWith("[]") && callee.prop === "clone") {
      const elem = ot.slice(2);
      if (!["u64", "f64", "bool", "Str"].includes(elem)) throw new Error("C 后端切片 clone 暂只支持标量元素——请用 h run");
      const obj = genExpr(callee.obj, scope);
      return `h_slice_clone_${shortName(elem)}(&${obj})`;
    }
    const t = inferType(callee.obj, scope);
    const table = scope.ctx.classMethods[t];
    const entry = table && table[callee.prop];
    if (entry) fname = entry.source + "_" + entry.name;
    else if (callee.prop === "to_str") throw new Error("C 后端暂不支持 to_str（print 参数内自动处理）——请用 h run");
    else throw new Error("C 后端暂不支持方法调用 " + callee.prop);
  } else throw new Error("C 后端暂不支持该调用");
  const kinds = scope.ctx.paramKinds[fname];
  const ptypes = scope.ctx.paramTypes[fname];
  const args = e.args.map((a, i) => {
    if (kinds && kinds[i] === "ref") {
      // ref 参数：实参必须是可写变量 → 传地址（写透别名）
      if (a.type !== "Ident") throw new Error("ref 实参必须是可写变量（checker R3）——请用 h run");
      return "&" + genExpr(a, scope);
    }
    const code = genExpr(a, scope);
    // [T] 实参 → []T 切片形参：自动借用（视图指向原数据区，不复制）
    const at = inferType(a, scope);
    const pt = ptypes && ptypes[i];
    if (pt && pt.startsWith("[]") && at && at.startsWith("[") && at.endsWith("]")) {
      return `(${shortName(pt.slice(2))}_Slice){ .data = ${code}.data, .len = ${code}.len }`;
    }
    return code;
  });
  if (callee.type === "Ident") return `${fname}(${args.join(", ")})`;
  return `${fname}(${genExpr(callee.obj, scope)}${args.length ? ", " + args.join(", ") : ""})`;
}

function genPrint(args, scope) {
  const parts = [];
  for (const a of args) {
    if (parts.length) parts.push('printf(" ");');
    let t = inferType(a, scope);
    // x.to_str()：按 x 本身的类型打印（求值器 to_str = valueToStr，字符串会加引号）——先剥皮再生成代码
    let target = a, fromToStr = false;
    if (a.type === "CallExpr" && a.callee.type === "MemberExpr" && a.callee.prop === "to_str") {
      target = a.callee.obj;
      t = inferType(target, scope);
      fromToStr = true;
    }
    const code = genExpr(target, scope);
    if (t === "f64") parts.push(`h_print_f64(${code});`);
    else if (t === "u64") parts.push(`printf("%llu", ${code});`);
    else if (t === "bool") parts.push(`printf("%s", (${code}) ? "true" : "false");`);
    else if (t === "Str") parts.push(fromToStr ? `printf("\\\"%s\\\"", ${code});` : `printf("%s", ${code});`);
    else if (scope.classType(t)) parts.push(`if (${code}) { h_print_${t}(${code}); } else { printf("null"); }`);
    else if (scope.ctx.structs[t]) parts.push(`h_print_${t}(&${code});`);
    else if (t && t.startsWith("[") && t.endsWith("]")) parts.push(`h_print_${shortName(t.slice(1, -1))}_Array(&${code});`);
    else if (t && t.startsWith("[]")) parts.push(`h_print_${shortName(t.slice(2))}_Slice(&${code});`);
    else if (t && t.startsWith("(") && t.endsWith(")")) parts.push(`h_print_${tupleCName(t)}(&${code});`);
    else parts.push(`printf("%s", ${code});`);
  }
  parts.push('printf("\\n");');
  return "h_print_lock();\n  " + parts.join("\n  ") + "\n  h_print_unlock();";
}

/* 数组字面量 → 复合字面量（可强制元素类型：空数组/字段类型已知时） */
function genArrayLiteral(e, t, scope) {
  const items = e.items.map(x => genExpr(x, scope)).join(", ");
  const et = cType(t.slice(1, -1));
  return `(${cType(t)}){ .len = ${e.items.length}, .data = (${et}[]){ ${items} } }`;
}
/* 元组字面量 → 复合字面量 (tup_x){ ._0 = .., ._1 = .. }（类型须已在收集阶段注册） */
function genTupleLiteral(e, scope) {
  const t = inferType(e, scope);
  if (!t || !scope.ctx.tuples[t]) throw new Error("C 后端无法推断元组类型：" + (t || "?"));
  const cn = tupleCName(t);
  const names = e.named ? e.items.map(i => i.name) : e.items.map((_, i) => "_" + i);
  const vals = e.items.map((it, i) => "." + names[i] + " = " + genExpr(it.expr, scope)).join(", ");
  return `(${cn}){ ${vals} }`;
}
/* 切片 range 表达式：s[a..b] → (T_Slice){ .data = &s.data[a], .len = b - a }（视图，不复制） */
function genRange(e, scope) {
  const ot = inferType(e.obj, scope);
  const elem = ot.startsWith("[]") ? ot.slice(2) : ot.slice(1, -1);
  const sn = shortName(elem) + "_Slice";
  const obj = genExpr(e.obj, scope);
  const start = e.start ? genExpr(e.start, scope) : "0";
  const end = e.end ? genExpr(e.end, scope) : obj + ".len";
  const data = e.start ? `&${obj}.data[${start}]` : `${obj}.data`;
  return `(${sn}){ .data = ${data}, .len = (${end}) - (${start}) }`;
}
/* 解构 (a, b) = f()：临时承接 + 逐元素赋值（新变量声明 / 已有覆盖） */
let destructSeq = 0;
function genDestructure(e, scope) {
  const t = inferType(e.right, scope);
  if (!t || !scope.ctx.tuples[t]) throw new Error("C 后端无法推断解构类型：" + (t || "?"));
  const cn = tupleCName(t);
  const named = tupleIsNamed(t);
  const elems = tupleElemTypes(t);
  const types = named ? elems.map(x => x.split(": ")[1]) : elems;
  const names = named ? elems.map(x => x.split(": ")[0]) : elems.map((_, i) => "_" + i);
  const tmp = "_d" + (destructSeq++);
  const L = [`  ${cn} ${tmp} = ${genExpr(e.right, scope)};`];
  e.left.items.forEach((it, i) => {
    const nm = it.expr.name;
    const t2 = types[i] || "u64";
    if (scope.declared(nm)) {
      L.push(`  ${nm} = ${tmp}.${names[i]};`);
    } else {
      scope.declareType(nm, t2, false);
      L.push(`  ${cType(t2)} ${nm} = ${tmp}.${names[i]};`);
    }
  });
  return L.join("\n");
}

function inferEnumName(e, scope) {
  if (e.type === "Ident") {
    const t = scope.typeOf(e.name);
    return t && t !== "?" && t !== "void" && !scope.classType(t) ? t : null;
  }
  if (e.type === "MemberExpr" && e.obj.type === "Ident") return e.obj.name;
  return null;
}
function inferType(e, scope) {
  switch (e.type) {
    case "Literal":
      if (typeof e.value === "number") return e.kind === "float" ? "f64" : "u64";
      if (typeof e.value === "string") return "Str";
      return "bool";
    case "Ident": {
      if (scope.ctx.globals && scope.ctx.globals[e.name]) return "Channel";   // 全局 channel
      return scope.typeOf(e.name) || "?";
    }
    case "ConstructExpr": return e.name;
    case "MoveExpr": return inferType(e.expr, scope);
    case "MemberExpr": {
      if (e.obj.type === "Ident" && !scope.declared(e.obj.name)) return e.obj.name;
      const ot = inferType(e.obj, scope);
      if (ot && (ot.startsWith("[") || ot.startsWith("[]")) && e.prop === "len") return "u64";
      // 元组访问：命名 .x / 位置 .0 → 元素类型
      if (ot && ot.startsWith("(") && ot.endsWith(")")) {
        const named = tupleIsNamed(ot);
        const elems = tupleElemTypes(ot);
        const types = named ? elems.map(x => x.split(": ")[1]) : elems;
        if (named) {
          const idx = elems.findIndex(x => x.split(": ")[0] === e.prop);
          return idx >= 0 ? types[idx] : "?";
        }
        const n = Number(e.prop);
        return Number.isInteger(n) && n >= 0 && n < types.length ? types[n] : "?";
      }
      if (scope.ctx.classes[ot]) {
        const f = scope.ctx.classes[ot].fields.find(x => x.name === e.prop);
        return f ? f.type : "?";
      }
      if (scope.ctx.structs && scope.ctx.structs[ot] && scope.ctx.structs[ot][e.prop]) {
        return scope.ctx.structs[ot][e.prop];
      }
      return "?";
    }
    case "ArrayLiteral": {
      const et = e.items.length ? inferType(e.items[0], scope) : "u64";
      return "[" + et + "]";
    }
    case "IndexExpr": {
      const ot = inferType(e.obj, scope);
      if (!ot) return "?";
      if (ot.startsWith("[]")) return ot.slice(2);
      return ot.startsWith("[") ? ot.slice(1, -1) : "?";
    }
    case "TupleLit": {
      const ts = e.items.map(it => inferType(it.expr, scope) || "?");
      return e.named ? "(" + e.items.map((it, i) => it.name + ": " + ts[i]).join(", ") + ")" : "(" + ts.join(", ") + ")";
    }
    case "RangeExpr": {
      const ot = inferType(e.obj, scope);
      if (!ot) return "?";
      return ot.startsWith("[]") ? ot : "[]" + ot.slice(1, -1);
    }
    case "MatchExpr": return e.arms.length ? inferType(e.arms[0].expr, scope) : "?";
    case "BinExpr": {
      const l = inferType(e.left, scope), r = inferType(e.right, scope);
      if (l === "f64" || r === "f64") return "f64";
      if (l !== "?") return l;
      return r;
    }
    case "UnaryExpr": return inferType(e.operand, scope);
    case "CallExpr": {
      if (e.callee.type === "Ident") {
        if (e.callee.name === "Channel") return "Channel";
        if (scope.ctx.rets[e.callee.name]) return scope.ctx.rets[e.callee.name];
      }
      if (e.callee.type === "MemberExpr") {
        const t = inferType(e.callee.obj, scope);
        if (t === "Channel") {
          if (e.callee.prop === "recv") {
            const g = e.callee.obj.type === "Ident" ? scope.ctx.globals[e.callee.obj.name] : null;
            return g ? g.elem : "u64";
          }
          return "void";
        }
        if (e.callee.prop === "to_bytes") return "Str";
        if (e.callee.prop === "from_bytes" && e.callee.obj.type === "Ident") return e.callee.obj.name;
        if (t && t.startsWith("[]") && e.callee.prop === "clone") return "[" + t.slice(2) + "]";   // clone 返回独立数组
        const table = scope.ctx.classMethods[t];
        const entry = table && table[e.callee.prop];
        if (entry) {
          const src = scope.ctx.classes[entry.source];
          const m = src.methods.find(x => x.name === entry.name);
          return m && m.ret ? typeName(m.ret.rtype) : "void";
        }
      }
      return "?";
    }
    default: return "?";
  }
}

module.exports = { genC };

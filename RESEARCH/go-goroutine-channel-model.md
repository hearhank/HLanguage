# Go Goroutines & Channels: A Deep Dive

> Research conducted using primary sources: Go source code (`src/runtime/proc.go`, `src/runtime/chan.go`, `src/runtime/select.go`, `src/runtime/HACKING.md`), Go official documentation (FAQ, Effective Go, Memory Model), Go blog posts, and design talks.

---

## Table of Contents

1. [Goroutine Scheduling: The M:N Model & G-M-P](#1-goroutine-scheduling-the-mn-model--g-m-p)
2. [Goroutine Cost & Stack Management](#2-goroutine-cost--stack-management)
3. [Channel Implementation](#3-channel-implementation)
4. [The `select` Statement](#4-the-select-statement)
5. [Go's Philosophy: Share Memory by Communicating](#5-gos-philosophy-share-memory-by-communicating)
6. [Comparison with OS Threads](#6-comparison-with-os-threads)
7. [History: Why Goroutines Instead of OS Threads or Async/Await](#7-history-why-goroutines-instead-of-os-threads-or-asyncawait)

---

## 1. Goroutine Scheduling: The M:N Model & G-M-P

### Overview

Go implements an **M:N scheduler** (also called a "hybrid" or "two-level" scheduler) that multiplexes `M` goroutines onto `N` OS threads. The scheduler is part of the Go runtime, compiled directly into every Go binary.

The scheduler's job is described in the runtime source code:

> "The scheduler's job is to distribute ready-to-run goroutines over worker threads." — `src/runtime/proc.go`, line 26

### The G-M-P Triad

The runtime manages three core types, as documented in `src/runtime/HACKING.md`:

| Component | Type | Description |
|-----------|------|-------------|
| **G** | `g` | A goroutine. Contains the stack, instruction pointer, and other state (e.g., waiting reason, channel it's parked on). |
| **M** | `m` | An OS thread (a "machine"). Executes Go code, runtime code, or system calls. Can be idle or blocked. |
| **P** | `p` | A "processor" — the resource required to execute Go code. Exactly `GOMAXPROCS` Ps exist. |

Key relationships from `src/runtime/proc.go`:

> **G** — goroutine.
> **M** — worker thread, or machine.
> **P** — processor, a resource that is required to execute Go code.
> M must have an associated P to execute Go code, however it can be blocked or in a syscall without an associated P.
>
> — `src/runtime/proc.go`, lines 29-33

The scheduler's job is to match up a G (code to execute), an M (where to execute it), and a P (the rights and resources to execute it).

### How Scheduling Works

**Cooperative + Preemptive Scheduling.** Go's scheduler is primarily cooperative, but with asynchronous preemption support since Go 1.14:

1. **Cooperative yield points**: Function calls, channel operations, `Gosched()`, garbage collection safepoints.
2. **Asynchronous preemption**: The `sysmon` thread (a system monitoring thread) sends a preemption signal to goroutines that have run for more than **10ms** (`forcePreemptNS = 10 * 1000 * 1000` in `proc.go`), setting `gp.preempt = true` and `gp.stackguard0 = stackPreempt`. This causes the goroutine's next function call to trigger a stack-growth check, which instead initiates a preemption.

The `sysmon` thread runs continuously, as described in `src/runtime/proc.go`:

```go
// forcePreemptNS is the time slice given to a G before it is
// preempted.
const forcePreemptNS = 10 * 1000 * 1000 // 10ms
```

### The Scheduling Loop

The core scheduling loop is in `schedule()` at `src/runtime/proc.go:4150`:

1. **`schedule()`** calls `findRunnable()` to locate a runnable G.
2. **`findRunnable()`** (line 3404) checks in order:
   - GC waiting or safe-point work
   - Trace reader
   - GC worker
   - Global run queue (every 61st tick for fairness)
   - Local P run queue
   - Global run queue (batch)
   - Network poller (non-blocking)
   - **Work stealing** from other Ps
3. **`execute()`** (line 3346) transitions the G to `_Grunning` and jumps to its instruction pointer via `gogo()`.
4. When a G blocks (on a channel, syscall, or timer), the M calls `schedule()` again.

### Work Stealing

When a P has no work in its local queue, it becomes a **spinning M** and attempts to steal work from other Ps. The stealing is randomized and fair:

```go
const stealTries = 4
```

The work-stealing algorithm (from `src/runtime/proc.go:3843`) tries up to 4 times, each time iterating through all other Ps in a random order, stealing half of their run queue entries.

### Spinning Thread Management

The runtime carefully manages "spinning" threads (threads looking for work) to balance CPU utilization:

> We unpark an additional thread when we submit work if:
> 1. There is an idle P, and
> 2. There are no "spinning" worker threads.
>
> — `src/runtime/proc.go`, lines 65-67

This prevents excessive thread creation while ensuring all available parallelism is utilized.

### GOMAXPROCS

`GOMAXPROCS` controls the number of Ps, which limits the number of goroutines that can execute simultaneously. Since Go 1.5, it defaults to the number of CPU cores available:

> By default, Go programs run with `GOMAXPROCS` set to the number of cores available; in prior releases it defaulted to 1.
>
> — Go 1.5 Release Notes, "Introduction"

The runtime can allocate more OS threads than `GOMAXPROCS` when goroutines are blocked in system calls:

> The runtime can allocate more threads than the value of `GOMAXPROCS` to service multiple outstanding I/O requests. `GOMAXPROCS` only affects how many goroutines can actually execute at once; arbitrarily more may be blocked in system calls.
>
> — Go FAQ, "How can I control the number of CPUs?"

### Network Poller Integration

When a goroutine performs network I/O, the runtime's **netpoller** (integrated with the OS's epoll/kqueue/IOCP) handles the blocking asynchronously. The goroutine is parked, and the P can run other work. When the network operation completes, the goroutine is made runnable again. This is a key reason Go can handle hundreds of thousands of connections with few OS threads.

---

## 2. Goroutine Cost & Stack Management

### Memory Footprint

A goroutine's initial cost is minimal. From the Go FAQ:

> A newly minted goroutine is given a few kilobytes, which is almost always enough.
>
> — Go FAQ, "Why goroutines instead of threads?"

The initial stack size in the current runtime is **2 KB** (`stackMin` = 2048 bytes). This is defined in `src/runtime/stack.go`:

```go
const (
    stackMin = 2048
)
```

Because goroutines are so cheap, it is practical to create hundreds of thousands or even millions of them:

> It is practical to create hundreds of thousands of goroutines in the same address space. If goroutines were just threads, system resources would run out at a much smaller number.
>
> — Go FAQ, "Why goroutines instead of threads?"

### Stack Growth (Dynamic Resizable Stacks)

Go does not use traditional segmented stacks (which had "hot split" problems). Instead, since Go 1.3, it uses **contiguous stacks with copying**:

> User stacks start small (e.g., 2K) and grow or shrink dynamically.
>
> — `src/runtime/HACKING.md`

When a goroutine's stack is too small:

1. The function prologue calls `morestack()`.
2. A new, larger stack is allocated (typically 2× the current size).
3. All stack contents are **copied** to the new stack.
4. All pointers into the old stack are **updated** (the stack is a GC root, so the GC knows all pointers).
5. The old stack is freed.

This is efficient because:
- Most goroutines never need more than the initial 2 KB.
- Stack copying is local and scales well.
- Stacks can also **shrink** during GC if they are mostly unused.

> When a goroutine exits, its stack memory may be freed immediately or retained for reuse by another goroutine. If the stack has the default starting size, it is kept with the `g` object for reuse. If the stack has grown beyond the starting size, it is freed and a new stack will be allocated when the `g` is reused.
>
> — `src/runtime/HACKING.md`

### Maximum Stack Size

The maximum stack size is 1 GB on 64-bit systems and 250 MB on 32-bit:

```go
if goarch.PtrSize == 8 {
    maxstacksize = 1000000000
} else {
    maxstacksize = 250000000
}
```

### G Object Reuse

The `g` struct itself is never freed — it's returned to a pool:

> When a goroutine exits, its `g` object is returned to a pool of free `g`s and can later be reused for some other goroutine.
>
> — `src/runtime/HACKING.md`

This amortizes allocation costs and maintains type stability for the GC.

### CPU Overhead

> The CPU overhead averages about three cheap instructions per function call.
>
> — Go FAQ, "Why goroutines instead of threads?"

---

## 3. Channel Implementation

### The `hchan` Struct

Channels are implemented in `src/runtime/chan.go`. The core data structure is `hchan`:

```go
type hchan struct {
    qcount   uint           // total data in the queue
    dataqsiz uint           // size of the circular queue
    buf      unsafe.Pointer // points to an array of dataqsiz elements
    elemsize uint16
    closed   uint32
    timer    *timer         // timer feeding this chan
    elemtype *_type         // element type
    sendx    uint           // send index
    recvx    uint           // receive index
    recvq    waitq          // list of recv waiters
    sendq    waitq          // list of send waiters
    lock     mutex          // protects all fields
}
```

Key invariants, from the source:

> At least one of `c.sendq` and `c.recvq` is empty, except for the case of an unbuffered channel with a single goroutine blocked on it for both sending and receiving using a select statement.
>
> For buffered channels: `c.qcount > 0` implies that `c.recvq` is empty. `c.qcount < c.dataqsiz` implies that `c.sendq` is empty.
>
> — `src/runtime/chan.go`, lines 9-18

### Buffered vs. Unbuffered Channels

**Unbuffered channels** (`make(chan T)` or `make(chan T, 0)`):
- `dataqsiz` = 0, `buf` = nil
- A send blocks until a matching receiver is ready.
- A receive blocks until a matching sender is ready.
- The value is copied **directly from the sender's stack to the receiver's stack** via `sendDirect()`.

**Buffered channels** (`make(chan T, N)` where N > 0):
- A circular buffer of size N is allocated.
- A send only blocks when the buffer is full (`qcount == dataqsiz`).
- A receive only blocks when the buffer is empty (`qcount == 0`).

### Send Operation (`chansend`)

The send algorithm in `src/runtime/chan.go:176`:

1. **Fast path** (non-blocking only): Check if the channel is closed and full without acquiring the lock.
2. **Lock** the channel.
3. **Check closed**: Panic if sending on a closed channel.
4. **Try direct handoff**: If a waiting receiver exists (`c.recvq.dequeue() != nil`), send the value directly to the receiver, bypassing the buffer.
5. **Try buffer**: If space is available in the buffer, enqueue the element.
6. **Block** (if blocking): Acquire a `sudog` (scheduler data structure for blocking on channels), enqueue on `c.sendq`, and park the goroutine via `gopark()`.

### Receive Operation (`chanrecv`)

The receive algorithm in `src/runtime/chan.go:524`:

1. **Fast path** (non-blocking only): Check if the channel is empty without acquiring the lock.
2. **Lock** the channel.
3. **Closed + empty**: Return zero value.
4. **Try direct handoff**: If a waiting sender exists, receive directly.
5. **Try buffer**: If buffer is non-empty, dequeue from `recvx` position.
6. **Block**: Acquire a `sudog`, enqueue on `c.recvq`, and park.

### Direct Handoff Optimization

For unbuffered or empty-buffered channels, the runtime copies data **directly from one goroutine's stack to another's** without going through the buffer:

```go
func sendDirect(t *_type, sg *sudog, src unsafe.Pointer) {
    // src is on our stack, dst is a slot on another stack.
    dst := sg.elem.get()
    typeBitsBulkBarrier(t, uintptr(dst), uintptr(src), t.Size_)
    memmove(dst, src, t.Size_)
}
```

This is a critical optimization. The comment notes:

> Sends and receives on unbuffered or empty-buffered channels are the only operations where one running goroutine writes to the stack of another running goroutine.
>
> — `src/runtime/chan.go`, lines 382-386

### Close Operation

Closing a channel (`closechan` at line 414):
1. Sets `c.closed = 1`.
2. Releases all waiting receivers (they receive zero values, with `success = false`).
3. Releases all waiting senders (they will panic when they wake up).
4. All goroutines are made runnable via `goready()` **after** the channel lock is released.

### Memory Model Guarantees

The Go Memory Model specifies:

> A send on a channel is synchronized before the completion of the corresponding receive from that channel.
>
> A receive from an unbuffered channel is synchronized before the completion of the corresponding send on that channel.
>
> The kth receive from a channel with capacity C is synchronized before the completion of the k+Cth send on that channel.
>
> — Go Memory Model, "Channel communication"

### The `sudog` Structure

`sudog` (scheduler + "s" + "u" for goroutine?) is the per-goroutine structure used when a goroutine blocks on a channel:

```go
type sudog struct {
    g        *g
    isSelect bool
    elem     unsafe.Pointer // data element
    c        *hchan
    next     *sudog
    prev     *sudog
    waitlink *sudog
    success  bool
    // ...
}
```

These are allocated from per-P caches to avoid contention:

```go
func acquireSudog() *sudog {
    mp := acquirem()
    pp := mp.p.ptr()
    if len(pp.sudogcache) == 0 {
        // grab a batch from central cache
    }
    // ...
}
```

---

## 4. The `select` Statement

The `select` statement is implemented in `src/runtime/select.go` by the `selectgo()` function.

### Algorithm

The `selectgo` function (line 122) follows these steps:

**Phase 1: Randomize and sort**
1. Randomize the polling order of all cases (using `cheaprandn`).
2. Sort the lock order by channel address to prevent deadlocks (heap sort, O(n log n)).

**Phase 2: Try to proceed without blocking**
For each case in randomized order:
- **Send case**: If the channel is closed, goto `sclose` (panic). If there's a waiting receiver, perform direct send. If buffer has space, buffer the send.
- **Receive case**: If there's a waiting sender, perform direct receive. If buffer has data, receive from buffer. If the channel is closed, return zero value.

**Phase 3: Block**
If no case can proceed and there's no `default`:
1. Acquire a `sudog` for each case.
2. Enqueue each sudog on the appropriate channel's `sendq` or `recvq`.
3. Park the goroutine via `gopark()`.

**Phase 4: Wake-up**
When woken up:
1. Lock all channels again.
2. Identify which case succeeded (the sudog's `success` field).
3. Dequeue the sudog from all other channels.
4. Clean up and return the chosen case index.

### Key Design Points

- **Random fairness**: The polling order is randomized each time, preventing starvation.
- **Deadlock prevention**: Channels are locked in a consistent order (sorted by address).
- **Cap on cases**: The number of cases is capped at 65536 (`1 << 16`) to keep stack usage bounded.
- **Compiler optimizations**: Selects with 0 or 1 cases plus default are rewritten by the compiler into simpler constructs (e.g., `selectnbsend`/`selectnbrecv`).

### Select with `default`

The compiler rewrites `select` with a `default` case into a non-blocking send/receive:

```go
// compiler implements
//   select {
//   case c <- v:
//       ... foo
//   default:
//       ... bar
//   }
// as
//   if selectnbsend(c, v) {
//       ... foo
//   } else {
//       ... bar
//   }
```

---

## 5. Go's Philosophy: Share Memory by Communicating

### The Proverb

> Do not communicate by sharing memory; instead, share memory by communicating.
>
> — Effective Go, "Share by communicating"

### Explanation

The traditional concurrency model (pthreads, Java threads, etc.) involves multiple threads accessing shared data protected by locks. This leads to:

- Complex lock ordering reasoning
- Race conditions
- Deadlocks
- Difficult-to-reason-about code

Go's approach, derived from **CSP (Communicating Sequential Processes)** by Tony Hoare, inverts this:

> Instead of explicitly using locks to mediate access to shared data, Go encourages the use of channels to pass references to data between goroutines. This approach ensures that only one goroutine has access to the data at a given time.
>
> — "Share Memory By Communicating" blog post, 2010

### Practical Meaning

The philosophy means: **pass ownership of data through channels**. When a goroutine sends a value on a channel, it transfers ownership of that value to the receiving goroutine. The sender should not touch the value again after sending it.

```go
// Traditional: shared state with mutexes
type Resource struct {
    url        string
    polling    bool
    lastPolled int64
    lock       sync.Mutex
}

// Go idiom: pass ownership via channels
type Resource string

func Poller(in, out chan *Resource) {
    for r := range in {
        // poll the URL
        out <- r  // send back when done
    }
}
```

### Caveat

The "share memory by communicating" approach is not a panacea. The Go team acknowledges that simpler cases (like reference counts) are best handled with mutexes:

> This approach can be taken too far. Reference counts may be best done by putting a mutex around an integer variable, for instance. But as a high-level approach, using channels to control access makes it easier to write clear, correct programs.
>
> — Effective Go, "Share by communicating"

### The CSP Connection

Go's concurrency model is based on **Hoare's Communicating Sequential Processes (CSP)**, but with a procedural twist:

> Go's concurrency primitives derive from a different part of the family tree whose main contribution is the powerful notion of channels as first class objects. Experience with several earlier languages has shown that the CSP model fits well into a procedural language framework.
>
> — Go FAQ, "Why build concurrency on the ideas of CSP?"

The preceding languages include **Newsqueak** (Rob Pike), **Alef** (Phil Winterbottom), and **Limbo** (Pike, Winterbottom, etc.).

---

## 6. Comparison with OS Threads

| Aspect | OS Thread | Goroutine |
|--------|-----------|-----------|
| **Stack size** | Fixed, typically 1-8 MB | Starts at 2 KB, grows/shrinks dynamically |
| **Creation cost** | ~1 µs (kernel entry + accounting) | ~0.1 µs (user-space only) |
| **Context switch** | ~1-10 µs (kernel trap) | ~0.1 µs (3 instructions per function call) |
| **Number per process** | Thousands (limited by virtual memory) | Hundreds of thousands to millions |
| **Scheduling** | OS kernel scheduler (preemptive) | Go runtime scheduler (cooperative + async preemption) |
| **Identity** | Kernel-managed PID/TID | No user-visible ID (intentionally anonymous) |
| **Blocking** | Blocks an OS thread | Parks the G, M picks up another G |
| **Memory model** | Shared memory + locks | Channels + `sync` primitives |

### Key Differences in Detail

**Stack overhead**: An OS thread's stack is typically 1-8 MB and fixed. If you create 10,000 threads, you need 10-80 GB of virtual memory just for stacks. A goroutine starts at 2 KB, so 10,000 goroutines use ~20 MB.

**Context switching**: OS thread context switching requires a kernel trap (mode switch), saving/restoring registers, TLB flushes, etc. Goroutine switching happens entirely in user space — just saving/restoring a few registers (PC, SP, BP) and the G's state.

**Blocking behavior**: When an OS thread blocks on I/O, the entire thread is blocked. When a goroutine blocks on a channel or I/O, the Go runtime parks just that goroutine and the M picks up another runnable G. This is the M:N multiplexing advantage.

**No goroutine ID**: By design, goroutines have no user-visible ID:

> Goroutines do not have names; they are just anonymous workers. The fundamental reason goroutines are anonymous is so that the full Go language is available when programming concurrent code.
>
> — Go FAQ, "Why is there no goroutine ID?"

---

## 7. History: Why Goroutines Instead of OS Threads or Async/Await

### The Problem with OS Threads

When Go was designed in 2007, the programming landscape was dominated by C++ and Java, and the rise of multicore CPUs was creating new challenges:

> The rise of multicore CPUs argued that a language should provide first-class support for some sort of concurrency or parallelism.
>
> — Go FAQ, "What is the purpose of the project?"

OS threads were deemed unsuitable because:

1. **High memory overhead**: 1 MB+ stacks each.
2. **Slow creation**: Kernel involvement for every thread.
3. **Complex programming model**: The pthreads model (mutexes, condition variables, memory barriers) was considered too low-level and error-prone.
4. **Manual stack management**: The programmer had to worry about stack sizes.

### Why Not Async/Await

Go predates async/await (which was popularized by C# 2012, JavaScript ES2017, Rust 2015+). But more fundamentally, the Go team's philosophy favored **concurrency as a language primitive**, not a library feature:

- Async/await requires **coloring** of functions (async vs. sync), which Go designers explicitly rejected.
- Go's goroutines are **transparent**: any function can be called as a goroutine, and any function can block.
- The runtime manages the multiplexing automatically, so the programmer doesn't think about "is this function async?"

### The CSP Influence

The Go team chose to build concurrency on **CSP** rather than the traditional shared-memory threading model:

> Concurrency and multi-threaded programming have over time developed a reputation for difficulty. We believe this is due partly to complex designs such as pthreads and partly to overemphasis on low-level details such as mutexes, condition variables, and memory barriers. Higher-level interfaces enable much simpler code, even if there are still mutexes and such under the covers.
>
> — Go FAQ, "Why build concurrency on the ideas of CSP?"

### The Three Design Goals

Go's concurrency model was designed to solve three problems simultaneously:

1. **Efficiency**: Lightweight enough to create thousands.
2. **Simplicity**: The `go` keyword is all you need to spawn a concurrent task.
3. **Safety**: Channels provide communication with synchronization built in.

### Timeline

- **2007**: Go design begins at Google (Griesemer, Pike, Thompson).
- **2008**: First compiler (producing C code).
- **2009**: Go becomes open source (November 10).
- **2012**: Go 1.0 — goroutines and channels stabilized.
- **2015**: Go 1.5 — concurrent GC, GOMAXPROCS defaults to #cores, runtime rewritten in Go.
- **2018**: GC latency reduced to <500 µs typical.
- **2020**: Go 1.14 — asynchronous preemption (non-cooperative scheduling).

### What Go Did Not Do

The Go team deliberately avoided several approaches:

- **No async/await**: Keeps the language simple and avoids function coloring.
- **No actor model**: Unlike Erlang, Go does not isolate goroutines with separate heaps (they share the same address space).
- **No software transactional memory**: Considered but rejected as too complex.
- **No explicit goroutine identity**: Forces programmers to use channels for communication, not thread IDs.

---

## Sources

1. **Go Runtime Source Code**: `src/runtime/proc.go` — scheduler, G-M-P model, sysmon, work stealing
2. **Go Runtime Source Code**: `src/runtime/chan.go` — channel implementation (`hchan`, `chansend`, `chanrecv`, `closechan`)
3. **Go Runtime Source Code**: `src/runtime/select.go` — `select` statement implementation (`selectgo`)
4. **Go Runtime Source Code**: `src/runtime/HACKING.md` — scheduler structures, stacks, synchronization primitives
5. **Go FAQ**: "Why goroutines instead of threads?", "Why build concurrency on the ideas of CSP?"
6. **Effective Go**: "Share by communicating", "Goroutines", "Channels"
7. **Go Memory Model**: Channel communication synchronization guarantees
8. **Go 1.5 Release Notes**: Concurrent GC, GOMAXPROCS default change, runtime in Go
9. **Go Blog**: "Share Memory By Communicating" (2010)
10. **Go Blog**: "Concurrency is not parallelism" (2013)
11. **ISMM 2018 Keynote**: "Getting to Go: The Journey of Go's Garbage Collector" by Rick Hudson
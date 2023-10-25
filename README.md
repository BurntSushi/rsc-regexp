# Translations of a simple C program to Rust

This repository contains several translations of [Russ Cox's Thompson NFA C
program][c-nfa] to Rust. Cox's program features as one of a few supporting
programs in his article "[Regular Expression Matching Can Be Simple And
Fast][regexp1]."

I was motivated to do this exercise by [Andy Chu's][andy-chu] (of [Oil][oil]
fame) [questioning of whether Cox's program could be modeled using Rust's
borrow checker][andy-chu-question]. My belief is that the answer to that
question is a firm "no." More than that, I knew it would be "no" before
I started this exercise. So why do it? It's my belief that it would be
interesting to see how to translate a simple C program whose use of pointers
can't be modeled by the borrow checker. Indeed, I did three different
translations of the [original program](./original/nfa.c):

* A [**dumb**](./dumb-translation/nfa.rs) translation where I tried to copy the
original program as closely as possible, even if it meant using `unsafe` in
Rust. This means there are raw pointers and the borrow checker does not help
us (much). In this translation, I believe I've substantially preserved the
character of the original program, including its elegant use of pointers to
represent the NFA state graph.
* A [**safe**](./safe-translation/nfa.rs) translation that follows from the
**dumb** translation but with one extra restriction: no use of `unsafe`
allowed. This program does *not* preserve the character of the original program
as much as the **dumb** translation does, but it tries. The `PtrList` trick
in the original program is lost for example, and now we use reference counted
pointers to states. It's not so elegant.
* An [**idiomatic**](./idiomatic-translation/nfa.rs) translation that similarly
tries to preserve the character of the original program, but in a way that
"makes sense" for Rust. In this program, we use handles/indices to states
instead of pointers to states. This bypasses the borrow checker but still uses
no `unsafe` code. Many of the principle downsides of handles/indices do not
apply to this program, because once the state graph is built, it is immutable
until it is discarded. Moreover, unlike the original program and both the
**dumb** and **safe** translations, this program has no memory leaks.
* A [**rust-regex**](./rust-regex/nfa.rs) translation that preserves the
behavior of the original program, but uses the `regex` crate. This is for "fun"
comparison purposes only. And a way to sanity check my test suite.

## Building and testing

You'll need Clang and Cargo to build the original program and its translations.
After cloning this repository, you should just need to run the tests. The
harness will build the programs and test them for you:

```
$ ./test all
=== original ===
//badsyntax ... FAILED
a|/a/badsyntax ... FAILED
a.b/a.b/badsyntax ... FAILED
=== dumb-translation(rust) ===
=== safe-translation(rust) ===
=== idiomatic-translation(rust) ===
=== regex crate ===
```

The failing tests for the original program are expected.

## Target audience

The discussion below gets into the weeds pretty quickly. I'd suggest the
following:

* Reading at least the "implementation" sections in Cox's [blog][regexp1]. This
will give you the proper context for understanding the original program and
what it's actually doing.
* At least intermediate C and and Rust experience is assumed.
* Some passing familiarity with the [original program written by
Cox](./original/nfa.c).

## Goal

The goal of this exercise was to explore what the Rust translations would look
like. In particular, I think this may be helpful to folks looking to see how
to translate pointer techniques used in C to Rust. Assuredly, this is not a
general guide for doing so. However, I think it can serve nicely as a single
example for a specific C program that might partially generalize to other C
programs that use similar tricks. I say this with the experience of having
used the same techniques in the **idiomatic** translation in many other Rust
programs and libraries.

## Observations

Interested parties will want to reivew the code themselves. Each translation
has a comment at the top of the source file with some notes. With that said,
I feel there are some interesting observations to discuss in a broader context.

### Source lines of code

Lines of code is a not-so-good metric to measure complexity, but it can provide
a very rough feeling:

```
$ tokei --files --type C,Rust --columns 90 \
    original/nfa.c \
    dumb-translation/nfa.rs \
    safe-translation/nfa.rs \
    idiomatic-translation/nfa.rs
==========================================================================================
 Language                       Files        Lines         Code     Comments       Blanks
==========================================================================================
 C                                  1          419          304           78           37
------------------------------------------------------------------------------------------
 original/nfa.c                                419          304           78           37
------------------------------------------------------------------------------------------
 Rust                               3         1372          923          376           73
------------------------------------------------------------------------------------------
 idiomatic-translation/nfa.rs                  488          301          165           22
 safe-translation/nfa.rs                       450          317          107           26
 dumb-translation/nfa.rs                       434          305          104           25
```

Overall, the original program and its translations are almost exactly the same
length at around 300 source lines of code.

### The parser remains invariant

The parser essentially looks the same across all translations and it matches
up pretty well with the parser in the original C program. There are a few
differences worth noting:

* The translations don't use a global static buffer to store the postfix
version of the pattern. I could have done this in the **dumb** version using
`unsafe`, but given that `Vec<u8>` is in Rust's standard library and is very
simple to use, I decided to just put the postfix pattern on the heap. This also
has the advantage of making the parse function re-entrant.
* The translations don't use NUL terminated strings. I *could* have done that,
but I didn't see a good reason to. Instead I used a `Vec<u8>`. Technically a
`Vec<u8>` is far more complicated than a `char*` since it's a dynamically
growable vector on the heap, but its usage is very simple courtesy of the
standard library.
* I moved the state that tracks nested parenthetical expressions from the
stack to the heap.

Overall I felt that these changes did not alter the character of the parser
much if at all. I kept the same limits as the original parser even though they
aren't quite as important now that both the pattern and the nesting state are
on the heap.

### Leaks

In addition to the original program, the **dumb** and **safe** translation both
leak memory. The **dumb** translation leaks memory for the same reason that
the original does: there is no attempt to free any of the memory allocated
on the heap. It is clearly an intentional omission, likely in the name of
keeping the program simple. For the use case of teaching someone about the
Thompson NFA simulation via a short lived program, freeing any memory allocated
is superfluous since the operating system will automatically handle it upon
program termination.

In the case of the **safe** translation, it leaks memory because of cycles
created between reference counted pointers. Rust's [`std::rc::Rc`] type in
particular is documented to leak memory in the case of cycles, so this is
expected behavior. Rust's `Rc` pointer does support creating `Weak` pointers
that can break the cycle by not incrementing the reference count. However, I
could see no simple way of adapting the use of weak reference counted pointers
to this program in a way that prevents leaks.

The **idiomatic** translation does not have any memory leaks. Since this
translation works by putting all NFA states into one single allocation that
gets dropped automatically, there is no cyclic in-memory data structure. The
handles/indices of course still can form a cycle, but this doesn't impact
memory management.

I do think that the fact that this program is designed to leak in C in favor of
simplicity does limit its usefulness somewhat as a comparison point. Namely,
I think this calls into question Chu's characterization of the C program as
an example of elegance. It might be elegantly simple for narrow pedagogical
reasons, but it isn't reflective of robust code. In particular, the cyclic
graph that is created in memory by the C program is non-trivial to free
manually. Adding the necessary `free` logic to this program would bring it
closer to something "real," but it would almost certainly detract from its
simplistic elegance.

I also think that it is interesting to note that the **idiomatic** translation
does not have leaks and yet is arguably as simple as the original. Of
course, one could translate the **idiomatic** Rust program back into C by
using handles/indices in C, but this would likely detract from its elegance
when compared to Rust. For the handles/indices technique, Rust benefits
significantly from having a `Vec<T>` in its standard library. It's likely
that the elegant path to go in C is to compute the number of states one needs
to allocate in advance from the postfix syntax. That would avoid needing to
hand-write the code for a dynamically growable vector.

### Undefined behavior

In the course of this exercise, I found 3 distinct occurrences of undefined
behavior in the original program:

* Passing an empty pattern results in the original trying to do an unchecked
pop of an empty stack in `post2nfa`.
* Passing a pattern that ends with an alternation symbol, e.g., `a|`. This
pattern is considered valid and results in an unchecked pop of an empty stack
in `post2nfa`.
* Passing a pattern that contains a `.` can wreak all sorts of havoc, since `.`
is treated as a literal in `re2post` but as a meta character (the concatenation
operator) in `post2nfa`.

All of the translations to Rust, including the **dumb** translation, fix these
bugs by rejecting the patterns that provoke the undefined behavior.

While the existence of undefined behavior in a pedagogical toy program is not
necessarily significant, I do think these are legitimate bugs in the program.
Namely, the program puts in some effort to reject invalid patterns. That is,
it's doing *some* sanitization of the input, but it is not complete.

The above errors were found through unit testing (see the [`./test`](./test)
harness program). In some cases, the C program still "behaved normally," but
the Rust program tripped an assert. (The Rust translations move things like
stacks to the heap and assert that the stack is not empty before popping it.)

### Pointer tricks

I think the part of the original program I struggled with the most was its
use of the `Ptrlist` union type. In short, it uses type punning to store a
linked list of pointers that need to be patched to real states later in the
NFA construction process. While I was able to re-create the technique used
in the original in the **dumb** translation, the **safe** and **idiomatic**
translations lack this trick.

It's worth lingering on this trick for a moment, because it's one of the two
things in the original program that really inhibit the use of Rust's borrow
checker and ownership system without additional abstractions. (The other being
the cyclic graph of states.)

To explain this trick, let's look at the type definitions from the original
C program that we need to understand the trick:

```c
typedef struct State {
    int c;
    State *out;
    State *out1;
    int lastlist;
} State;

typedef struct Frag {
    State *start;
    Ptrlist *out;
} Frag;

typedef union Ptrlist {
    Ptrlist *next;
    State *s;
} Ptrlist;
```

In `post2nfa`, each character in the postfix expression of the pattern
corresponds to an action on a stack of `Frag` values and the creation of a new
NFA state. When a state is first created, its outgoing transitions (the `out`
and `out1` fields) aren't necessarily both known. For example, consider what
happens when a `+` repetition operator is seen in `post2nfa`:

```c
        case '+':    /* one or more */
            e = pop();
            s = state(Split, e.start, NULL);
            patch(e.out, s);
            push(frag(e.start, list1(&s->out1)));
            break;
```

This is popping a fragment (`e` has type `Frag`) off of a stack and then
building a `Split` state. The `Split` state knows one of its outgoing
transitions: `out` is set to `e.start` via the `state` constructor. But `out1`
is set to `NULL`. In effect, `Split` represents a fork in the execution of the
Thompson VM: it can either repeat the previous state (that's `s->out` which
is set to `e.start`) or it can go on to the next state (that's `s->out1`, but
unknown as of yet).

So when does the next state get patched in? That happens at some indeterminate
point later when `patch` is called. Note that the `patch` call in the fragment
above is not patching `s->out1`, but rather, the outgoing transitions on the
state (`e.out`) in the fragment (`e`) popped off the stack. Those outgoing
transitions are set to the the state we just created.

The trick is that the list of outgoing transitions to patch is maintained as
a linked list of pointers within the allocation of each `State` itself. The
linked list is created via `list1` in the above snippet. `list1` is as follows:

```c
Ptrlist* list1(State **outp) {
    Ptrlist *l;

    l = (Ptrlist*)outp;
    l->next = NULL;
    return l;
}
```

A list can also be created by appending two existing lists together:

```c
Ptrlist* append(Ptrlist *l1, Ptrlist *l2) {
    Ptrlist *oldl1;

    oldl1 = l1;
    while(l1->next)
        l1 = l1->next;
    l1->next = l2;
    return oldl1;
}
```

The trick here is that the memory locations used by the linked list are the
unset `out` or `out1` fields in the `State` struct itself. This works because
each `State` is given its own stable allocation that will never move.

Finally, `patch` traverses this list and updates the pointers to the next state
once the next state is known:

```c
void patch(Ptrlist *l, State *s) {
    Ptrlist *next;

    for(; l; l=next){
        next = l->next;
        l->s = s;
    }
}
```

Notice here that the `Ptrlist` given is treated as a linked list by following
`next` and then immediately treated as a `State*` and updated to the next
state given. This, crucially, works because the `post2nfa` algorithm ensures
that the `Ptrlist` values are used exactly once. If they were followed again
after `patch` is called you would wind up with undefined behavior.

The nice thing about this trick is that it reuses the allocations of `State`
values so that tracking the unpatched transitions doesn't require any extra
memory. Both of the **safe** and **idiomatic** translations opt to use extra
heap memory here instead. The tricky part about representing this in safe
Rust is that it uses interior pointers into an existing allocation. These are
allowed in safe Rust, but only as borrows. While there are some tricks to make
self-referencing structs work safely in Rust, the trick used this program
stores the interior pointers in a function's call stack, so it likely seems
even harder to do here. Perhaps a slab or bump allocator abstraction could help
here, but I'm not sure.

As a comparison point, let's look at how the **idiomatic** translation handles
this. We'll look at the types, just as we did with the C program:

```rust
enum State {
    Literal { byte: u8, out: StateID },
    Split { out1: StateID, out2: StateID },
    Match,
}

struct Frag {
    start: StateID,
    out: Vec<ToPatch>,
}

#[derive(Clone, Copy)]
enum ToPatch {
    // patch 'out' or 'out1' in given state
    Out1(StateID),
    // patch 'out2' in given state
    Out2(StateID),
}
```

Instead of using a linked list, we use a dynamically growable vector to
represent all of the outgoing transitions that need to be patched. A source of
inelegance here is that we need to keep track of *which* outgoing transition to
patch: either the first or second. Namely, the `StateID` handles we use here
(instead of `State` pointers in the C program) actually point to the state
containing the outgoing transition instead of the location in memory that needs
to be updated. That means we need to carry with it an instruction of which
transition to update. We can see more concretely what this means by looking at
our patch function:

```rust
impl NFA {
    // Perform all patch instructions such that all
    // handles point to the state given.
    fn patch(&mut self, l: &[ToPatch], s: StateID) {
        for &p in l.iter() {
            match p {
                ToPatch::Out1(sid) => match self.states[sid as usize] {
                    State::Literal { ref mut out, .. } => {
                        *out = s;
                    }
                    State::Split { ref mut out1, .. } => {
                        *out1 = s;
                    }
                    _ => unreachable!("invalid out1 patch"),
                },
                ToPatch::Out2(sid) => match self.states[sid as usize] {
                    State::Split { ref mut out2, .. } => {
                        *out2 = s;
                    }
                    _ => unreachable!("invalid out2 patch"),
                },
            }
        }
    }
}
```

Here, we iterate over our patch instructions, figure out what kind of patching
we need to do and then inspect the `State` itself to get access to the outgoing
transitions we need to patch. If, for example, we have a patch instruction for
the second outgoing transition but a non-split state, then we panic indicating
a bug. In the C program, this sort of error is still possible, but it would
likely manifest as a logical bug in matching somehow.

I'm torn on which approach is "better" here. The C program definitely wins the
elegance trophy for reusing the unset parts of a state as a linked list to
track future work that needs to be done. But, this was definitely the part of
the program that took me the longest to understand.

## Performance

As Cox says in his [blog][regexp1], the original C program "was not written
with performance in mind." I stuck to that as well and didn't bother thinking
too deeply about performance. Instead, the main thing to optimize for here
(in my opinion) is a working implementation that is simple enough to quickly
demonstrate the Thompson NFA simulation concept.

With that said, I was still curious how fast they were. So I devised a
"torture" test. The test is a little hokey because we have to work within the
limits imposed by the program (the parser has a fixed limit on the size of the
pattern) and the operating system (since the haystack is passed as an argument
to the process). The test consists of the following pattern

```
(abc)*d|(abc)*d|...|(abc)*d|(abc)*Z
```

matched against the following haystack:

```
abcabcabc...abcabcZ
```

where `...` represents repetition. The idea here is that all of the
alternations are "active" throughout the search but only the last one matches.
Each alternation corresponds to a "split" NFA state which all stack up on
each other. This results in an enormous amount of time spent chasing epsilon
transitions for each byte in the haystack.

To run the torture test, first make sure all of the programs are built:

```
$ SKIPTEST=1 ./test all
=== original ===
=== dumb-translation(rust) ===
=== safe-translation(rust) ===
=== idiomatic-translation(rust) ===
=== regex crate ===
```

And then either run the torture test for each program individually, e.g.,

```
$ time ./torture-test ./original/nfa
PASSED

real    0.252
user    0.246
sys     0.053
maxmem  26 MB
faults  0

$ time ./torture-test ./idiomatic-translation/target/release/nfa
PASSED

real    0.238
user    0.230
sys     0.056
maxmem  26 MB
faults  0
```

Or use a tool like [`hyperfine`] to bake them off against one another:

```
$ hyperfine -w5 \
    "./torture-test ./original/nfa" \
    "./torture-test ./dumb-translation/target/release/nfa" \
    "./torture-test ./safe-translation/target/release/nfa" \
    "./torture-test ./idiomatic-translation/target/release/nfa" \
    "./torture-test ./rust-regex/target/release/nfa"
Benchmark 1: ./torture-test ./original/nfa
  Time (mean ± σ):     179.6 ms ±   9.8 ms    [User: 178.7 ms, System: 1.4 ms]
  Range (min … max):   169.1 ms … 204.9 ms    16 runs

Benchmark 2: ./torture-test ./dumb-translation/target/release/nfa
  Time (mean ± σ):     153.5 ms ±   3.6 ms    [User: 151.8 ms, System: 2.1 ms]
  Range (min … max):   149.4 ms … 162.5 ms    19 runs

Benchmark 3: ./torture-test ./safe-translation/target/release/nfa
  Time (mean ± σ):     510.7 ms ±  11.3 ms    [User: 508.8 ms, System: 2.2 ms]
  Range (min … max):   486.1 ms … 528.9 ms    10 runs

Benchmark 4: ./torture-test ./idiomatic-translation/target/release/nfa
  Time (mean ± σ):     166.0 ms ±   4.1 ms    [User: 165.0 ms, System: 1.5 ms]
  Range (min … max):   163.0 ms … 175.4 ms    17 runs

Benchmark 5: ./torture-test ./rust-regex/target/release/nfa
  Time (mean ± σ):      12.5 ms ±   0.2 ms    [User: 5.8 ms, System: 7.2 ms]
  Range (min … max):    11.8 ms …  13.1 ms    212 runs

Summary
  './torture-test ./rust-regex/target/release/nfa' ran
   12.26 ± 0.37 times faster than './torture-test ./dumb-translation/target/release/nfa'
   13.26 ± 0.41 times faster than './torture-test ./idiomatic-translation/target/release/nfa'
   14.35 ± 0.83 times faster than './torture-test ./original/nfa'
   40.80 ± 1.20 times faster than './torture-test ./safe-translation/target/release/nfa'
```

In other words:

* The original program along with the **dumb** and **idiomatic** translations
have very similar performance characteristics.
* The **safe** translation is a fair bit slower. Light profiling of this
program suggests the difference comes from reference counting. (Via
`Rc::drop`.)
* The **rust-regex** program is quite a bit faster, but primarily because it
uses a different technique for this particular regex (a lazy DFA).

[regexp1]: https://swtch.com/~rsc/regexp/regexp1.html
[c-nfa]: https://swtch.com/~rsc/regexp/nfa.c.txt
[andy-chu-question]: https://lobste.rs/s/zhbv0i/object_soup_is_made_indexes#c_42wcoa
[andy-chu]: https://andychu.net/
[oil]: https://www.oilshell.org/
[`hyperfine`]: https://github.com/sharkdp/hyperfine
[`std::rc::Rc`]: https://doc.rust-lang.org/std/rc/index.html

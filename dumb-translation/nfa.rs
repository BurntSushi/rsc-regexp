// This program is meant to be a fairly direct translation from the original
// C program. That is, it uses pointer idioms from C that are extremely rare
// to use in Rust. As a result, I'm not even particularly confident that either
// the C program or the Rust program are free from UB. The stuff around the
// `union` in particular looks fishy because it looks like we're reading and
// writing from different union fields.
//
// One can run this program under miri via `cargo run miri ...`, and if you
// ignore the errors about leaks (which is okay given the task, given that the
// original program doesn't have a single call to `free`), everything looks
// okay. So that gives me some confidence that this program is correct, but I'm
// still not 100% sure. In other words, don't use this program or the code in
// it for anything serious.
//
// Note that this program isn't a 100% dumb mechanical translation. For
// example, we use `String` and `&[u8]` and such conveniences instead of NUL
// terminated strings. Also, for places in the code that use stacks (as a
// data structure), we put them on the heap instead of relying on a fixed
// stack allocation. We do retain the limits of the original program, so as to
// prevent unbounded heap growth. I took these liberties because they don't
// meaningfully impact the structure or logic of the program and they were
// very easy to do. I also justified the use of the heap in lieu of stack
// allocations because the original program does use `malloc` in places,
// so it's not like using the heap changes something fundamental about the
// program's enivronmental requirements.
//
// Another liberty I took was to use `Box<[*mut State]>` instead of
// `*mut *mut State`. I did implement the latter initially and there's
// no real problem with it. You do need to use `std::alloc::alloc`. But
// I switched it to something a bit more automatic because, again, it didn't
// seem necessary to write out all the plumbing myself.
//
// The key thing I did not attempt to simplify was the use of `*mut State`
// everywhere. These pointers are copied and referenced in various places.
// That means `Box<State>` is not possible to use because it implies unique
// ownership.
//
// You could almost get away with `Arc<State>`, but the representation used
// by the original program includes a list generation number inside the state
// itself. It's an optimization meant to quickly track whether a particular
// state is already in particular list or not. If you know it is, then there's
// no need to spend extra work adding it. This is useful because it's not
// uncommon to revisit the same state multiple times through each step of the
// Thompson NFA simulation. I could have devised a way around this by
// de-coupling the optimization from the representation of `State`, but I think
// that would change the character of the original program a bit too much for
// a "dumb translation." And since we use `*mut State`, pretty much anything
// that accepts a `*mut State` (which is almost everything) and does anything
// interesting with it also needs to be marked as not-safe. Otherwise the
// routine is almost certainly unsound.

use std::{
    process::ExitCode,
    ptr::{addr_of_mut, null_mut},
    sync::atomic::{AtomicI32, Ordering},
};

// Convert infix regexp re to postfix notation.
// Insert . as explicit concatenation operator.
// Returns `None` for invalid patterns.
fn re2post(re: &[u8]) -> Option<Vec<u8>> {
    struct Paren {
        nalt: i32,
        natom: i32,
    }

    // Unlike the original program, we reject the
    // empty pattern as invalid. This avoids an
    // error case in post2nfa.
    if re.is_empty() {
        return None;
    }
    if re.len() >= 8000 / 2 {
        return None;
    }
    let (mut nalt, mut natom) = (0, 0);
    let mut paren = vec![];
    let mut dst = vec![];
    for &byte in re.iter() {
        match byte {
            b'(' => {
                if natom > 1 {
                    natom -= 1;
                    dst.push(b'.');
                }
                if paren.len() >= 100 {
                    return None;
                }
                paren.push(Paren { nalt, natom });
                nalt = 0;
                natom = 0;
            }
            b'|' => {
                if natom == 0 {
                    return None;
                }
                natom -= 1;
                while natom > 0 {
                    dst.push(b'.');
                    natom -= 1;
                }
                nalt += 1;
            }
            b')' => {
                let p = paren.pop()?;
                if natom == 0 {
                    return None;
                }
                natom -= 1;
                while natom > 0 {
                    dst.push(b'.');
                    natom -= 1;
                }
                while nalt > 0 {
                    dst.push(b'|');
                    nalt -= 1;
                }
                nalt = p.nalt;
                natom = p.natom;
                natom += 1;
            }
            b'*' | b'+' | b'?' => {
                if natom == 0 {
                    return None;
                }
                dst.push(byte);
            }
            // Not handled in the original program.
            // Since '.' is a meta character in the
            // postfix syntax, it can result in UB.
            // So we reject it here.
            b'.' => return None,
            _ => {
                if natom > 1 {
                    natom -= 1;
                    dst.push(b'.');
                }
                dst.push(byte);
                natom += 1;
            }
        }
    }
    if !paren.is_empty() {
        return None;
    }
    // The original program doesn't handle this case, which in turn
    // causes UB in post2nfa. It occurs when a pattern ends with a |.
    // Other cases like `a||b` and `(a|)` are rejected correctly above.
    if natom == 0 && nalt > 0 {
        return None;
    }
    natom -= 1;
    while natom > 0 {
        dst.push(b'.');
        natom -= 1;
    }
    while nalt > 0 {
        dst.push(b'|');
        nalt -= 1;
    }
    Some(dst)
}

// Represents an NFA state plus zero or one or two arrows exiting.
// if c == Match, no arrows out; matching state.
// If c == Split, unlabeled arrows to out and out1 (if != NULL).
// If c < 256, labeled arrow with character c to out.
const MATCH: i32 = 256;
const SPLIT: i32 = 257;

struct State {
    c: i32,
    out: *mut State,
    out1: *mut State,
    lastlist: i32,
}

// The original uses unsynchronized shared mutable state for this. We use an
// atomic because it's trivial to do so and is safe.
static NSTATE: AtomicI32 = AtomicI32::new(0);

// matching state
static mut MATCH_STATE: State =
    State { c: MATCH, out: null_mut(), out1: null_mut(), lastlist: 0 };

impl State {
    // Allocate and initialize State
    fn new(c: i32, out: *mut State, out1: *mut State) -> *mut State {
        NSTATE.fetch_add(1, Ordering::AcqRel);
        let state = Box::new(State { c, out, out1, lastlist: 0 });
        Box::into_raw(state)
    }
}

// A partially built NFA without the matching state filled in.
// Frag.start points at the start state.
// Frag.out is a list of places that need to be set to the
// next state for this fragment.
struct Frag {
    start: *mut State,
    out: *mut PtrList,
}

impl Frag {
    // Initialize Frag struct.
    fn new(start: *mut State, out: *mut PtrList) -> Frag {
        Frag { start, out }
    }
}

// Since the out pointers in the list are always
// uninitialized, we use the pointers themselves
// as storage for the Ptrlists.
union PtrList {
    next: *mut PtrList,
    s: *mut State,
}

impl PtrList {
    // Create singleton list containing just outp.
    unsafe fn list1(outp: *mut *mut State) -> *mut PtrList {
        let l = outp.cast::<PtrList>();
        (*l).next = null_mut();
        l
    }

    // Patch the list of states at out to point to start.
    unsafe fn patch(mut l: *mut PtrList, s: *mut State) {
        while !l.is_null() {
            let next = (*l).next;
            (*l).s = s;
            l = next;
        }
    }

    // Join the two lists l1 and l2, returning the combination.
    unsafe fn append(mut l1: *mut PtrList, l2: *mut PtrList) -> *mut PtrList {
        let oldl1: *mut PtrList = l1;
        while !(*l1).next.is_null() {
            l1 = (*l1).next;
        }
        (*l1).next = l2;
        oldl1
    }
}

// Convert postfix regular expression to NFA.
// Return start state.
fn post2nfa(postfix: &[u8]) -> *mut State {
    let mut stack: Vec<Frag> = vec![];
    for &p in postfix.iter() {
        match p {
            // catenate
            b'.' => {
                let e2 = stack.pop().unwrap();
                let e1 = stack.pop().unwrap();
                unsafe {
                    PtrList::patch(e1.out, e2.start);
                }
                stack.push(Frag::new(e1.start, e2.out));
            }
            // alternate
            b'|' => {
                let e2 = stack.pop().unwrap();
                let e1 = stack.pop().unwrap();
                let s = State::new(SPLIT, e1.start, e2.start);
                let list = unsafe { PtrList::append(e1.out, e2.out) };
                stack.push(Frag::new(s, list));
            }
            // zero or one
            b'?' => {
                let e = stack.pop().unwrap();
                let s = State::new(SPLIT, e.start, null_mut());
                let list = unsafe {
                    PtrList::append(
                        e.out,
                        PtrList::list1(addr_of_mut!((*s).out1)),
                    )
                };
                stack.push(Frag::new(s, list));
            }
            // zero or more
            b'*' => {
                let e = stack.pop().unwrap();
                let s = State::new(SPLIT, e.start, null_mut());
                unsafe {
                    PtrList::patch(e.out, s);
                }
                let list = unsafe { PtrList::list1(addr_of_mut!((*s).out1)) };
                stack.push(Frag::new(s, list));
            }
            // one or more
            b'+' => {
                let e = stack.pop().unwrap();
                let s = State::new(SPLIT, e.start, null_mut());
                unsafe {
                    PtrList::patch(e.out, s);
                }
                let list = unsafe { PtrList::list1(addr_of_mut!((*s).out1)) };
                stack.push(Frag::new(e.start, list));
            }
            _ => {
                let s = State::new(i32::from(p), null_mut(), null_mut());
                let list = unsafe { PtrList::list1(addr_of_mut!((*s).out)) };
                stack.push(Frag::new(s, list));
            }
        }
    }
    // The original program assumes a stack pop
    // here is always correct. But it isn't! In
    // the case of an empty pattern, the original
    // program has UB but appears to behave "fine."
    // In our case, we reject the empty pattern as
    // invalid, and thus this unwrap can never be
    // reached.
    let e = stack.pop().unwrap();
    if !stack.is_empty() {
        return null_mut();
    }
    unsafe {
        PtrList::patch(e.out, addr_of_mut!(MATCH_STATE));
    }
    e.start
}

struct List {
    s: Box<[*mut State]>,
    n: i32,
}

static LIST_ID: AtomicI32 = AtomicI32::new(0);

impl List {
    // Compute initial state list
    unsafe fn start(&mut self, start: *mut State) -> &mut List {
        self.n = 0;
        LIST_ID.fetch_add(1, Ordering::AcqRel);
        self.add_state(start);
        self
    }

    // Check whether state list contains a match.
    unsafe fn is_match(&mut self) -> bool {
        for i in 0..self.n {
            if self.s[i as usize] == addr_of_mut!(MATCH_STATE) {
                return true;
            }
        }
        false
    }

    // Add s to l, following unlabeled arrows.
    unsafe fn add_state(&mut self, s: *mut State) {
        if s.is_null() || (*s).lastlist == LIST_ID.load(Ordering::Acquire) {
            return;
        }
        (*s).lastlist = LIST_ID.load(Ordering::Acquire);
        if (*s).c == SPLIT {
            // follow unlabeled arrows
            self.add_state((*s).out);
            self.add_state((*s).out1);
            return;
        }
        self.s[self.n as usize] = s;
        self.n += 1;
    }
}

// Step the NFA from the states in clist
// past the character c,
// to create next NFA state set nlist.
unsafe fn step(clist: &mut List, c: i32, nlist: &mut List) {
    LIST_ID.fetch_add(1, Ordering::AcqRel);
    nlist.n = 0;
    for i in 0..clist.n {
        let s = clist.s[i as usize];
        if (*s).c == c {
            nlist.add_state((*s).out);
        }
    }
}

// Run NFA to determine whether it matches s.
unsafe fn r#match(
    l1: &mut List,
    l2: &mut List,
    start: *mut State,
    s: &[u8],
) -> bool {
    let clist = l1.start(start);
    let nlist = l2;
    for &byte in s.iter() {
        step(clist, i32::from(byte), nlist);
        std::mem::swap(clist, nlist);
    }
    clist.is_match()
}

fn main() -> ExitCode {
    let mut argv = std::env::args_os();
    if argv.len() < 3 {
        eprintln!("usage: nfa regexp string...");
        return ExitCode::FAILURE;
    }

    let Ok(pattern) = argv.by_ref().skip(1).next().unwrap().into_string()
    else {
        eprintln!("pattern is invalid UTF-8");
        return ExitCode::FAILURE;
    };
    let Some(post) = re2post(pattern.as_bytes()) else {
        eprintln!("bad regexp {pattern}");
        return ExitCode::FAILURE;
    };
    let start = post2nfa(&post);
    if start.is_null() {
        eprintln!("error in post2nfa {pattern}");
        return ExitCode::FAILURE;
    }

    let nstate = NSTATE.load(Ordering::Acquire) as usize;
    let mut l1 = List { s: vec![null_mut(); nstate].into_boxed_slice(), n: 0 };
    let mut l2 = List { s: vec![null_mut(); nstate].into_boxed_slice(), n: 0 };
    for arg in argv {
        let Ok(haystack) = arg.into_string() else {
            eprintln!("haystack is invalid UTF-8");
            return ExitCode::FAILURE;
        };
        if unsafe { r#match(&mut l1, &mut l2, start, haystack.as_bytes()) } {
            println!("{haystack}");
        }
    }
    ExitCode::SUCCESS
}

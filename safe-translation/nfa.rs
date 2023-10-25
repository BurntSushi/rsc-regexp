// Unlike the dumb translation, this program is meant to be a translation that
// tries to preserve the character of the original program as much as possible
// but limited to only using safe code.
//
// There are two critical differences from the original program:
//
// * We replace *mut State with Rc<RefCell<State>>. The structure of the
// original program uses *mut State liberally. It not only doesn't adhere to
// an ownership pattern, but the graph of states created by the *mut State
// pointers is cyclic. Since this program copies the original's logic as much
// as possible, the graph formed by the Rc<RefCell<State>> pointers is also
// cyclic and thus leaks memory. (Rust's Arc and Rc types are documented to
// leak memory on cycles.)
//
// * We replace the raw union representation of PtrList with a more explicit
// sum type. Rust really has no hope of capturing the technique used in the
// original program safely. The original program type puns interior pointers
// into a State allocation to track which spots in memory need to be updated
// when patching together NFA fragments. We forget about trying to reuse the
// state allocation and just create our own linked list.
//
// If one wants to observe the memory leak in this program, you can use Miri
// to sniff it out:
//
//     cargo miri run -q --manifest-path safe-translation/Cargo.toml 'a+' 'a'
//
// Otherwise there are very few changes here. re2post is completely untouched.
// post2nfa is also largely the same with some minor changes to support the
// new PtrList representation described above. In fact, the structure of
// the original program is largely preserved. We mostly only change some
// representational details. Of course, do incur some extra costs by doing
// things "safely" (e.g., reference counting where there was none before), but
// it's unclear how to quantify that. Namely, these programs were never written
// with performance in mind, and evaluating their performance would really be
// an entirely different exercise.
//
// I think a fair conclusion to draw here is that we can largely preserve the
// character of the original program using safe Rust, but likely cannot do it
// without some added costs. In this case, those costs are likely both in CPU
// time and memory usage.

#![forbid(unsafe_code)]

use std::{
    cell::{Cell, RefCell},
    process::ExitCode,
    rc::Rc,
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
            // postfix syntax, it can wreak havoc
            // if we allow it here.
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
    out: Option<Rc<RefCell<State>>>,
    out1: Option<Rc<RefCell<State>>>,
    // If we use Rc<RefCell<State>> everywhere,
    // why do we use another layer of interior
    // mutability here? Because the state graph
    // is cyclic, and this field is actually part
    // of the search state. To avoid even runtime
    // borrowck errors, we either need to
    // excessively clone our state pointers or use
    // another layer of interior mutability.
    lastlist: Cell<i32>,
}

// The original uses unsynchronized shared mutable state for this. We use an
// atomic because it's trivial to do so and is safe.
static NSTATE: AtomicI32 = AtomicI32::new(0);

impl State {
    // Allocate and initialize State
    fn new(
        c: i32,
        out: Option<Rc<RefCell<State>>>,
        out1: Option<Rc<RefCell<State>>>,
    ) -> Rc<RefCell<State>> {
        NSTATE.fetch_add(1, Ordering::AcqRel);
        let state = State { c, out, out1, lastlist: Cell::new(0) };
        Rc::new(RefCell::new(state))
    }
}

// A partially built NFA without the matching state filled in.
// Frag.start points at the start state.
// Frag.out is a list of places that need to be set to the
// next state for this fragment.
struct Frag {
    start: Rc<RefCell<State>>,
    out: Box<PtrList>,
}

impl Frag {
    // Initialize Frag struct.
    fn new(start: Rc<RefCell<State>>, out: Box<PtrList>) -> Frag {
        Frag { start, out }
    }
}

// Represents a list of states to patch. Unlike
// the original, this tracks the parent states with
// a tag indicating whether to patch out or out1.
enum PtrList {
    None,
    Out(Rc<RefCell<State>>, Box<PtrList>),
    Out1(Rc<RefCell<State>>, Box<PtrList>),
}

impl PtrList {
    // Create singleton list that patches parent.out.
    fn out(parent: &Rc<RefCell<State>>) -> Box<PtrList> {
        Box::new(PtrList::Out(Rc::clone(parent), Box::new(PtrList::None)))
    }

    // Create singleton list that patches parent.out1.
    fn out1(parent: &Rc<RefCell<State>>) -> Box<PtrList> {
        Box::new(PtrList::Out1(Rc::clone(parent), Box::new(PtrList::None)))
    }

    // Patch the out pointers of the states in l to point to s.
    fn patch(mut l: Box<PtrList>, s: &Rc<RefCell<State>>) {
        loop {
            match *l {
                PtrList::None => return,
                PtrList::Out(parent, tail) => {
                    parent.borrow_mut().out = Some(Rc::clone(s));
                    l = tail;
                }
                PtrList::Out1(parent, tail) => {
                    parent.borrow_mut().out1 = Some(Rc::clone(s));
                    l = tail;
                }
            }
        }
    }

    // Join the two lists l1 and l2, returning the combination.
    fn append(mut l1: Box<PtrList>, l2: Box<PtrList>) -> Box<PtrList> {
        let mut p = &mut *l1;
        loop {
            match *p {
                PtrList::Out(_, ref mut tail)
                | PtrList::Out1(_, ref mut tail) => p = tail,
                PtrList::None => {
                    *p = *l2;
                    return l1;
                }
            }
        }
    }
}

// Convert postfix regular expression to NFA.
// Return start state.
fn post2nfa(postfix: &[u8]) -> Option<Rc<RefCell<State>>> {
    let mut stack: Vec<Frag> = vec![];
    for &p in postfix.iter() {
        match p {
            // catenate
            b'.' => {
                let e2 = stack.pop().unwrap();
                let e1 = stack.pop().unwrap();
                PtrList::patch(e1.out, &e2.start);
                stack.push(Frag::new(e1.start, e2.out));
            }
            // alternate
            b'|' => {
                let e2 = stack.pop().unwrap();
                let e1 = stack.pop().unwrap();
                let s = State::new(SPLIT, Some(e1.start), Some(e2.start));
                let list = PtrList::append(e1.out, e2.out);
                stack.push(Frag::new(s, list));
            }
            // zero or one
            b'?' => {
                let e = stack.pop().unwrap();
                let s = State::new(SPLIT, Some(e.start), None);
                let list = PtrList::append(e.out, PtrList::out1(&s));
                stack.push(Frag::new(s, list));
            }
            // zero or more
            b'*' => {
                let e = stack.pop().unwrap();
                let s = State::new(SPLIT, Some(e.start), None);
                PtrList::patch(e.out, &s);
                let list = PtrList::out1(&s);
                stack.push(Frag::new(s, list));
            }
            // one or more
            b'+' => {
                let e = stack.pop().unwrap();
                let s = State::new(SPLIT, Some(e.start.clone()), None);
                PtrList::patch(e.out, &s);
                let list = PtrList::out1(&s);
                stack.push(Frag::new(e.start, list));
            }
            _ => {
                let s = State::new(i32::from(p), None, None);
                let list = PtrList::out(&s);
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
        return None;
    }
    // In the original, a single match state is
    // re-used. Here, we create a new one every
    // time we need it. A bit wasteful, but safely
    // representing a single global match state
    // given our Rc pointers means switching to Arc.
    PtrList::patch(e.out, &State::new(MATCH, None, None));
    Some(e.start)
}

struct List {
    s: Box<[Rc<RefCell<State>>]>,
    n: i32,
}

static LIST_ID: AtomicI32 = AtomicI32::new(0);

impl List {
    // Compute initial state list
    fn start(&mut self, start: Rc<RefCell<State>>) -> &mut List {
        self.n = 0;
        LIST_ID.fetch_add(1, Ordering::AcqRel);
        self.add_state(Some(&start));
        self
    }

    // Check whether state list contains a match.
    fn is_match(&mut self) -> bool {
        for i in 0..self.n {
            if self.s[i as usize].borrow().c == MATCH {
                return true;
            }
        }
        false
    }

    // Add s to l, following unlabeled arrows.
    fn add_state(&mut self, s: Option<&Rc<RefCell<State>>>) {
        let Some(s) = s else { return };
        if s.borrow().lastlist.get() == LIST_ID.load(Ordering::Acquire) {
            return;
        }
        s.borrow().lastlist.set(LIST_ID.load(Ordering::Acquire));
        if s.borrow().c == SPLIT {
            // follow unlabeled arrows
            self.add_state(s.borrow().out.as_ref());
            self.add_state(s.borrow().out1.as_ref());
            return;
        }
        self.s[self.n as usize] = Rc::clone(s);
        self.n += 1;
    }
}

// Step the NFA from the states in clist
// past the character c,
// to create next NFA state set nlist.
fn step(clist: &mut List, c: i32, nlist: &mut List) {
    LIST_ID.fetch_add(1, Ordering::AcqRel);
    nlist.n = 0;
    for i in 0..clist.n {
        let s = &clist.s[i as usize];
        if s.borrow().c == c {
            nlist.add_state(s.borrow().out.as_ref());
        }
    }
}

// Run NFA to determine whether it matches s.
fn r#match(
    l1: &mut List,
    l2: &mut List,
    start: Rc<RefCell<State>>,
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
    let Some(start) = post2nfa(&post) else {
        eprintln!("error in post2nfa {pattern}");
        return ExitCode::FAILURE;
    };

    let nstate = NSTATE.load(Ordering::Acquire) as usize;
    let mut l1 = List {
        s: vec![State::new(0, None, None); nstate].into_boxed_slice(),
        n: 0,
    };
    let mut l2 = List {
        s: vec![State::new(0, None, None); nstate].into_boxed_slice(),
        n: 0,
    };
    for arg in argv {
        let Ok(haystack) = arg.into_string() else {
            eprintln!("haystack is invalid UTF-8");
            return ExitCode::FAILURE;
        };
        if r#match(&mut l1, &mut l2, start.clone(), haystack.as_bytes()) {
            println!("{haystack}");
        }
    }
    ExitCode::SUCCESS
}

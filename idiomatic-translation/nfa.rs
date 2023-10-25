// This version of the C program tries to represent something approximating an
// "idiomatic" Rust program. A more precise description might be "idiomatic
// but also simplistic." That is, like the previous translations, we do try
// to retain the character of the original. If the essence of the original
// program is to be small and digestible implementation of a Thompson NFA
// simulation, then we attempt to retain that essence here. For example, we
// don't bother with enhancing the parser to improve failure modes because that
// would distract from the primary goal: demonstrate the Thompson NFA matching
// algorithm.
//
// Instead, the principal change we make here is to replace the use of pointers
// to states with "handles" or indices to states. So instead of a `*State` or a
// `*mut State` or a `Rc<RefCell<State>>`, we have a `u32`. While such a change
// might not make a ton of sense in the original C program, it represents a
// considerable simplification to the Rust program. In particular, a lot of
// Rust's safety enforcements come from its borrow checker and that in turn
// is focused on carefully ensuring pointers aren't used deleteriously. In so
// doing, the borrow checker rejects some "valid" uses of pointers in favor of
// a more constrainer but safer paradigm. Indeed, the way in which pointers are
// used in the original C program cannot be (I believe) modeled by the Rust
// borrow checker.
//
// Once we swap the pointers out for handles though, the borrow checker no
// longer cares how we use those handles. It might look like that means we've
// given up some compiler checks and thus give up the correctness guarantees
// that Rust is supposed to give us. But there are some mitigating circumstances
// here:
//
// * Since "dereferencing" a handle is the same as indexing into a `Vec<State>`
// and indexing has bounds checks, we at least will get a runtime panic for a
// dangling handle. And of course, compared to C, there's no opportunity for
// UB.
//
// * Because an NFA becomes completely immutable after it's built, there is
// no real worry about using a handle after we've discarded the corresponding
// state. That is, if a state is discarded, it's only because every other state
// is also being discarded along with the NFA itself. The original C program
// doesn't suffer from this class of a problem either because it doesn't `free`
// anything.
//
// Using handles or indices instead of real pointers does make the code a
// little noisier, but the translation is otherwise very straight-forward for
// this program. And one advantage of the handle representation irrespective
// of Rust is that we can use a u32 to represent handles even in 64-bit
// environments. This means that the NFA needs only half the space to represent
// itself than it would with real pointers.
//
// I would call this technique idiomatic because it's the same technique that
// the regex crate uses to represent its own NFA.
//
// While switching to handles is definitely the biggest change, there are a few
// other changes I made as well:
//
// * I got rid of all shared global mutable state. Instead, it's encapsulated
// in a new type called `Matcher`. This works much more nicely for Rust because
// Rust forces shared global mutable state to be safe even in the face of
// multiple threads. Such a thing would be an unnecessary complication in a
// pedagogical single threaded program.
//
// * I moved the "last list ID" optimization off of the `State` type and into
// the `Matcher` type. The original program puts the last list ID on the
// `State` type itself, and as a result, is the only part of the NFA that is
// mutable after it's built. Because this program uses handles, there's no
// reason why we couldn't do that here too. That is, the borrow checker doesn't
// prevent us from mutating the NFA state. Instead, I moved it because I found
// it to make the program logic clearer.
//
// * A `State` is a sum type instead of a product type. In the original
// program, it is conceptually a sum type but uses a product representation. I
// don't know why exactly. One would need a tagged union in C I think and it
// might have just been too much ceremony. But sums are natural in Rust.
//
// * The `PtrList` stuff is replaced with a simpler but perhaps more wasteful
// `Vec` of state handles to patch. The original program embeds a linked list
// into the as-yet-unused parts of a `State`. It's not clear how this could be
// done with handles, and since performance isn't a concern and things like
// `Vec` are easily usable (unlike in C), I chose to just use a more explicit
// representation.

#![forbid(unsafe_code)]

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

// NFA states in a single contiguous
// allocation. States contain indices
// into this NFA instead of pointers
// directly to other states that they
// transition to.
struct NFA {
    start: StateID,
    states: Vec<State>,
}

// The type of a state handle. These
// are meant to be always-valid indices
// into NFA::states.
type StateID = u32;

// A state matches a literal byte,
// or splits execution to two other states,
// or indicates a match.
enum State {
    Literal { byte: u8, out: StateID },
    Split { out1: StateID, out2: StateID },
    Match,
}

// A partial NFA fragment with a start state
// and a list of instructions to create valid
// handles to the next state.
struct Frag {
    start: StateID,
    out: Vec<ToPatch>,
}

// An instruction to patch a state's out/out1
// or out2 handle to a valid state.
#[derive(Clone, Copy)]
enum ToPatch {
    // patch 'out' or 'out1' in given state
    Out1(StateID),
    // patch 'out2' in given state
    Out2(StateID),
}

impl NFA {
    // Convert postfix regular expression to NFA.
    // Return start state.
    fn post2nfa(postfix: &[u8]) -> Option<NFA> {
        let mut nfa = NFA { start: 0, states: vec![] };
        let mut stack: Vec<Frag> = vec![];
        for &byte in postfix.iter() {
            match byte {
                // catenate
                b'.' => {
                    let e2 = stack.pop().unwrap();
                    let e1 = stack.pop().unwrap();
                    nfa.patch(&e1.out, e2.start);
                    stack.push(Frag { start: e1.start, out: e2.out });
                }
                // alternate
                b'|' => {
                    let e2 = stack.pop().unwrap();
                    let mut e1 = stack.pop().unwrap();
                    let s = nfa.alloc(State::Split {
                        out1: e1.start,
                        out2: e2.start,
                    });
                    e1.out.extend(e2.out);
                    stack.push(Frag { start: s, out: e1.out });
                }
                // zero or one
                b'?' => {
                    let mut e = stack.pop().unwrap();
                    let s = nfa.alloc(State::Split { out1: e.start, out2: 0 });
                    e.out.push(ToPatch::Out2(s));
                    stack.push(Frag { start: s, out: e.out });
                }
                // zero or more
                b'*' => {
                    let e = stack.pop().unwrap();
                    let s = nfa.alloc(State::Split { out1: e.start, out2: 0 });
                    nfa.patch(&e.out, s);
                    let out = vec![ToPatch::Out2(s)];
                    stack.push(Frag { start: s, out });
                }
                // one or more
                b'+' => {
                    let e = stack.pop().unwrap();
                    let s = nfa.alloc(State::Split { out1: e.start, out2: 0 });
                    nfa.patch(&e.out, s);
                    let out = vec![ToPatch::Out2(s)];
                    stack.push(Frag { start: e.start, out });
                }
                _ => {
                    let s = nfa.alloc(State::Literal { byte, out: 0 });
                    let out = vec![ToPatch::Out1(s)];
                    stack.push(Frag { start: s, out });
                }
            }
        }
        let e = stack.pop().unwrap();
        if !stack.is_empty() {
            return None;
        }
        let s = nfa.alloc(State::Match);
        nfa.start = e.start;
        nfa.patch(&e.out, s);
        Some(nfa)
    }

    // Puts the given state on the heap and returns a stable
    // identifier for that state.
    fn alloc(&mut self, state: State) -> StateID {
        let id = self.states.len();
        self.states.push(state);
        // Our parser limits ensure this always succeeds.
        StateID::try_from(id).expect("less than StateID::MAX states")
    }

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

// A matcher encapsulates the state
// of searching for a regex match.
struct Matcher {
    // the nfa to use for matching
    nfa: NFA,
    // first or "current" list
    clist: List,
    // second or "next" list
    nlist: List,
    // the ID of the currently active 'next' list
    list_id: u32,
    // map from state handle to list ID
    last_list_id: Box<[u32]>,
}

// A list of state handles of length n.
struct List {
    s: Box<[StateID]>,
    n: usize,
}

impl Matcher {
    // create a matcher for the given NFA
    fn new(nfa: NFA) -> Matcher {
        let list = vec![0; nfa.states.len()].into_boxed_slice();
        let clist = List { s: list.clone(), n: 0 };
        let nlist = List { s: list, n: 0 };
        let last_list_id = vec![0; nfa.states.len()].into_boxed_slice();
        Matcher { nfa, clist, nlist, last_list_id, list_id: 0 }
    }

    // return true if the haystack matches
    fn is_match(&mut self, haystack: &[u8]) -> bool {
        self.start();
        for &byte in haystack.iter() {
            self.step(byte);
            std::mem::swap(&mut self.clist, &mut self.nlist);
        }
        self.clist.s[..self.clist.n]
            .iter()
            .any(|&sid| matches!(self.nfa.states[sid as usize], State::Match))
    }

    // add starting states to clist
    fn start(&mut self) {
        self.increment_list_id();
        // we add the states to nlist first, since
        // that's what add_state_to_next does, and
        // then just swap the lists
        self.nlist.n = 0;
        self.add_state_to_next(self.nfa.start);
        std::mem::swap(&mut self.clist, &mut self.nlist);
    }

    // step over all states in clist and add matching states to nlist
    fn step(&mut self, haystack_byte: u8) {
        self.increment_list_id();
        self.nlist.n = 0;
        // This is a good example of how borrowck can inhibit composition. We
        // would ideally want to use `self.clist.s[..self.clist.n].iter()` here
        // and iterate over the state handles directly. Instead, we iterate
        // over indices into the `clist` and then lookup the state handle in a
        // subsequent step. Why?
        //
        // If we use the iterator, then it necessarily borrows `self.clist`.
        // That's not necessarily an issue on its own, but during iteration,
        // we want to call `self.add_state_to_next` which wants to borrow
        // `self` mutable. But the iterator we created is already borrowing
        // part of `self` via `self.clist`. Thus, borrowck complains.
        //
        // Now, `self.add_state_to_next` doesn't actually need mutable access
        // to `clist`, so there is no actual conflict here. But borrowck can't
        // see past function boundaries. We could break down our `Matcher`
        // type into smaller components, but that's pretty heavy-handed here
        // and likely awkward. We could also use interior mutability (e.g.,
        // RefCell) in places to avoid needing to borrow `self` mutably. Or we
        // could just iterate over the indices of the list like we do below.
        // The other work-arounds may be appropriate in other circumstances!
        for i in 0..self.clist.n {
            let sid = self.clist.s[i];
            match self.nfa.states[sid as usize] {
                State::Literal { byte, out } if byte == haystack_byte => {
                    self.add_state_to_next(out);
                }
                _ => {}
            }
        }
    }

    // add given state handle to the nlist
    fn add_state_to_next(&mut self, sid: StateID) {
        if self.list_id == self.last_list_id[sid as usize] {
            return;
        }
        self.last_list_id[sid as usize] = self.list_id;
        if let State::Split { out1, out2 } = self.nfa.states[sid as usize] {
            // follow unlabeled arrows
            self.add_state_to_next(out1);
            self.add_state_to_next(out2);
            return;
        }
        self.nlist.s[self.nlist.n] = sid;
        self.nlist.n += 1;
    }

    // increment to a new list id
    fn increment_list_id(&mut self) {
        // The original implementation will overflow
        // int if enough searches are run and thus
        // cause UB. We could just panic on overflow
        // instead, but it's not hard to make this
        // correct. On overflow, we reset everything
        // back to the starting condition.
        self.list_id = match self.list_id.checked_add(1) {
            Some(list_id) => list_id,
            None => {
                for last_list_id in self.last_list_id.iter_mut() {
                    *last_list_id = 0;
                }
                1
            }
        };
    }
}

fn main() -> std::process::ExitCode {
    use std::process::ExitCode;

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
    let Some(nfa) = NFA::post2nfa(&post) else {
        eprintln!("error in post2nfa {pattern}");
        return ExitCode::FAILURE;
    };
    let mut matcher = Matcher::new(nfa);
    for arg in argv {
        let Ok(haystack) = arg.into_string() else {
            eprintln!("haystack is invalid UTF-8");
            return ExitCode::FAILURE;
        };
        if matcher.is_match(haystack.as_bytes()) {
            println!("{haystack}");
        }
    }
    ExitCode::SUCCESS
}

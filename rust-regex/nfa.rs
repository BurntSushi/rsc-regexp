// This program uses the regex crate as the regex implementation,
// mostly as a way to sanity check the tests. We do need to copy the
// parser from the other programs in order to reject the same set of
// invalid patterns. For example, the regex crate has higher limits on
// the size of patterns. And the regex crate also supports patterns
// like `a||b`.

#![forbid(unsafe_code)]

use regex::bytes::Regex;

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
    if re2post(pattern.as_bytes()).is_none() {
        eprintln!("bad regexp {pattern}");
        return ExitCode::FAILURE;
    }
    // The regex crate does unanchored searches by default.
    // Wrapping the pattern like this will technically make
    // some invalid patterns valid (e.g., `foo)(bar`), but
    // I didn't feel strongly enough to fix that. It could
    // be fixed by either building the syntax Hir manually
    // or using the lower level regex-automata anchored APIs.
    let anchored = format!(r"^(?:{pattern})$");
    let Ok(re) = Regex::new(&anchored) else {
        eprintln!("bad regexp {pattern}");
        return ExitCode::FAILURE;
    };
    for arg in argv {
        let Ok(haystack) = arg.into_string() else {
            eprintln!("haystack is invalid UTF-8");
            return ExitCode::FAILURE;
        };
        if re.is_match(haystack.as_bytes()) {
            println!("{haystack}");
        }
    }
    ExitCode::SUCCESS
}

// We copy the parser from our translation so that we
// can reject the same patterns as the other programs.
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

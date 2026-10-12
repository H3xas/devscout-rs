// The trailing `next:` line, the one part of a text answer that is about the
// NEXT question rather than this one.
//
// The line is meant to be pasted. That is the whole reason every argument
// outside a conservative safe set is single-quoted here rather than printed
// as-is: the arguments come from the graph, so a path or a symbol name is
// repository-controlled text, and a bare `$`, a backtick, a glob or a `#` in
// one would otherwise make the pasted line mean something different from what
// was suggested.

/// The `next:` trailer for a list of runnable argvs, or `None` when the list
/// is empty -- an answer with nothing left to suggest prints no line at all,
/// rather than a line naming no verb.
pub fn next_line(candidates: &[Vec<String>]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }
    let commands: Vec<String> = candidates
        .iter()
        .map(|argv| {
            let mut command = String::from("devscout");
            for arg in argv {
                command.push(' ');
                command.push_str(&quote_arg(arg));
            }
            command
        })
        .collect();
    Some(format!("next: {}", commands.join(" | ")))
}

// A shell-safe argument is printed bare; anything else is quoted. The set is
// the punctuation a POSIX shell passes through unchanged -- every other byte,
// and an empty argument, goes through `quote_arg`.
fn is_safe_arg_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'_' | b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'-'
        )
}

// POSIX single-quoting: everything inside `'...'` is literal, and an embedded
// single quote is written by closing the quote, escaping the quote and
// reopening it.
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && arg.bytes().all(is_safe_arg_byte) {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('\'');
    for ch in arg.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

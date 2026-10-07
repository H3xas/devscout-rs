use super::*;

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

#[test]
fn no_candidates_means_no_line_at_all() {
    assert_eq!(next_line(&[]), None);
}

#[test]
fn one_candidate_renders_the_verb_its_arguments_and_nothing_else() {
    assert_eq!(
        next_line(&[argv(&["tests", "Shop.IFoo"])]),
        Some("next: devscout tests Shop.IFoo".to_string())
    );
}

#[test]
fn several_candidates_are_joined_in_the_order_they_were_built() {
    assert_eq!(
        next_line(&[
            argv(&["tests", "Shop.IFoo"]),
            argv(&["impact", "src/IFoo.cs"])
        ]),
        Some("next: devscout tests Shop.IFoo | devscout impact src/IFoo.cs".to_string())
    );
}

// A shell-safe argument is quoted only when it needs to be: quoting is the
// exception, so the common line stays readable and greppable.
#[test]
fn an_argument_in_the_safe_set_is_printed_bare() {
    assert_eq!(
        next_line(&[argv(&["refs", "Acme.Storage.Bin.Stow"])]),
        Some("next: devscout refs Acme.Storage.Bin.Stow".to_string())
    );
}

// A Rust `::` path and a C# dotted id are ordinary text to a shell, so the
// quoting rule leaves them alone: it quotes what a shell would READ, not what
// merely looks unusual.
#[test]
fn a_path_separator_the_shell_ignores_is_not_quoted() {
    for arg in ["Widget::new", "Acme.Storage.Bin.Stow", "src/shelf.rs"] {
        assert_eq!(
            next_line(&[argv(&["read", arg])]),
            Some(format!("next: devscout read {arg}")),
            "{arg}"
        );
    }
}

#[test]
fn an_argument_the_shell_would_read_differently_is_single_quoted() {
    for (arg, quoted) in [
        ("Spaced Dir/IFoo.cs", "'Spaced Dir/IFoo.cs'"),
        ("src/shelf.rs#sweep", "'src/shelf.rs#sweep'"),
        ("a$b", "'a$b'"),
        ("`run`", "'`run`'"),
        ("*.cs", "'*.cs'"),
        ("hash#tag", "'hash#tag'"),
        ("", "''"),
    ] {
        assert_eq!(
            next_line(&[argv(&["read", arg])]),
            Some(format!("next: devscout read {quoted}")),
            "{arg}"
        );
    }
}

// A single quote inside the argument is closed, escaped and reopened, so the
// pasted line still yields exactly the argument that was suggested.
#[test]
fn an_embedded_single_quote_is_closed_and_reopened() {
    assert_eq!(
        next_line(&[argv(&["read", "O'Brien.cs"])]),
        Some("next: devscout read 'O'\\''Brien.cs'".to_string())
    );
}

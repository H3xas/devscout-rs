//! The process exit statuses.
//!
//! Every status the binary returns is one of these four values, so the
//! exit-code reference can name the outcome a command reported, and a caller
//! can branch on a named outcome instead of a bare integer.

/// The command completed and answered.
pub const EXIT_OK: i32 = 0;

/// The command ran and could not answer: an ambiguous seed, a refused query,
/// an environment it cannot read.
pub const EXIT_FAILURE: i32 = 1;

/// The invocation itself was malformed, and the verb printed its usage line
/// instead of doing any work.
pub const EXIT_USAGE: i32 = 2;

/// A query resolved but reached nothing. Neither success nor failure: the
/// index was read and the answer is empty, so a caller can branch on it
/// instead of rephrasing the query.
pub const EXIT_NO_RESULT: i32 = 3;

# AI Assistant Guidelines — uniblake-rs

## Git

- **Never stage, add, or commit.** Git is the user's action, always.
- Do not create branches or worktrees without being asked.
- `Cargo.lock`: pure library — `Cargo.lock` is correctly **not** tracked.

## Rust — required

**Error types**
- Box any third-party error stored in an enum variant: `Foo(#[from] Box<other::Error>)`.
  Unboxed variants set the size of every `Result` in the crate, success returns included.
  (`figment::Error` is 208 bytes; boxed, 8.)
- Assert the bound so a regression fails the build, not a lint:
  `const _: () = assert!(std::mem::size_of::<MyError>() <= 32);`

**Parameters**
- Take the least-owning form: `&str`, `&Path`, `&[T]`, or `impl AsRef<Path>`.
- Never `&String`, `&PathBuf`, `&Vec<T>` as a parameter type — the compiler accepts them
  (`Deref` makes them valid), but every caller holding only a borrow must allocate to call you.
- More than 7 parameters: pass a struct.

**Dead code**
- Do not add a field nothing reads. A gap belongs in a doc comment or an issue.
- Never write a loop or iterator chain whose only purpose is silencing a warning. Use
  `#[allow(...)]` **on the declaration**, with the reason in a comment. A runtime construct
  cannot suppress a compile-time lint honestly, and it reads as real work to the next reader.

**Attributes**
- `#[must_use]` on the type, not on every function returning it.

## Before returning Rust

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```

All three must pass. If clippy fails, **report the full inventory**, not the first error:

```sh
cargo clippy --workspace --all-targets --message-format=json \
  | jq -r 'select(.message.level=="warning") | .message.code.code' | sort | uniq -c | sort -rn
```

`-D warnings` aborts at the first failing crate, so a failure there is a lower bound, never a count.
Fixes also interact — correcting `&PathBuf` to `&Path` can create `unnecessary_to_owned` at a call
site. Re-run the inventory after each change.

## Edits

- Preserve existing content; do not delete details or references unless instructed.
- Explain "why" with "how". Offer tradeoffs. No time or schedule estimates unless asked.

# Contributing

```
cargo test                                  # has to pass
cargo clippy --all-targets -- -D warnings   # has to be silent
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps   # has to be silent
cargo test -- --ignored --test-threads=1    # only with a board attached
```

- **Protocol changes need evidence**: a place in the datasheet, or a measurement on a board,
  recorded in the README.
- **Hardware tests** are `#[ignore]`d and run one at a time; they switch every output.
- Code and commit messages in English. Commit messages carry no trailers: an imperative subject
  line, then a paragraph on why, wrapped at about 72 columns.

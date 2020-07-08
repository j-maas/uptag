# Release checklist

1. Compile for different targets using [cross](https://github.com/rust-embedded/cross): `cross build --target <target> --release`
   - x86_64-unknown-linux-gnu
   - x86_64-pc-windows-msvc
   - x86_64-apple-darwin
2. Strip Linux binary using `strip ./uptag`. (A [cargo setting is currently unstable](https://github.com/rust-lang/rust/issues/72110).)
3. Bump version in `Cargo.toml` and commit.
3. Tag commit with version tag, e.g. `v2.3.1`.
4. Add a [release](https://github.com/Y0hy0h/uptag/releases) on GitHub and upload binaries.
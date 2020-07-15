# Release checklist

1. Bump version in `Cargo.toml` and commit with message `Bump version to v2.3.1`.
2. Tag commit with version tag `v2.3.1`.
3. Compile for different targets using [cross](https://github.com/rust-embedded/cross): `cross build --target <target> --release`
   - x86_64-unknown-linux-gnu
   - x86_64-pc-windows-msvc

   (x86_64-apple-darwin is not available in cross on Windows)
4. Strip Linux binary using `strip ./uptag`. (A [cargo setting is currently unstable](https://github.com/rust-lang/rust/issues/72110).)
5. Add a [release](https://github.com/Y0hy0h/uptag/releases) on GitHub and upload binaries.
# uptag: Update tags in Dockerfiles.
[![GitHub Workflow Status](https://github.com/Y0hy0h/uptag/workflows/Build/badge.svg)](https://github.com/Y0hy0h/uptag/actions) [![Licensed under MIT or Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](#license)

Tired of manually looking up whether the base images you depend on have been updated?

```
$ uptag check ./Dockerfile
Report for Dockerfile at `/home/y0hy0h/Dockerfile`:

1 breaking update(s):
ubuntu:18.03
    -> 20.10

1 compatible update(s):
ubuntu:18.03
    -> 18.04
```

`/home/y0hy0h/Dockerfile`:
```Dockerfile
# uptag --pattern "0.<!>.<!>-r<>"
FROM ubuntu:18.03
```

## Pattern syntax
A pattern matches each character exactly. Use `<>` to match a number.  
- `<>.<>.<>` will match `2.13.3` but not `2.13.3a`.
- `debian-<>-beta` will match `debian-10-beta` but not `debian-10`.

Indicate which numbers indicate breaking changes using `<!>`. Uptag will report breaking changes separately from compatible changes.  
- Given the pattern `<!>.<>.<>`, if the current tag is `1.4.12`, then `1.6.12` and `1.4.13` are compatible updates, and `2.4.12` and `3.5.13` are breaking updates.

## License
Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
# uptag: Update tags in Dockerfiles.
[![CI status](https://github.com/Y0hy0h/uptag/workflows/CI/badge.svg)](https://github.com/Y0hy0h/uptag/actions?query=workflow%3ACI) [![Licensed under MIT or Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](#license)

Tired of manually looking up whether the base images you depend on have been updated?

```
$ uptag check ./Dockerfile
Report for Dockerfile at `/home/y0hy0h/Dockerfile`:

1 breaking update(s):
ubuntu:18.03
   -!> 20.10

1 compatible update(s):
ubuntu:18.03
    -> 18.04
```

`/home/y0hy0h/Dockerfile`:
```Dockerfile
# uptag --pattern "<!>.<>"
FROM ubuntu:18.03
```

Documentation is available with `uptag help`. Note that for example `uptag fetch -h` will give a summary, while `uptag fetch --help` prints the full documentation.

## Installation
Download the binaries from the [releases page](https://github.com/Y0hy0h/uptag/releases), available for Linux and Windows. Put them in a convenient location that is included in your [`PATH`](https://superuser.com/a/284351), so that `uptag` is available from everywhere.

Alternatively, you can build the binary for your system yourself. Install [`rustup`](https://rustup.rs/), clone this repository, and run `cargo build --release` in this folder. The binary will be available at `./target/release/uptag`.

## Pattern syntax
Use `<>` to match a number. Everything else will be matched literally.
- `<>.<>.<>` will match `2.13.3` but not `2.13.3a`.
- `debian-<>-beta` will match `debian-10-beta` but not `debian-10`.

Specify which numbers indicate breaking changes using `<!>`. Uptag will report breaking changes separately from compatible changes.
- Given pattern `<!>.<>.<>` and the current tag `1.4.12`
  - compatible updates: `1.6.12` and `1.4.13`
  - breaking updates: `2.4.12` and `3.5.13`

## Specifying patterns
### Dockerfiles
Each `FROM` definition needs to be annotated with a pattern and declare a specific tag that matches that pattern. The pattern must be given as a comment in the line before each `FROM <image>:<tag>` definition in the following format:
`# uptag --pattern "<pattern>"`

Example `Dockerfile`:
```
# uptag --pattern "<!>.<>"
FROM ubuntu:18.04

# uptag --pattern "<!>.<>.<>-slim"
FROM node:14.5.0-slim
```

### docker-compose.yml
Each service must associate a pattern with its images. There are two supported declarations.

A service can specify an `image` field, pointing to an image on DockerHub. Such an image needs to be annotated with a pattern and declare a specific tag that matches that pattern. The pattern must be given as a comment in the line before the `image` field in the following format:
`# uptag --pattern "<pattern>"`

Alternatively, a service can point to a folder containing a Dockerfile via its `build` field. That Dockerfile needs to specify patterns as [documented for Dockerfiles](#Dockerfiles).

Example `docker-compose.yml`:
```
version: "3.6"

services:
  ubuntu: 
    # uptag --pattern "<!>.<>"
    image: ubuntu:18.04

  alpine:
    build: ./alpine
```

## License
Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
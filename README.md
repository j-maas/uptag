# uptag: Update tags in Dockerfiles.
[![GitHub Workflow Status](https://github.com/Y0hy0h/uptag/workflows/Build/badge.svg)](https://github.com/Y0hy0h/uptag/actions) [![Licensed under MIT or Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](#license)

Tired of manually looking up whether the base images you depend on have been updated?

```
$ uptag check ./Dockerfile
Report for Dockerfile at `/home/y0hy0h/wordpress/Dockerfile`:

1 with compatible update:
bitnami/wordpress:5.3.2-r25
               -> 5.3.2-r26
```

# License
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
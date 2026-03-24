# Creating a Release

[GitHub](https://github.com/LilDojd/pagers/releases) and [FlakeHub](https://flakehub.com/flake/LilDojd/pagers) releases are automated via [GitHub Actions](./.github/workflows/release.yml) and triggered by pushing a tag.

1. Run the [release script](./release.sh): `./release.sh v[X.Y.Z]`
1. Push the changes: `jj git push -b main`
1. Check if [CI](https://github.com/LilDojd/pagers/actions) workflow completes successfully.
1. Push the tag: `git push origin v[X.Y.Z]`
1. Wait for [Release](https://github.com/LilDojd/pagers/actions) workflow to finish.

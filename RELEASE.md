# Creating a Release

Releases are automated by release-plz.

1. Merge normal changes into `main`.
1. Review and merge the release-plz release PR.
1. Wait for the Release-plz workflow to publish the crates and create the GitHub release.
1. Wait for the Release workflow to attach archives and publish to FlakeHub.

The repository requires these secrets:

- `RELEASE_APP_ID`: ID of a GitHub App with contents and pull-request write access.
- `RELEASE_APP_PRIVATE_KEY`: private key for that GitHub App.
- `CARGO_TOKEN`: crates.io publishing token.

Rerun Release-plz after a crates.io failure while `pagers` is not published at the workspace version.

If `pagers` is published but its tag or GitHub release is missing, do not rerun Release-plz. Find the exact source commit that release-plz published. For a merge commit, use the release PR's last commit, not the merge commit. For a squash merge, use the resulting commit on `main`. Then run:

```bash
VERSION=X.Y.Z
RELEASE_COMMIT=full-published-source-SHA
gh release create "v${VERSION}" --target "$RELEASE_COMMIT" --title "v${VERSION}" --generate-notes
```

This creates a missing tag at the release commit and publishes the GitHub release, which starts the Release workflow.

Rerun Release after an archive or FlakeHub failure. `gh release upload --clobber` deletes an old asset before it uploads the replacement, so rerun the workflow after a partial failure. Confirm that the release contains archives for all five targets: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `aarch64-unknown-linux-gnu`, `armv7-unknown-linux-gnueabihf`, and `aarch64-apple-darwin`.

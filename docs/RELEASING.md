# Releasing axross-amp

Back to the [README](../README.md) - everything in detail: the [extended README](README-extended.md).

A release is built by CI and signed by the maintainer: the signing key never
goes to CI, into a repository or into a package. The player's update
([Updates](README-extended.md#updates), src/update.rs) takes no release
without a valid signature of its `SHA256SUMS` by a key pinned in the
program (`TRUSTED_KEYS`).

## The key

- An Ed25519 key, PEM (PKCS#8), mode 0600, in a folder of its own outside
  every checkout (mode 0700). Back it up offline: without it no release
  can be signed for the players installed.
- Its public half is pinned in `src/update.rs` (`TRUSTED_KEYS`);
  `scripts/sign_release.py pubkey --key KEY` prints it in that form.
- Rotating: pin the new key beside the old one and release with the old
  one; from the release after that, sign with the new key; drop the old
  one once the releases still pinning only it are no longer in use.

## Steps

1. The version in `Cargo.toml`, a `## X.Y.Z - DATE` section in
   `CHANGELOG.md`; the tests (`cargo test`) and the supply-chain gate
   (`security/supply-chain-cargo.toml`) pass; the not-public check of the
   tree to be pushed passes.
2. Tag `vX.Y.Z` and push the tag: `.github/workflows/release.yml` runs the
   tests and the gate again, builds Linux, macOS and Windows, writes
   `SHA256SUMS` and the provenance attestation, and makes a **draft**
   release.
3. On the maintainer's machine (Python 3 with `cryptography`, the GitHub
   CLI logged in):

   ```sh
   scripts/sign_release.py release vX.Y.Z --key PATH/amp-update-signing.pem
   ```

   It downloads the draft's `SHA256SUMS`, checks every file of the draft
   against it, refuses a key that is not pinned, signs, checks the
   signature and uploads `SHA256SUMS.sig`.
4. Look at the draft (the files, the notes), then publish it:
   `scripts/sign_release.py release vX.Y.Z --key ... --publish` does steps 3
   and 4 in one, or `gh release edit vX.Y.Z --draft=false --latest`.

A release published without `SHA256SUMS.sig` is never offered by the
update; signing later (step 3 on the published release) is enough.

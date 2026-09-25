# The `kit` branch

`kit` is this fork's integration branch. It carries changes that upstream GPUI Kit does not have yet, such as the block editor and the shell work that Loom uses.

## Rebasing

`kit` is rebased onto upstream `main` when upstream moves. A rebase rewrites every commit on the branch, so a revision that a consumer pinned may no longer be on `kit` afterwards. Pinned revisions are not tagged.

After a rebase, every consumer moves to the new head of `kit`. There is no support for staying on an old revision.

## Consumers

Loom vendors this fork at `vendor/gpui-kit`, and its submodule revision is the single source of the kit revision and the `gpui-pre` version. Other consumers follow Loom:

1. Commit on `kit` (or rebase it) and push.
2. Move Loom's `vendor/gpui-kit` to the new head and commit.
3. Check epub-suite against it when reader crates are affected.
4. Move Ashlar's pin records and Limen's vendored Loom to the same revision.

No consumer pins `kit` on its own.

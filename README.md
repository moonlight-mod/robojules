# RoboJules

A PR differ for [moonlight extensions](https://github.com/moonlight-mod/extensions).

## Requirements

- Git and [difftastic](https://github.com/Wilfred/difftastic) must both be in your PATH environment variable.
- `GITHUB_TOKEN` environment variable set to a personal access token. RoboJules can load `.env` files.

## How it works

RoboJules only works with extensions that have already been submitted once. New extensions should be reviewed manually.

- Input the ID of the pull request (e.g. [#56](https://github.com/moonlight-mod/extensions/pull/56)).
- RoboJules downloads a few things:
  - The manifest from the `main` branch on the [extensions](https://github.com/moonlight-mod/extensions) repository.
  - The `.asar` of the built extension, from the `main` branch on the [extensions-dist](https://github.com/moonlight-mod/extensions-dist) repository.
  - The `.asar` of the built extension, from the pull request's CI artifacts.
  - The old commit of the built extension.
  - The new .commit of the built extension.
- RoboJules extracts the `.asar` files.
- RoboJules diffs the source repository and extracted `.asar` folders using difftastic.
- You, the user, read those diffs and verify it's safe.

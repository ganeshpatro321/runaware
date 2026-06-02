# Homebrew Tap

RunAware can be installed through Homebrew before it is accepted into homebrew-core by publishing a tap repo:

```text
github.com/ganeshpatro321/homebrew-tap
```

The user-facing command becomes:

```bash
brew install ganeshpatro321/tap/runaware
```

Plain `brew install runaware` only works after the formula is accepted into homebrew-core.

## Release Steps

1. Publish a RunAware GitHub release from a tag such as `v0.1.0`.
2. Copy `Formula/runaware.rb` into `ganeshpatro321/homebrew-tap/Formula/runaware.rb`.
3. Replace the `sha256` placeholders with values from the release `.sha256` assets.
4. Commit and push the tap repo.
5. Test:

```bash
brew install --build-from-source Formula/runaware.rb
brew test runaware
```

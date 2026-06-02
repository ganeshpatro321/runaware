# Open Source Checklist

Use this before making the repository public.

## Repository Files

- [x] README with install, usage, MCP setup, troubleshooting, and roadmap
- [x] License
- [x] Contributing guide
- [x] Security policy
- [x] Code of conduct
- [x] Changelog
- [x] Issue templates
- [x] Pull request template
- [x] CI workflow
- [x] Release workflow
- [x] Dependabot configuration
- [x] Install scripts
- [x] Homebrew tap formula template

## Maintainer Actions

- [ ] Review repository for secrets before publishing
- [ ] Decide whether issues and discussions should be enabled
- [ ] Add repository description and topics
- [ ] Enable branch protection for `main`
- [ ] Require CI before merge
- [ ] Create first public release tag
- [ ] Publish GitHub Release artifacts
- [ ] Publish Homebrew tap formula
- [ ] Consider publishing to crates.io
- [ ] Consider reserving npm package name for a future binary installer wrapper

## Suggested GitHub Topics

```text
ai
mcp
debugging
developer-tools
logs
observability
cli
rust
local-first
coding-agents
```

## Notes

Plain `brew install runaware` requires acceptance into Homebrew core. Until then, use a tap:

```bash
brew install ganeshpatro321/tap/runaware
```

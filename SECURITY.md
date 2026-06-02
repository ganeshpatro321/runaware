# Security Policy

RunAware captures local runtime output. Logs may contain secrets, credentials, tokens, internal URLs, or other sensitive data.

## Supported Versions

RunAware is pre-1.0. Security fixes will target the latest release.

## Reporting a Vulnerability

Please do not open a public GitHub issue for security vulnerabilities.

Report security issues by email:

```text
ganeshpatro321@gmail.com
```

Include:

- affected version or commit
- operating system
- reproduction steps
- expected impact
- whether logs, redaction, MCP output, or local storage are involved

## Security Expectations

RunAware currently:

- stores data locally by default
- redacts common secret patterns before storing logs
- avoids exposing raw environment dumps
- exposes active-run context through MCP

RunAware does not claim perfect secret detection. Treat redaction as a safety layer, not a complete data-loss-prevention system.

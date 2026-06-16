# Security Policy

## Supported Versions

We release patches for security vulnerabilities. Which versions are eligible for receiving such patches depends on the CVSS v3.0 Rating:

| CVSS v3.0 | Supported Versions |
|------------|--------------------|
| 9.0-10.0 (Critical) | Latest release |
| 4.0-8.9 (High/Medium) | Latest release |

## AIKD Privacy & Local-First Guarantee

**AIKD is 100% Local-First.**

- Your source code **NEVER** leaves your machine.
- AIKD does not contain any telemetry or analytics.
- Indexing and search happen entirely offline on your local hardware.
- The SQLite database and Tantivy index are stored locally in `~/.aikd/`.
- When used with Cloud AI Agents (like Cursor or Claude), AIKD only returns the specific text chunks requested by the agent via MCP protocol.

## Security Features

- **Path Traversal Protection**: Rejects `..` traversal and null bytes in all path inputs.
- **Allowed Roots Enforcement**: Scans are restricted to configured allowed directories.
- **Auth Token Support**: REST API supports optional Bearer token authentication.
- **No Arbitrary File Read**: File access is scoped to project directories only.

## Reporting a Vulnerability

If you discover a security vulnerability within AIKD, please report it responsibly:

1. **Do NOT** open a public GitHub issue.
2. Send an email to: **gelutjari@gmail.com**
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

All security vulnerabilities will be addressed within 72 hours of confirmation.

## Acknowledgments

We thank the security research community for helping keep AIKD safe for everyone.

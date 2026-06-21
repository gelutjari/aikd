# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 2.0.x   | ✅ Yes             |
| 1.x.x   | ❌ No              |

## Reporting a Vulnerability

**Please do NOT report security vulnerabilities through public GitHub issues.**

Instead, please report them via email to: **security@gelutjari.com**

### What to Include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

### Response Timeline

- **Initial response**: Within 48 hours
- **Triage**: Within 1 week
- **Fix development**: Within 30 days
- **Public disclosure**: After fix is released (90-day policy)

### Safe Harbor

We consider security research conducted in accordance with this policy as:

- Authorized and will not pursue legal action
- Exempt from DMCA and CFAA restrictions
- Conducted in good faith

## Security Measures

### Authentication

- Bearer token authentication for REST API
- JWT token support for session management
- Constant-time token comparison (prevents timing attacks)

### Input Validation

- Path traversal prevention (canonicalization, symlink checks)
- SQL injection prevention (parameterized queries)
- XSS prevention (output encoding)

### Network Security

- CORS whitelist configuration
- Rate limiting (10 requests/second per IP)
- Localhost-only binding by default

### Data Security

- SQLite WAL mode for data integrity
- Blake3 hashing for file integrity
- No external network calls (100% local)

## Responsible Disclosure

We kindly ask that you:

1. Give us reasonable time to fix the issue before public disclosure
2. Avoid accessing or modifying other users' data
3. Avoid actions that negatively impact other users

## Hall of Fame

We recognize security researchers who help improve AIKD:

| Researcher | Vulnerability | Date |
|------------|---------------|------|
| AI Security Audit | Auth bypass, CORS, Path traversal | 2025-06 |

## Contact

- **Security issues**: security@gelutjari.com
- **General questions**: GitHub Discussions
- **Bug reports**: GitHub Issues

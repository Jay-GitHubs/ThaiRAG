# Contributing to ThaiRAG

Thank you for your interest in contributing to ThaiRAG!

## Contributor License Agreement (CLA)

Before your contribution can be accepted, you must agree to our [Contributor License Agreement](CLA.md). By submitting a pull request, you acknowledge that you have read and agree to the CLA terms.

**Why a CLA?** ThaiRAG uses dual licensing (AGPL-3.0 + commercial). The CLA ensures the project maintainer can continue to offer commercial licenses alongside the open source version. This is standard practice for dual-licensed projects (e.g., MySQL, Qt, MongoDB).

## How to Contribute

### Reporting Bugs

1. Check existing issues to avoid duplicates
2. Open an issue with:
   - Clear description of the bug
   - Steps to reproduce
   - Expected vs actual behavior
   - ThaiRAG version and configuration tier

### Suggesting Features

1. Open an issue with the "feature request" label
2. Describe the use case and expected behavior

### Submitting Code

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-feature`
3. Make your changes
4. Run tests (the project has 326+ backend tests):
   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   cd admin-ui && npx tsc --noEmit
   ```
5. Commit with clear messages
6. Open a pull request against `main`

### Code Standards

- **Rust**: Follow `cargo fmt` and `cargo clippy` with no warnings
- **TypeScript**: Pass `npx tsc --noEmit` with no errors
- **Tests**: Add tests for new functionality
- **Security**: No hardcoded secrets, follow OWASP guidelines

## Development Setup

```bash
# Backend
THAIRAG_TIER=free cargo run -p thairag-api

# Admin UI
cd admin-ui && npm install && npm run dev

# Run all tests
cargo test
```

## Questions?

Contact: jdevspecialist@gmail.com

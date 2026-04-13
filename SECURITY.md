# Security Policy

## Reporting a Vulnerability

Please report security issues privately via GitHub Security Advisories (use the "Report a vulnerability" button on the repository's Security tab). This enables private vulnerability reporting and coordinated disclosure.

If GitHub Security Advisories are unavailable, email `security@ymir.invalid` (TODO: update before publishing).

Do not open public issues or pull requests for suspected vulnerabilities.

## Supported Versions

Only `main` is supported during pre-1.0 development. Security fixes will be released as patch versions (e.g. 0.1.1) against the latest minor.

## Response Window

- Acknowledgement within 7 days of receipt.
- Initial triage within 14 days.
- Fix or mitigation plan within 30 days where feasible. Complex issues may take longer; we will communicate status if so.

## Scope

Ymir is a simulation library with minimal attack surface: no network I/O in core crates, no authentication, and no user-facing web components. Expected report categories include:

- Dependency advisories affecting crates Ymir uses directly.
- Parsing bugs in override JSON or catalog data (malformed input causing unsafe behavior).
- Arithmetic or panic issues that could crash pipelines on adversarial input.

## Out of Scope

- Performance issues (slow runs, high memory usage) without a security impact.
- Non-reproducible numerical discrepancies between platforms.
- Issues in third-party crates with no fixable surface in Ymir itself (report those upstream).

## Disclosure Policy

We practice coordinated disclosure. Reporters are asked to honor a 90-day embargo ceiling while a fix is prepared. If a fix lands sooner, disclosure can happen sooner by mutual agreement. Credit will be given to reporters who wish to be acknowledged.

# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.2.x   | Yes       |
| 0.1.x   | Yes       |

## Reporting a vulnerability

If you discover a security vulnerability in ph-curves, please report it
**privately** — do not open a public issue.

Email **steve@giacomelli.ca** with:

- A description of the vulnerability.
- Steps to reproduce or a proof of concept.
- The affected version(s).

You should receive an acknowledgement within 48 hours. We will work with you to
understand and address the issue before any public disclosure.

## Scope

ph-curves is a `no_std` library primarily used in embedded firmware. Security
concerns most likely to apply include:

- Integer overflow or wraparound in math helpers.
- Unsound `unsafe` code (if any is introduced).
- Panics or undefined behaviour triggered by crafted input to the code-gen CLI.

## Disclosure

Once a fix is available, we will publish an advisory and a patched release.
Credit will be given to the reporter unless they prefer to remain anonymous.

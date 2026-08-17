# Security Policy

## Supported versions

Only the newest published minor line receives security fixes.

Support changes when a release is actually published to crates.io, not when a
release branch, changelog section, tag candidate, or pull request exists. This
makes the transition precise before and after a release:

| Version | Support rule |
|---------|--------------|
| 0.3.x   | Supported once 0.3.0 is published; pre-release branches are not supported releases. |
| 0.2.x   | Supported until 0.3.0 is published; unsupported afterward. |
| 0.1.x   | Unsupported. |

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

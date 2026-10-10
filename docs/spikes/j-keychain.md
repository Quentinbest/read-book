# Spike J: Keychain under ad-hoc signing (Reading Lens §6.5)

- **Date:** 2026-10-10. Mac14,3 (M2), macOS 14.6.1.
- **Question:** Linen's releases are ad-hoc signed (`.github/workflows/release.yml`), and macOS ties Keychain items to the code that wrote them. Across an update and a reinstall, does a stored key stay readable silently, prompt once, or fail? LK7's design was to be adjusted before Stage 2d if needed.

## Method

A throwaway program (outside the repository; `security-framework` 3) wrote a generic password and read it back. It was built twice from the same source with one constant changed, so the two builds had different code (CDHash `25f9c859…` and `35086d79…`), and each was ad-hoc signed (`codesign -s -`). Reads used `kSecUseAuthenticationUISkip`, so an item that would need the person to authorise is skipped instead of raising a dialog: “not found” would mean “would ask”.

| Read by | Stands for | Result |
|---|---|---|
| Build A, which wrote it | the same app | Read silently |
| A copy of build A at another path | a reinstall | Read silently |
| Build B (different CDHash and identifier) | an update | Read silently |
| An unrelated ad-hoc program (another crate) | any other local software | Read silently |

The item's access list named only build A (by path, and `partition_id: cdhash:25f9c859…`), yet every ad-hoc-signed reader got the value without a prompt.

## Finding

On this macOS, a login-keychain item written by an ad-hoc-signed program is readable without a prompt by other ad-hoc-signed programs of the same user. For Linen this means:

- **Updates and reinstalls keep the key, silently.** No “enter the key again” and no Keychain dialog after an update.
- **Keychain protects the key at the level of the user account,** encrypted at rest, not against other software the person runs. It does not isolate Linen's keys from other local programs. A Developer ID signature would change this; the build has none (item 28).
- **The protections LK7 relies on still hold:** extensions run in Linen's web view and can't call Keychain; the core adds a key to a request itself, only to the host it was saved for, never following redirects, and no command returns a key.

## Decision for LK7

Keep the macOS Keychain (`security-framework`), as planned. Do not claim protection beyond the account in the UI: the key sheet says the extension can't read the key, which is true, and nothing more. Revisit when releases have a Developer ID (item 28). `LK7-keychain-denied` still covers a locked or refused Keychain with a clear state.

## Limits

Run from command-line programs, not the app bundle; the bundle is ad-hoc signed the same way, so the same rule should apply. Not run on macOS 13 or 15.

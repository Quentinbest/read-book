# Sync Stage 0: the iCloud Drive and Google Drive folders on this Mac

- **Date:** 2026-10-17. Mac14,3 (M2), macOS 14.6.1 (23G93). Google Drive for desktop 115.0.1.0 (File Provider). One Google account.
- **Question:** how the two folders that sync writes into (`docs/sync-plan.md` SY2, SY7, SY30) behave for an app like Linen: where the roots are, whether macOS asks first, files that aren't downloaded, renames, upload time, names, the device-ID Keychain item, and the storage being unavailable. Issue #91.
- **Raw results:** `raw/sync-folders.json` (account and home-folder names removed).

## Method

Throwaway programs outside the repository, all ad-hoc signed (`codesign -s -`) as Linen's releases are:

- A command-line probe (`URLResourceValues` with the `ubiquitousItem…` keys, and `lstat` for the dataless flag `SF_DATALESS`).
- A small unsandboxed app bundle, `LinenSpike.app` (bundle ID `app.linen.reader.syncspike`), opened with `open`. macOS attributes a privacy prompt to the app that triggers it, and this is how Linen would meet one.
- Every file was written into a `Linen-spike` folder at the top of each storage, deleted afterwards. The owner agreed to the spike and was at the Mac for the prompts.

## Findings

### Roots (SY2)

| Storage | Root | Notes |
|---|---|---|
| iCloud Drive | `~/Library/Mobile Documents/com~apple~CloudDocs` | `ubiquityIdentityToken` is present when signed in. |
| Google Drive | `~/Library/CloudStorage/GoogleDrive-‹account›/My Drive` | The account root itself is read-only (`dr-x`); write into `My Drive`. |

`~/Library/CloudStorage` also holds **leftover folders from earlier setups**: `GoogleDrive-‹account› (2026-02-03 17:45)`, and two `iCloudDrive-iCloudDrive (‹date›)` folders. They look like live roots but aren't, so **root discovery must skip any name with a ` (…)` suffix**. Stream versus mirror mode wasn't read from Drive's settings; files could be evicted, so Linen must handle not-downloaded files in either mode.

### Privacy prompts

| Who | iCloud Drive | Google Drive |
|---|---|---|
| A background command-line process | `Operation not permitted`, no prompt | allowed |
| The app bundle, first run | **one prompt, about iCloud Drive** (owner; the exact wording wasn't recorded), then allowed | no prompt, allowed |
| The app rebuilt, with a new CDHash at the same path and bundle ID (as an update would be) | no prompt, allowed | allowed |

So Linen will meet **one macOS prompt for iCloud Drive**, the first time it reads the folder, and none for Google Drive. The answer is kept across an ad-hoc rebuild at the same path and bundle ID. A refusal shows up as `EPERM` ("Operation not permitted").

### Files that aren't downloaded (SY30)

| | Google Drive | iCloud Drive |
|---|---|---|
| `FileManager.evictUbiquitousItem` | works | works |
| How it looks afterwards | `SF_DATALESS` set, full size reported, downloading status `NotDownloaded` | `SF_DATALESS` set |
| Reading it while the app runs | blocks until downloaded: 5 MB in 7.4 s | blocks until downloaded: 2 MB in 2.9 s |
| Reading it with Google Drive for desktop quit | fails at once (0.1 s) with `ETIMEDOUT`, "Operation timed out" | not tested |
| Placeholder `.icloud` files | n/a | none: macOS 14 uses dataless files, not `.name.icloud` |

### Writing and uploading

- **Temporary name, then rename:** works in both. Replacing an existing file by rename (the plan's `index.json`) worked in Google Drive and uploaded the newer content.
- **Upload time**, from the rename until the file reports uploaded:
  - Google Drive, small files: 6.8 to 21.5 s, median 9.0 s (eleven files).
  - Google Drive, 5 MB: 11.2 s.
  - iCloud Drive, small file: 19.5 s (one sample).
- **Temporary files are tracked too.** `.linen-tmp-…` is a ubiquitous item while it exists, so a temporary file left behind by a crash could be uploaded.
- **With Google Drive for desktop quit:** listing and reading downloaded files work; a new file is written locally and marked not uploaded, and it uploaded 40.7 s after the app reopened.

### Names

- **Case:** both folders are case-insensitive (`case.txt` is `Case.txt`).
- **Unicode normalisation:** Google Drive is insensitive to it (an NFD name opens the NFC file).
- **Symbolic links:** Google Drive accepted one and later reported it uploaded. SY2's refusal of symbolic links is needed.

### The device-ID Keychain item (SY7)

| Keychain | Result for an ad-hoc-signed program |
|---|---|
| Data-protection keychain, *ThisDeviceOnly* | **refused: `-34018`, "A required entitlement isn't present"** |
| Login keychain, with *ThisDeviceOnly* requested | stored, but the accessibility class isn't kept (absent on read-back) |
| Login keychain | stored |
| The Mac's hardware UUID (`IOPlatformUUID`, IOKit) | readable, no entitlement needed |

The *ThisDeviceOnly* class that SY7 relies on isn't available to Linen's ad-hoc builds. It needs the keychain entitlement a Developer ID signature brings (#53). The login keychain ignores it, and Migration Assistant and backups carry the login keychain, so a migrated Mac would bring Linen's device ID along. Migration itself wasn't run.

## What this changes in the plan (for the owner)

1. **SY7, the device ID store (pending item 112).** Use the login keychain until Linen has a Developer ID. Everything kept with the ID would migrate with it, including SY7's writer token and high-water mark, so the ID also records the Mac's hardware UUID (`IOPlatformUUID`, readable by an ad-hoc program without any entitlement, measured). On a mismatch, as on a Mac set up by Migration Assistant, the device takes a new ID. A Time Machine restore of the same Mac rolls the keychain and the library back together. That case is caught by storage: when it holds changes under this ID *newer* than the device's own mark, the device takes a new ID. (Stale storage is always behind, never ahead, so this can't fire on it.) Move to the *ThisDeviceOnly* item once releases have a Developer ID.
2. **SY2, finding the roots.** Skip `CloudStorage` folders with a ` (…)` suffix, and write Google Drive's `Linen` folder inside `My Drive`. These are clarifications that follow from SY2, for Stage 2.
3. **SY6, the setup sheet.** For iCloud Drive, the sheet should say macOS will ask once, and the first read should come from the sheet's confirmation, so the prompt has context. A refusal (`EPERM`) gets its own status line: "Linen can't open iCloud Drive · Open System Settings". This is wording within item 99, to be captured in Stage 3.
4. **SY30, files not downloaded.** Before reading, check `SF_DATALESS` (or the downloading status) and call `startDownloadingUbiquitousItem`. Never read such a file on the main thread, and count the wait in SY2's 60 s pass budget. `ETIMEDOUT` from a dataless file means "waiting" (SY21), not damaged or missing.
5. **SY11, temporary files.** A `.linen-tmp-…` file can be uploaded if left behind, so each device removes its own stale temporary files (older than an hour) at the start of a pass.
6. **SY32, timing.** A change takes about 10–20 s to upload, so a frontier acknowledgement or hold (SY11) takes at least that long. That fits the plan's 10 s and 5-minute timings, and the in-app checks must allow for it.

## Not measured (still open in #91)

- **Other macOS releases** (13, 15 and 26): only 14.6.1 was available.
- **A second device:** whether a rename looks atomic and how long a file takes to arrive there, conflict-copy names (they need concurrent edits from two devices), and Migration Assistant or a Time Machine restore to another Mac.
- **iCloud Drive turned off or signed out, offline, and full storage.** Going offline would cut this session's network, and filling the storage wasn't attempted.
- **Two Google accounts:** this Mac has one.
- **Google Drive's stream or mirror mode:** not read from its settings (eviction worked, so Linen must handle not-downloaded files in either).
- **The exact wording of the iCloud Drive prompt.**
- **A different path:** the app run from a different path (a copy in `/Applications` versus a disk image) wasn't tested for the prompt.

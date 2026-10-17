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

`~/Library/CloudStorage` also holds **leftover folders from earlier setups**: `GoogleDrive-‹account› (2026-02-03 17:45)`, and two `iCloudDrive-iCloudDrive (‹date›)` folders. They look like live roots but aren't, so **root discovery must skip any name with a ` (…)` suffix**. Google Drive is in **Stream files** mode on this Mac (the owner read Drive's preferences); files could be evicted, so Linen must handle not-downloaded files in either mode.

### Privacy prompts

| Who | iCloud Drive | Google Drive |
|---|---|---|
| A background command-line process | `Operation not permitted`, no prompt | allowed |
| The app bundle, first run | **one prompt, about iCloud Drive** (owner; the exact wording wasn't recorded), then allowed | no prompt, allowed |
| The app rebuilt, with a new CDHash at the same path and bundle ID (as an update would be) | no prompt, allowed | allowed |
| The same app copied to `/Applications`, or run from a disk image made on this Mac | no prompt, allowed | not tested |
| After `tccutil reset` of the app | the prompt again (three times) | not tested |

So Linen will meet **one macOS prompt for iCloud Drive**, the first time it reads the folder, and none for Google Drive. The answer belongs to the app, not its path: it was kept across an ad-hoc rebuild, a copy in `/Applications` and a disk image. A *downloaded*, quarantined disk image (which macOS runs from a randomised path, App Translocation) wasn't tested. What the app receives after a reader *refuses* the prompt wasn't measured: the app was allowed, and `EPERM` was only seen by the command-line process that got no prompt.

### Files that aren't downloaded (SY30)

| | Google Drive | iCloud Drive |
|---|---|---|
| `FileManager.evictUbiquitousItem` | works | works |
| How it looks afterwards | `SF_DATALESS` set, full size reported, downloading status `NotDownloaded` | `SF_DATALESS` set |
| Reading it while the app runs | blocks until downloaded: 5 MB in 7.4 s | blocks until downloaded: 2 MB in 2.9 s |
| Reading it with Google Drive for desktop quit | fails at once (0.1 s) with `ETIMEDOUT`, "Operation timed out" | not tested |
| Reading it **offline** (the app running, the network cut) | **blocks**: it returned the data 84.5 s later, once the network was back | not conclusive (the read started about as the network came back) |
| Writing a new file offline | written locally, not uploaded; uploaded as soon as the network was back | written locally, not uploaded; uploaded 13 s after |
| Placeholder `.icloud` files | n/a | none: macOS 14 uses dataless files, not `.name.icloud` |
| Asking for the download (`startDownloadingUbiquitousItem`, 2 MB, evicted) | the call returns in 4 ms; the file reports downloaded after 4.0 s; reading it then takes 3 ms | the call returns in 3 ms; downloaded after 2.0 s; reading takes 3 ms |

### Writing and uploading

- **Temporary name, then rename:** works in both. Replacing an existing file by rename (the plan's `index.json`) worked in Google Drive and uploaded the newer content.
- **Upload time**, from the rename until the file reports uploaded:
  - Google Drive, small files: 6.8 to 21.5 s, median 9.0 s (eleven files).
  - Google Drive, 5 MB: 11.2 s.
  - iCloud Drive, small file: 19.5 s (one sample).
- **Temporary files are tracked too.** `.linen-tmp-…` is a ubiquitous item while it exists, so a temporary file left behind by a crash could be uploaded.
- **With Google Drive for desktop quit:** listing and reading downloaded files work; a new file is written locally and marked not uploaded, and it uploaded 40.7 s after the app reopened.

### Thousands of small files (SY11's blobs)

2,000 files of about 200 bytes each, written by temporary name and rename into a fresh folder, then deleted:

| | Google Drive | iCloud Drive |
|---|---|---|
| Writing them | 0.5 s | 0.7 s |
| Listing the folder | 0.00 s | 0.01 s |
| Half uploaded | 234 s | 143 s |
| All uploaded | 290 s (about 7 files a second) | 245 s (about 8 files a second) |
| Not uploaded after 30 min | 0 | 0 |
| Deleting the folder | 0.3 s | 0.2 s |

One file per body works. Locally it's instant; uploading goes at about 7–8 files a second, so a first sync of a library with 10,000 notes and highlights takes about 20–25 minutes to reach storage.

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

The *ThisDeviceOnly* class that SY7 relies on isn't available to Linen's ad-hoc builds. It needs a keychain-access-group entitlement. A Developer ID signature alone doesn't grant that: the entitlement must be authorised by an embedded provisioning profile (Apple TN3125), which the release workflow doesn't embed today (#53). The login keychain ignores it, and Migration Assistant and backups carry the login keychain, so a migrated Mac would bring Linen's device ID along. Migration itself wasn't run.

## What this changes in the plan (for the owner)

1. **SY7, the device ID store (pending item 112).** Use the login keychain until Linen has a Developer ID. Everything kept with the ID would migrate with it, including SY7's writer token and high-water mark, so the ID also records the Mac's hardware UUID (`IOPlatformUUID`, readable by an ad-hoc program without any entitlement, measured). On a mismatch, as on a Mac set up by Migration Assistant, the device takes a new ID. A Time Machine restore of the same Mac rolls the keychain and the library back together. That case is caught by storage: when it holds changes under this ID *newer* than the device's own mark, the device takes a new ID. (Stale storage is always behind, never ahead, so this can't fire on it.) Move to the *ThisDeviceOnly* item only once a built release is shown to carry the keychain entitlement, authorised by an embedded provisioning profile (checked with `codesign -d --entitlements`, then a probe like this one returning success), not merely once releases have a Developer ID.
2. **SY2, finding the roots.** Skip `CloudStorage` folders with a ` (…)` suffix, and write Google Drive's `Linen` folder inside `My Drive`. These are clarifications that follow from SY2, for Stage 2.
3. **SY6, the setup sheet.** For iCloud Drive, the sheet should say macOS will ask once, and the first read should come from the sheet's confirmation, so the prompt has context. A refusal gets its own status line: "Linen can't open iCloud Drive · Open System Settings". This is wording within item 99, to be captured in Stage 3. Which error a refusal produces is still to measure, so Stage 3 treats any permission error from the root that way.
4. **SY30, files not downloaded.** Before reading, check `SF_DATALESS` (or the downloading status) and call `startDownloadingUbiquitousItem`. Never read a dataless file directly: offline, a read blocks until the network returns. Ask for the download, then wait for the downloaded status, within SY2's 60 s pass budget; until then the file is "waiting" (SY21). `ETIMEDOUT` from a dataless file (the Drive app quit) means the same, not damaged or missing.
5. **SY11, temporary files.** A `.linen-tmp-…` file can be uploaded if left behind, so each device removes its own stale temporary files (older than an hour) at the start of a pass.
6. **SY11, blobs or packs.** One file per body works at 2,000 files in both storages, at about 7–8 uploads a second. Packs aren't needed for correctness. They would only shorten a large library's first upload, so the recommendation is to keep one file per body and revisit if Stage 4's 500-book check shows the first sync too slow.
7. **SY32, timing.** A change takes about 10–20 s to upload, so a frontier acknowledgement or hold (SY11) takes at least that long. That fits the plan's 10 s and 5-minute timings, and the in-app checks must allow for it.

## Not measured yet

The owner narrowed Stage 0 on 2026-10-17 (`docs/decisions.md`): there's no second device or account yet, and sync waits for a second platform anyway. The session with the owner at the Mac covered Drive's mode, another app path and offline. What it couldn't finish moved on too.

**Moved to Stage 6 (#97), real devices:**
- Other macOS releases (13, 15 and 26): only 14.6.1 was available.
- A second device on the same accounts: whether a rename looks atomic there, how long a file takes to arrive, conflict-copy names (they need concurrent edits from two devices), and Migration Assistant or a Time Machine restore to another Mac.
- iCloud Drive turned off and signed out, and full storage (the owner chose not to on this Mac).
- The exact wording of the iCloud Drive prompt, and what the app receives after a refusal (the prompt was answered "Allow" each time).
- Reading a not-downloaded iCloud Drive file offline, and an app run from a downloaded, quarantined disk image.
- Google Drive in Mirror files mode: this Mac streams.

**Moved to Stage 2 (#93), as a unit test:**
- Several Google accounts. Root discovery only reads folder names (`GoogleDrive-‹account›`, with leftovers to skip), so a test over fake folders covers it; no second account is needed.

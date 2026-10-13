# Linen

[English](README.md) · [简体中文](README.zh-Hans.md) · **繁體中文** · [日本語](README.ja.md)

> 本文譯自英文版 [README.md](README.md)；如有出入，以英文版為準。

Linen 是一款安靜的 macOS EPUB 閱讀器。它不打擾你讀書：內文填滿視窗，控制項只在你需要時出現，你讀的內容不會離開你的 Mac。Linen 以 Tauri 2（Rust 核心）、Svelte 5 和 foliate-js 打造，採用 MIT 授權開放原始碼。

![在 Linen 中閱讀《白鯨記》，閱讀設定已打開](docs/readme/reader.png)

- **照你的方式閱讀。** 翻頁或捲動，寬視窗下雙頁並排，支援由右至左與直排文字，以及可縮放的固定版面書籍。可調整文字大小、行距、字體（包括 OpenDyslexic）、頁面寬度，以及三種主題：紙張、棕褐、夜間。
- **隨時找到位置。** 目錄、在書中搜尋、依百分比、章節或紙本頁碼前往，註釋以浮動視窗顯示，每次跳轉後都能返回。
- **螢光標示與筆記**，共四種顏色，集中在「筆記」標籤頁，可輸出為 Markdown 或 W3C Web Annotation。
- **查詢**：在 Mac 內建辭典或你自己的 MDX 辭典中查字，不必離開目前頁面。
- **鍵盤優先。** 所有指令都在指令面板（⌘K）中，所有快速鍵都可以更改。
- **延伸功能** 可以加入指令、選取文字後的動作、導覽列標籤頁、主題和輸出格式；每個延伸功能都在自己的沙盒中執行，只擁有你允許的權限。
- **五種語言：** 英文、簡體中文、繁體中文、日文和西班牙文。VoiceOver 可以朗讀整個介面。

| 目錄                                                   | 螢光標示與筆記                                    |
| ------------------------------------------------------ | ------------------------------------------------- |
| ![內文旁的目錄](docs/readme/contents.png)              | ![筆記標籤頁](docs/readme/notes.png)              |
| **選取列**                                             | **查詢**                                          |
| ![螢光標示上方的選取列](docs/readme/selection-bar.png) | ![在 Mac 辭典中查詢單字](docs/readme/look-up.png) |

## 讀者指南

### 安裝

1. 需要 **macOS 13 或以上版本，並安裝 Safari 16.4 或以上版本。** Linen 使用 Safari 隨附的系統 WebKit 排版書籍。若 WebKit 版本太舊，Linen 會告訴你，並引導你前往軟體更新，而不會打開書籍。
2. 從[最新版本](https://github.com/Quentinbest/read-book/releases/latest)下載 DMG：Apple 晶片請選 `aarch64`，Intel 處理器的 Mac 請選 `x64`。打開 DMG，將 Linen 拖到「應用程式」檔案夾。
3. **第一次打開。** Linen 尚未以 Apple Developer ID 簽署，因此 macOS 會在打開前詢問。按住 Control 鍵並按一下 Linen，選擇「打開」，再按「打開」；或先打開一次，再到「系統設定 › 隱私權與安全性」選擇「強制打開」。

Linen 每天檢查一次更新，並自動安裝經過簽署的更新。各版本的變更請見 [`CHANGELOG.md`](CHANGELOG.md)（英文）。

### 讀第一本書

1. **加入書籍。** 將 EPUB 檔案拖到書庫，或選擇「打開…」（⌘O），或在 Finder 中按兩下 EPUB。Linen 會將每本書拷貝到自己的書庫。
2. **閱讀。** 按一下封面，或按「繼續閱讀」打開上次讀的書。用 → 和 ←、空白鍵、Page Down 或觸控式軌跡板翻頁；將指標移到視窗頂部或底部邊緣即可顯示工具列。⌘L 回到書庫。
3. **調整成你習慣的樣子。** 按一下 **Aa** 可設定文字大小、主題、行距、字體、頁面寬度，以及翻頁或捲動。⌘+ 和 ⌘− 在任何地方都能調整文字大小。
4. **找到位置。** ⌘T 打開目錄，⌘F 在書中搜尋，⌘J 前往某頁、某章或某個百分比，跳轉後按 ⌘[ 返回。
5. **螢光標示。** 選取文字後，選取列會提供四種顏色、筆記、拷貝、搜尋和查詢。選取文字時，按 H 加上螢光標示，按 N 加上筆記。⌘⇧A 打開「筆記」標籤頁，可在其中篩選和輸出。
6. **查字。** 選取一個字詞，按 ⌃⌘D。若要使用自己的辭典，請在「設定 › 辭典」中加入 MDX 檔案。
7. **其他所有功能** 都在指令面板（⌘K）中。按 ? 查看快速鍵一覽表，並可在「設定 › 快速鍵」中更改任何快速鍵。

**語言。** Linen 會依照 Mac 的偏好語言顯示。若要選擇其他語言，請前往「設定 › 一般 › 語言」；重新啟動 Linen 後生效。

### 你的書庫

- **位置。** 所有內容都在同一個檔案夾：`~/Library/Application Support/app.linen.reader`，包括資料庫（閱讀位置、螢光標示、筆記、設定）、書籍、封面，以及延伸功能和它們的資料。「設定 › 書庫 › 在 Finder 中顯示」可以打開它。
- **備份。** 備份這個檔案夾即可；Time Machine 已經會備份它。
- **回復。** 將檔案夾放回原處（在這部或另一部 Mac 上），再打開 Linen。
- **輸出。** 「設定 › 書庫 › 輸出所有螢光標示和筆記…」會為每本書寫出一個 W3C Web Annotation 檔案，其他 App 也能讀取。
- **解除安裝。** 將 Linen 移到垃圾桶後，這個檔案夾仍會保留。若要移除全部內容，請一併刪除它以及 `~/Library/Logs/app.linen.reader`。

### 隱私權

Linen 不會將任何資料傳送到任何地方。發生問題時，詳細資訊只會寫入這部 Mac 上的當機記錄（`~/Library/Logs/app.linen.reader/crash.log`）。「設定 › 關於」可以找到它，你可以自行決定是否附在電子郵件中。延伸功能只能連線到自己宣告的主機，而且必須先經過你允許。

![Linen 的夜間主題](docs/readme/night.png)

## 開發者指南

### 準備環境

需要一部安裝了 Xcode 命令列工具、[Rust](https://rustup.rs)（stable）、Node.js 24 和 pnpm 的 Mac（`corepack enable` 會使用 `package.json` 中指定的版本）。

```sh
git clone https://github.com/Quentinbest/read-book.git
cd read-book
pnpm install        # 同時套用 patches/foliate-js@0.0.0.patch
pnpm tauri dev      # 執行 App
```

### 程式碼結構

| 位置                       | 內容                                                                                          |
| -------------------------- | --------------------------------------------------------------------------------------------- |
| `src-tauri/src`            | Rust 核心：IPC 指令、SQLite 儲存、匯入與 EPUB 驗證、延伸功能登錄、AppKit 橋接。               |
| `src/reader/engine.ts`     | `ReaderEngine`，foliate-js 唯一的轉接層：翻頁、捲動、橫向捲動、固定版面。                     |
| `src/reader/Reader.svelte` | 閱讀器介面及其指令。                                                                          |
| `src/lib/commands/`        | 所有指令都在此宣告一次；選單列、⌘K 和快速鍵一覽表都由它產生。                                 |
| `src/lib/strings/`         | 所有介面文字，每種語言一個檔案。統一用語請見 `docs/i18n/glossary.md`。                        |
| `src/extensions/`          | 延伸功能主機。API 參考請見 [`docs/extensions/README.md`](docs/extensions/README.md)（英文）。 |
| `docs/`                    | 實作計畫、設計、決策與進度。程式碼註解會引用計畫中的規則編號（例如 `// B8: …`）。             |

### 第一次修改

1. 從 `main` 建立分支，完成修改，並在旁邊加上測試（Vitest 用 `src/**/*.test.ts`，Rust 用 `#[cfg(test)]` 模組）。
2. 執行 CI 會執行的檢查。任何警告都會讓 CI 失敗。

   ```sh
   scripts/ci-local.sh          # 格式、lint、型別、單元測試、設計 token、cargo fmt、clippy、cargo test
   scripts/ci-local.sh --full   # 另加 Playwright 測試
   pnpm vitest run src/reader/layout.test.ts    # 單一測試檔
   ```

3. 送出 pull request。CI 通過後才會合併。

### 撰寫延伸功能

延伸功能是一個名為 `*.linenext` 的 zip 檔，內含 `manifest.json`、一個 worker 指令碼，以及選用的頁面。可以從 [`examples/extensions`](examples/extensions) 中的範例（一個辭典和 Night Owl 主題）開始，閱讀 [`docs/extensions/README.md`](docs/extensions/README.md)，然後在「設定 › 延伸功能 › 從檔案安裝…」中安裝。

### 在真正的 App 中測試

macOS 上沒有適用於 Tauri 的 WebDriver，因此 App 會在測試版本中執行自己的端對端測試套件：

```sh
LINEN_SPIKES=1 pnpm tauri build --no-bundle --features spikes \
  --config src-tauri/tauri.spikes.conf.json           # 測試版本
scripts/e2e.sh                                        # App 內測試套件（約 30 分鐘）
scripts/e2e.sh r '^(N3-bodymatter|B8)'                # 只執行編號符合的檢查
scripts/e2e.sh v && node tests/visual/compare.mjs     # 擷取畫面並與核准的基準圖比較
python3 scripts/perf-coldstart.py; scripts/perf-memory.sh   # 效能預算（§6.4）
(cd src-tauri && cargo test --features spikes)        # Rust，包含僅供測試的程式碼
```

測試套件在目前的桌面執行，並在執行期間讓顯示器保持喚醒；`LINEN_SPACE=2 scripts/e2e.sh` 會將視窗移到桌面 2。在執行 `python3 scripts/corpus/fetch.py` 和 `python3 scripts/corpus/generate.py` 產生測試書籍之前，Rust 語料測試會自動略過。

### 建置與發行

```sh
scripts/release-macos.sh --unsigned # 開發用 DMG
scripts/release-macos.sh            # 簽署並公證（需要指令碼列出的憑證）
```

- **發行。** 更新 `src-tauri/tauri.conf.json` 和 `src-tauri/Cargo.toml` 中的 `version`，在 [`CHANGELOG.md`](CHANGELOG.md) 中加入該版本的摘要，然後推送對應的 `vX.Y.Z` 標籤。`.github/workflows/release.yml` 會建立 GitHub Release 草稿，附上該摘要、Apple 晶片與 Intel 的 DMG、經簽署的更新封存檔和 `latest.json`；發布草稿即完成發行。在取得 Apple Developer ID 之前，建置使用臨時（ad-hoc）簽署。
- **更新金鑰** 位於發行用 Mac 上的 `~/.tauri/linen-updater.key`（及 `.password`），以及儲存庫密鑰 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。請妥善備份：少了它，已安裝的副本將無法更新。絕不可提交到儲存庫。

## 授權

[MIT](LICENSE)。截圖中的書是赫爾曼・梅爾維爾的《白鯨記》，屬於公有領域。

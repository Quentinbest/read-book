# Linen

[English](README.md) · **简体中文** · [繁體中文](README.zh-Hant.md) · [日本語](README.ja.md)

> 本文译自英文版 [README.md](README.md)；如有出入，以英文版为准。

Linen 是一款安静的 macOS EPUB 阅读器。它不打扰你读书：正文铺满窗口，控件只在你需要时出现，你读的内容不会离开你的 Mac。Linen 使用 Tauri 2（Rust 核心）、Svelte 5 和 foliate-js 构建，以 MIT 许可证开源。

![在 Linen 中阅读《白鲸》，阅读设置已打开](docs/readme/reader.png)

- **按你的方式阅读。** 翻页或滚动，宽窗口下双页并排，支持从右到左和竖排文字，以及可缩放的固定版式书。可调文字大小、行距、字体（含 OpenDyslexic）、页面宽度，以及三种主题：纸张、棕褐、夜间。
- **随时找到位置。** 目录、在书中搜索、按百分比、章节或纸书页码前往，注释在浮窗中查看，每次跳转后都能返回。
- **高亮和笔记**，四种颜色，集中在“笔记”标签页中，可导出为 Markdown 或 W3C Web Annotation。
- **查询**：在 Mac 自带词典或你自己的 MDX 词典中查词，无需离开当前页。
- **键盘优先。** 所有命令都在命令面板（⌘K）中，所有快捷键都可以更改。
- **扩展** 可以添加命令、选中文字后的操作、导航栏标签页、主题和导出格式；每个扩展运行在自己的沙盒中，只拥有你允许的权限。
- **五种语言：** 英语、简体中文、繁体中文、日语和西班牙语。VoiceOver 可以朗读全部界面。

| 目录                                               | 高亮和笔记                                        |
| -------------------------------------------------- | ------------------------------------------------- |
| ![正文旁的目录](docs/readme/contents.png)          | ![笔记标签页](docs/readme/notes.png)              |
| **选择栏**                                         | **查询**                                          |
| ![高亮上方的选择栏](docs/readme/selection-bar.png) | ![在 Mac 词典中查询单词](docs/readme/look-up.png) |

## 读者指南

### 安装

1. 需要 **macOS 13 或更高版本，并安装 Safari 16.4 或更高版本。** Linen 使用随 Safari 提供的系统 WebKit 排版书籍。如果 WebKit 版本过旧，Linen 会给出提示并引导你前往软件更新，而不会打开书籍。
2. 从[最新版本](https://github.com/Quentinbest/read-book/releases/latest)下载 DMG：Apple 芯片选择 `aarch64`，Intel 芯片的 Mac 选择 `x64`。打开 DMG，把 Linen 拖到“应用程序”文件夹。
3. **首次打开。** Linen 尚未使用 Apple Developer ID 签名，因此 macOS 会在打开前询问。按住 Control 键点按 Linen，选择“打开”，再点“打开”；或者先打开一次，然后在“系统设置 › 隐私与安全性”中选择“仍要打开”。

Linen 每天检查一次更新，并自动安装经过签名的更新。各版本的改动见 [`CHANGELOG.md`](CHANGELOG.md)（英文）。

### 读第一本书

1. **添加书籍。** 把 EPUB 文件拖到书库中，或选择“打开…”（⌘O），或在访达中双击 EPUB。Linen 会把每本书复制到自己的书库。
2. **阅读。** 点按封面，或点“继续阅读”打开上次读的书。用 → 和 ←、空格键、Page Down 或触控板翻页；把指针移到窗口顶部或底部边缘即可显示工具栏。⌘L 返回书库。
3. **调整到顺手。** 点按 **Aa** 可设置文字大小、主题、行距、字体、页面宽度，以及翻页或滚动。⌘+ 和 ⌘− 在任何地方都能调整文字大小。
4. **找到位置。** ⌘T 打开目录，⌘F 在书中搜索，⌘J 前往某页、某章或某个百分比，跳转后按 ⌘[ 返回。
5. **高亮。** 选中文字后，选择栏会提供四种颜色、笔记、拷贝、搜索和查询。选中文字时，按 H 高亮，按 N 添加笔记。⌘⇧A 打开“笔记”标签页，可在其中筛选和导出。
6. **查词。** 选中一个词，按 ⌃⌘D。要使用自己的词典，请在“设置 › 词典”中添加 MDX 文件。
7. **其他一切** 都在命令面板（⌘K）中。按 ? 查看快捷键速查表，可在“设置 › 快捷键”中更改任何快捷键。

**语言。** Linen 跟随 Mac 的首选语言。要选择其他语言，请前往“设置 › 通用 › 语言”；重新启动 Linen 后生效。

### 你的书库

- **位置。** 所有内容都在一个文件夹中：`~/Library/Application Support/app.linen.reader`，包括数据库（阅读位置、高亮、笔记、设置）、书籍、封面，以及扩展和它们的数据。“设置 › 书库 › 在访达中显示”可以打开它。
- **备份。** 备份这个文件夹即可；Time Machine 已经会备份它。
- **恢复。** 把文件夹放回原处（在这台或另一台 Mac 上），然后打开 Linen。
- **导出。** “设置 › 书库 › 导出所有高亮和笔记…”会为每本书写出一个 W3C Web Annotation 文件，其他应用也能读取。
- **卸载。** 把 Linen 移到废纸篓后，这个文件夹仍会保留。若要删除全部内容，请一并删除它以及 `~/Library/Logs/app.linen.reader`。

### 隐私

Linen 不会向任何地方发送任何数据。出现问题时，详情只写入这台 Mac 上的崩溃日志（`~/Library/Logs/app.linen.reader/crash.log`）。“设置 › 关于”可以找到它，你可以自行决定是否把它附在邮件中。扩展只能访问自己声明的主机，而且必须先经你允许。

![Linen 的夜间主题](docs/readme/night.png)

## 开发者指南

### 准备环境

需要一台装有 Xcode 命令行工具、[Rust](https://rustup.rs)（stable）、Node.js 24 和 pnpm 的 Mac（`corepack enable` 会使用 `package.json` 中指定的版本）。

```sh
git clone https://github.com/Quentinbest/read-book.git
cd read-book
pnpm install        # 同时应用 patches/foliate-js@0.0.0.patch
pnpm tauri dev      # 运行应用
```

### 代码结构

| 位置                       | 内容                                                                                    |
| -------------------------- | --------------------------------------------------------------------------------------- |
| `src-tauri/src`            | Rust 核心：IPC 命令、SQLite 存储、导入与 EPUB 校验、扩展注册表、AppKit 桥接。           |
| `src/reader/engine.ts`     | `ReaderEngine`，foliate-js 的唯一适配层：翻页、滚动、横向滚动、固定版式。               |
| `src/reader/Reader.svelte` | 阅读器界面及其命令。                                                                    |
| `src/lib/commands/`        | 所有命令都在此处声明一次；菜单栏、⌘K 和速查表都由它生成。                               |
| `src/lib/strings/`         | 所有界面文字，每种语言一个文件。统一术语见 `docs/i18n/glossary.md`。                    |
| `src/extensions/`          | 扩展宿主。API 参考见 [`docs/extensions/README.md`](docs/extensions/README.md)（英文）。 |
| `docs/`                    | 实施计划、设计、决策和进度。代码注释会引用计划中的规则编号（如 `// B8: …`）。           |

### 第一次修改

1. 从 `main` 创建分支，完成修改，并在旁边添加测试（Vitest 用 `src/**/*.test.ts`，Rust 用 `#[cfg(test)]` 模块）。
2. 运行 CI 会运行的检查。任何警告都会让 CI 失败。

   ```sh
   scripts/ci-local.sh          # 格式、lint、类型、单元测试、设计令牌、cargo fmt、clippy、cargo test
   scripts/ci-local.sh --full   # 另加 Playwright 测试
   pnpm vitest run src/reader/layout.test.ts    # 单个测试文件
   ```

3. 提交拉取请求。CI 通过后才会合并。

### 编写扩展

扩展是一个名为 `*.linenext` 的 zip 包，包含 `manifest.json`、一个 worker 脚本和可选的页面。可以从 [`examples/extensions`](examples/extensions) 中的示例（一个词典和 Night Owl 主题）开始，阅读 [`docs/extensions/README.md`](docs/extensions/README.md)，然后在“设置 › 扩展 › 从文件安装…”中安装。

### 在真实应用中测试

macOS 上没有适用于 Tauri 的 WebDriver，因此应用会在测试构建中运行自己的端到端测试套件：

```sh
LINEN_SPIKES=1 pnpm tauri build --no-bundle --features spikes \
  --config src-tauri/tauri.spikes.conf.json           # 测试构建
scripts/e2e.sh                                        # 应用内测试套件（约 30 分钟）
scripts/e2e.sh r '^(N3-bodymatter|B8)'                # 只运行编号匹配的检查
scripts/e2e.sh v && node tests/visual/compare.mjs     # 截图并与已批准的基准图比较
python3 scripts/perf-coldstart.py; scripts/perf-memory.sh   # 性能预算（§6.4）
(cd src-tauri && cargo test --features spikes)        # Rust，包含仅用于测试的代码
```

测试套件在当前桌面运行，并在运行期间保持显示器唤醒；`LINEN_SPACE=2 scripts/e2e.sh` 会把窗口移到桌面 2。在运行 `python3 scripts/corpus/fetch.py` 和 `python3 scripts/corpus/generate.py` 生成测试书籍之前，Rust 语料测试会自动跳过。

### 构建与发布

```sh
scripts/release-macos.sh --unsigned # 开发用 DMG
scripts/release-macos.sh            # 签名并公证（需要脚本列出的凭据）
```

- **发布。** 更新 `src-tauri/tauri.conf.json` 和 `src-tauri/Cargo.toml` 中的 `version`，在 [`CHANGELOG.md`](CHANGELOG.md) 中添加该版本的摘要，然后推送对应的 `vX.Y.Z` 标签。`.github/workflows/release.yml` 会创建一个 GitHub Release 草稿，附上该摘要、Apple 芯片和 Intel 的 DMG、签名的更新包和 `latest.json`；发布草稿即完成发布。在取得 Apple Developer ID 之前，构建使用临时（ad-hoc）签名。
- **更新密钥** 位于发布用 Mac 上的 `~/.tauri/linen-updater.key`（及 `.password`），以及仓库密钥 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。请妥善备份：没有它，已安装的副本将无法更新。切勿提交到仓库。

## 许可证

[MIT](LICENSE)。截图中的书是赫尔曼·梅尔维尔的《白鲸》，属于公有领域。

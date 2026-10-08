# Glossary

Terms that must read the same everywhere in a language. Every row was confirmed by the native reviewers on 2026-10-08 (`docs/decisions.md`). **Status** says where a row came from:

- **macOS**: the name macOS itself uses for the standard menus and items, in its own localisation. Keep it, so Linen matches every other Mac app. These rows were written from knowledge of macOS, not read from a Mac set to each language: the reviewer checks them against a standard app (TextEdit, Finder) in that language. For Spanish, macOS has `es` and `es-419`; Linen's single `es` (L-3) follows `es` where they differ.
- **proposed**: a first proposal, written while building the translation kit (2026-10-07), not by a native speaker. The reviewer of each language confirms or replaces it (L-6), and the row then says **reviewed**.

Linen's own names (Pages, Scroll, Navigator, Paper…) describe what the reader sees; translate the meaning, not the word. Names that are never translated: Linen, EPUB, Finder, Safari, Time Machine, VoiceOver, ⌘ and the other key symbols.

## macOS menus and standard items

| English | zh-Hans | zh-Hant | ja | es | Status |
|---|---|---|---|---|---|
| File | 文件 | 檔案 | ファイル | Archivo | macOS, reviewed |
| Edit | 编辑 | 編輯 | 編集 | Edición | macOS, reviewed |
| View | 显示 | 顯示方式 | 表示 | Visualización | macOS, reviewed |
| Go | 前往 | 前往 | 移動 | Ir | macOS, reviewed |
| Window | 窗口 | 視窗 | ウインドウ | Ventana | macOS, reviewed |
| About Linen | 关于 Linen | 關於 Linen | Linen について | Acerca de Linen | macOS, reviewed |
| Settings… | 设置… | 設定… | 設定… | Ajustes… | macOS, reviewed |
| Services | 服务 | 服務 | サービス | Servicios | macOS, reviewed |
| Hide Linen | 隐藏 Linen | 隱藏 Linen | Linen を非表示 | Ocultar Linen | macOS, reviewed |
| Hide Others | 隐藏其他 | 隱藏其他 | ほかを非表示 | Ocultar otros | macOS, reviewed |
| Show All | 全部显示 | 顯示全部 | すべてを表示 | Mostrar todo | macOS, reviewed |
| Quit Linen | 退出 Linen | 結束 Linen | Linen を終了 | Salir de Linen | macOS, reviewed |
| Undo | 撤销 | 還原 | 取り消す | Deshacer | macOS, reviewed |
| Cut / Copy / Paste | 剪切 / 拷贝 / 粘贴 | 剪下 / 拷貝 / 貼上 | カット / コピー / ペースト | Cortar / Copiar / Pegar | macOS, reviewed |
| Select All | 全选 | 全選 | すべてを選択 | Seleccionar todo | macOS, reviewed |
| Minimize | 最小化 | 縮到最小 | しまう | Minimizar | macOS, reviewed |
| Full screen | 全屏幕 | 全螢幕 | フルスクリーン | Pantalla completa | macOS, reviewed |
| Show in Finder | 在访达中显示 | 在 Finder 中顯示 | Finder に表示 | Mostrar en el Finder | macOS, reviewed (zh-Hans names Finder 访达) |
| Keyboard shortcuts | 键盘快捷键 | 鍵盤快速鍵 | キーボードショートカット | Funciones rápidas de teclado | macOS, reviewed |
| Look Up | 查询 | 查詢 | 調べる | Consultar | macOS, reviewed |
| Restart | 重新启动 | 重新啟動 | 再起動 | Reiniciar | macOS, reviewed |

## Linen

| English | Meaning | zh-Hans | zh-Hant | ja | es | Status |
|---|---|---|---|---|---|---|
| Library | All the reader's books | 书库 | 書庫 | ライブラリ | Biblioteca | reviewed |
| Book | An EPUB in the library | 书 | 書 | ブック | Libro | reviewed |
| Continue reading / Resume reading | The book last open | 继续阅读 | 繼續閱讀 | 続きを読む | Seguir leyendo | reviewed |
| Contents | The book's table of contents | 目录 | 目錄 | 目次 | Índice | reviewed |
| Chapter | A section of the book | 章节 | 章節 | 章 | Capítulo | reviewed |
| Navigator | The side panel with Contents, Search and Notes | 导航栏 | 導覽列 | ナビゲータ | Navegador | reviewed; the reviewer may prefer a plainer word for a side panel |
| Highlight | Marked text (noun) / to mark text (verb) | 高亮 | 螢光標示 | ハイライト | Resaltado / resaltar | reviewed |
| Note | Text the reader adds to a highlight | 笔记 | 筆記 | メモ | Nota | reviewed |
| Highlights and notes | The Notes tab and command | 高亮和笔记 | 螢光標示與筆記 | ハイライトとメモ | Resaltados y notas | reviewed |
| Footnote / note (in the book) | The book's own notes, shown in a peek | 注释 | 註釋 | 注 | Nota al pie | reviewed; keep apart from the reader's Note |
| Search in book | Searching the open book | 在书中搜索 | 在書中搜尋 | ブック内を検索 | Buscar en el libro | reviewed |
| Go to… | Jump to a percent, chapter or print page | 前往… | 前往… | 移動… | Ir a… | reviewed |
| Print page | The printed edition's page number | 纸书页码 | 紙本頁碼 | 紙の本のページ | Página impresa | reviewed |
| Pages / Scroll | The two layouts | 翻页 / 滚动 | 翻頁 / 捲動 | ページ / スクロール | Páginas / Desplazamiento | reviewed |
| Reading settings | The Aa popover | 阅读设置 | 閱讀設定 | 読書設定 | Ajustes de lectura | reviewed |
| Text size | | 文字大小 | 文字大小 | 文字サイズ | Tamaño del texto | reviewed |
| Line spacing | | 行距 | 行距 | 行間 | Interlineado | reviewed |
| Theme: Paper, Sepia, Night, Auto | Page colours | 主题：纸张、棕褐、夜间、自动 | 主題：紙張、棕褐、夜間、自動 | テーマ：ペーパー、セピア、ナイト、自動 | Tema: Papel, Sepia, Noche, Automático | reviewed |
| Command palette | ⌘K | 命令面板 | 指令面板 | コマンドパレット | Paleta de comandos | reviewed |
| Single-key shortcuts | Keys such as [ ] H N without ⌘ | 单键快捷键 | 單鍵快速鍵 | 単一キーのショートカット | Funciones rápidas de una tecla | reviewed |
| Caret browsing | Moving a text cursor through the book (F7) | 光标浏览 | 游標瀏覽 | キャレットブラウズ | Navegación con cursor | reviewed |
| Extension | An add-on (a `.linenext` package) | 扩展 | 延伸功能 | 機能拡張 | Extensión | reviewed |
| Permission | What an extension may access | 权限 | 權限 | アクセス権 | Permiso | reviewed |
| Damaged chapter | A chapter that could not be opened | 损坏的章节 | 損毀的章節 | 破損した章 | Capítulo dañado | reviewed |
| Fixed pages / Reflowable | Book layouts | 固定版式 / 可重排 | 固定版面 / 可重排 | 固定レイアウト / リフロー | Páginas fijas / Adaptable | reviewed |
| Publisher styles | The book's own fonts and colours | 出版社样式 | 出版社樣式 | 出版社のスタイル | Estilos de la editorial | reviewed |
| Crash log | Linen's local error log (D1) | 崩溃日志 | 當機記錄 | クラッシュログ | Registro de fallos | reviewed |

-- Fixture for schema version 4 (plan §6.3): a database as version 3 wrote it,
-- with rows in every table. Never edit; add schema-vN.sql for each new version.
PRAGMA user_version = 4;
CREATE TABLE books (id TEXT PRIMARY KEY, content_hash TEXT NOT NULL, package_identifier TEXT, file_path TEXT NOT NULL, title TEXT NOT NULL, title_source TEXT NOT NULL, authors TEXT NOT NULL DEFAULT '[]', language TEXT, page_direction TEXT NOT NULL DEFAULT 'default', writing_mode TEXT, layout TEXT NOT NULL DEFAULT 'reflowable', has_page_list INTEGER NOT NULL DEFAULT 0, a11y_metadata TEXT NOT NULL DEFAULT '[]', cover_path TEXT, generated_cover_tint TEXT, added_at INTEGER NOT NULL, opened_at INTEGER, finished_at INTEGER, replaced_at INTEGER, removed_at INTEGER);
CREATE UNIQUE INDEX books_content_hash ON books(content_hash);
CREATE INDEX books_package_identifier ON books(package_identifier);
CREATE TABLE book_damage (book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE, item_href TEXT NOT NULL, error_kind TEXT NOT NULL, PRIMARY KEY (book_id, item_href));
CREATE TABLE positions (book_id TEXT PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE, cfi TEXT NOT NULL, fraction REAL NOT NULL, updated_at INTEGER NOT NULL, chapter_label TEXT);
CREATE TABLE book_settings (book_id TEXT PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE, layout_mode TEXT NOT NULL DEFAULT 'pages', navigator_docked TEXT);
CREATE TABLE annotations (id TEXT PRIMARY KEY, book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE, anchored_content_hash TEXT NOT NULL, color TEXT NOT NULL, cfi_range TEXT NOT NULL, quote_exact TEXT NOT NULL, quote_prefix TEXT NOT NULL DEFAULT '', quote_suffix TEXT NOT NULL DEFAULT '', note TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, anchor_status TEXT NOT NULL DEFAULT 'anchored');
CREATE INDEX annotations_book ON annotations(book_id, deleted_at);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE extensions (id TEXT PRIMARY KEY, version TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, granted_permissions TEXT NOT NULL DEFAULT '[]', installed_at INTEGER NOT NULL, crash_log TEXT NOT NULL DEFAULT '[]');
CREATE TABLE extension_storage (ext_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE, key TEXT NOT NULL, value BLOB NOT NULL, PRIMARY KEY (ext_id, key));

CREATE TABLE search_text (
    book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    section INTEGER NOT NULL,
    text TEXT NOT NULL,
    PRIMARY KEY (book_id, section)
);

CREATE TABLE dictionaries (id TEXT PRIMARY KEY, name TEXT NOT NULL, title TEXT NOT NULL, source_hash TEXT NOT NULL, generation TEXT NOT NULL, position INTEGER NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, entries INTEGER NOT NULL, resources INTEGER NOT NULL DEFAULT 0, added_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);

INSERT INTO books VALUES ('b1', 'hash-1', 'urn:isbn:1', '/lib/b1.epub', 'Moby-Dick', 'package', '["Herman Melville"]', 'en', 'ltr', NULL, 'reflowable', 1, '[["accessMode","textual"]]', '/lib/b1.jpg', NULL, 1000, 2000, NULL, NULL, NULL);
INSERT INTO books VALUES ('b2', 'hash-2', NULL, '/lib/b2.epub', 'untitled file', 'filename', '[]', NULL, 'rtl', 'vertical-rl', 'fixed', 0, '[]', NULL, '#E8DCC8', 1500, NULL, 3000, 2500, NULL);
INSERT INTO book_damage VALUES ('b2', 'OEBPS/c2.xhtml', 'missing');
INSERT INTO positions VALUES ('b1', 'epubcfi(/6/14!/4/2/1:0)', 0.31, 2000, 'Chapter 1 · Loomings');
INSERT INTO book_settings VALUES ('b1', 'scroll', 'contents');
INSERT INTO annotations VALUES ('a1', 'b1', 'hash-1', 'yellow', 'epubcfi(/6/4!/4/2,/1:0,/1:36)', 'a damp, drizzly November in my soul', 'whenever it is ', '; whenever I', 'Ishmael’s gloom', 1100, 1200, NULL, 'anchored');
INSERT INTO annotations VALUES ('a2', 'b1', 'hash-0', 'rose', 'epubcfi(/6/8!/4/2,/1:0,/1:5)', 'Whale', '', '', NULL, 1300, 1300, 1400, 'reanchor');
INSERT INTO settings VALUES ('theme', 'sepia');
INSERT INTO settings VALUES ('singleKeyShortcuts', 'off');
INSERT INTO extensions VALUES ('markdown-export', '1.0.0', 1, '["annotations.read"]', 900, '[]');
INSERT INTO extension_storage VALUES ('markdown-export', 'lastPath', X'2F746D70');
INSERT INTO search_text VALUES ('b1', 'hash-1', 0, 'Call me Ishmael.');
INSERT INTO search_text VALUES ('b1', 'hash-1', 1, 'Some years ago…');
INSERT INTO dictionaries VALUES ('d1', 'oald', 'Learner''s Dictionary', 'hash-d1', '0123456789abcdef0123456789abcdef', 0, 1, 1200, 3, 4000, 4000);

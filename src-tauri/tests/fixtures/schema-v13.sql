-- Fixture for schema version 13 (plan §6.3): a database as version 3 wrote it,
-- with rows in every table. Never edit; add schema-vN.sql for each new version.
PRAGMA user_version = 13;
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

CREATE TABLE extension_secrets (ext_id TEXT NOT NULL, name TEXT NOT NULL, host TEXT NOT NULL, label TEXT NOT NULL DEFAULT '', saved_at INTEGER NOT NULL, PRIMARY KEY (ext_id, name));

CREATE TABLE sync_libraries (library TEXT PRIMARY KEY, storage TEXT NOT NULL, device TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'on', active INTEGER NOT NULL DEFAULT 0, clock_ms INTEGER NOT NULL DEFAULT 0, clock_n INTEGER NOT NULL DEFAULT 0, joined_at INTEGER NOT NULL, restored_from INTEGER);
CREATE UNIQUE INDEX sync_libraries_active ON sync_libraries(active) WHERE active = 1;
CREATE TABLE sync_versions (library TEXT NOT NULL REFERENCES sync_libraries(library), key TEXT NOT NULL, device TEXT NOT NULL, seq INTEGER, hash TEXT NOT NULL, raw TEXT NOT NULL, applied INTEGER NOT NULL DEFAULT 0);
CREATE UNIQUE INDEX sync_versions_published ON sync_versions(library, device, seq, key) WHERE seq IS NOT NULL;
CREATE UNIQUE INDEX sync_versions_unsent ON sync_versions(library, key) WHERE seq IS NULL;
CREATE INDEX sync_versions_key ON sync_versions(library, key);
CREATE INDEX sync_versions_body ON sync_versions(json_extract(raw, '$.body.hash'));
CREATE TABLE sync_stopped (library TEXT NOT NULL, device TEXT NOT NULL, reason TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY (library, device));
CREATE TABLE sync_undo (library TEXT NOT NULL, key TEXT NOT NULL, device TEXT NOT NULL, seq INTEGER NOT NULL, hash TEXT NOT NULL, before BLOB, at INTEGER NOT NULL, turn INTEGER NOT NULL DEFAULT 0);
CREATE INDEX sync_undo_at ON sync_undo(library, at);
CREATE TABLE sync_counted (library TEXT NOT NULL, at INTEGER NOT NULL, what TEXT NOT NULL, kind TEXT NOT NULL, book TEXT, bytes INTEGER NOT NULL, approved INTEGER NOT NULL);
CREATE INDEX sync_counted_at ON sync_counted(library, at);
CREATE TABLE sync_question (library TEXT PRIMARY KEY, asking INTEGER NOT NULL, approved INTEGER NOT NULL, asked_at INTEGER, device TEXT, seq INTEGER, hash TEXT, counts TEXT, answered_at INTEGER, totals_asked_at INTEGER, versions TEXT);
CREATE TABLE sync_deleted (library TEXT NOT NULL, key TEXT NOT NULL, device TEXT NOT NULL, hash TEXT NOT NULL, seq INTEGER, first_seen INTEGER NOT NULL, displaced BLOB, displaced_raw TEXT, highlight TEXT, PRIMARY KEY (library, key, device, hash));
CREATE TABLE sync_bodies (hash TEXT PRIMARY KEY, body BLOB NOT NULL);
CREATE TABLE sync_books (library TEXT NOT NULL, sync_id TEXT NOT NULL, book_id TEXT, PRIMARY KEY (library, sync_id));
CREATE UNIQUE INDEX sync_books_local ON sync_books(library, book_id) WHERE book_id IS NOT NULL;
CREATE TABLE sync_changes (library TEXT NOT NULL, device TEXT NOT NULL, seq INTEGER NOT NULL, hash TEXT NOT NULL, PRIMARY KEY (library, device, seq));
CREATE TABLE sync_seen (library TEXT NOT NULL, device TEXT NOT NULL, seq INTEGER NOT NULL, hash TEXT NOT NULL, PRIMARY KEY (library, device));
CREATE TABLE sync_aside (library TEXT NOT NULL, device TEXT NOT NULL, hash TEXT NOT NULL, reason TEXT NOT NULL, size INTEGER NOT NULL, raw BLOB NOT NULL, first_at INTEGER NOT NULL, PRIMARY KEY (library, device, hash));

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
INSERT INTO extension_secrets VALUES ('org.example.explain', 'deepseek', 'api.deepseek.com', 'API key', 5000);
INSERT INTO sync_libraries VALUES ('00112233445566778899aabbccddeeff', 'icloud-drive', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 'on', 1, 1760702400123, 2, 6000, NULL);
INSERT INTO sync_versions VALUES ('00112233445566778899aabbccddeeff', 'place:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 3, 'aa', '{"format":1}', 1);
INSERT INTO sync_bodies VALUES ('ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad', X'616263');
INSERT INTO sync_books VALUES ('00112233445566778899aabbccddeeff', '7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6', 'b1');
INSERT INTO sync_changes VALUES ('00112233445566778899aabbccddeeff', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 3, 'aa');
INSERT INTO sync_seen VALUES ('00112233445566778899aabbccddeeff', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 3, 'aa');
INSERT INTO sync_aside VALUES ('00112233445566778899aabbccddeeff', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 'bb', 'format', 3, X'7B7D', 6100);
INSERT INTO sync_stopped VALUES ('00112233445566778899aabbccddeeff', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 'budget', 6200);
INSERT INTO sync_undo VALUES ('00112233445566778899aabbccddeeff', 'place:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 3, 'cc', X'7B7D', 6300, 1);
INSERT INTO sync_counted VALUES ('00112233445566778899aabbccddeeff', 6300, 'changed', 'places', '7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6', 0, 0);
INSERT INTO sync_question VALUES ('00112233445566778899aabbccddeeff', 0, 0, 6250, NULL, NULL, NULL, '{}', 6260, NULL, NULL);
INSERT INTO sync_deleted VALUES ('00112233445566778899aabbccddeeff', 'note:1a2b3c4d-5e6f-4a1b-8c2d-3e4f5a6b7c8d', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 'dd', 4, 6400, NULL, NULL, NULL);


    CREATE TABLE sync_late (
        library TEXT NOT NULL,              -- SP1: library ID
        device TEXT NOT NULL,               -- SP26: removed or quarantined writer
        key TEXT NOT NULL,                  -- SP11: the late version's record
        seq INTEGER NOT NULL,               -- SP11: the late version's sequence
        hash TEXT NOT NULL,                 -- SP11: the late version's hash
        bytes INTEGER NOT NULL,             -- SP26: body bytes; zero for a deletion
        reason TEXT NOT NULL CHECK(reason IN ('removed', 'quarantined')), -- SP26, SP30
        at INTEGER NOT NULL,                -- local time listed or Keep chosen
        state TEXT NOT NULL DEFAULT 'held' CHECK(state IN ('held', 'kept')), -- SP12: pending Keep
        kept TEXT,                          -- SP12: replacement hash, awaiting a sequence
        PRIMARY KEY (library, device, seq, key)
    );
    CREATE TABLE sync_late_over (
        library TEXT NOT NULL,              -- SP1: library ID
        device TEXT NOT NULL,               -- SP26: writer whose list overflowed
        dropped INTEGER NOT NULL,           -- SP26: unlisted changes blocking cleanup
        PRIMARY KEY (library, device)
    );
    CREATE TABLE sync_late_seen (
        library TEXT NOT NULL,              -- SP1: library ID
        device TEXT NOT NULL,               -- SP26: late writer
        seq INTEGER NOT NULL,               -- SP11: processed version's sequence
        key TEXT NOT NULL,                  -- SP11: processed version's record
        was INTEGER NOT NULL,              -- SP30: original state, applied or pending
        decided INTEGER NOT NULL DEFAULT 0, -- SP12: reader kept or discarded this version
        PRIMARY KEY (library, device, seq, key) -- survives choices and overflow
    );
    CREATE TABLE sync_late_passes (
        library TEXT NOT NULL,              -- SP1: library ID
        device TEXT NOT NULL,               -- SP26, SP30: stopped writer
        cutoff INTEGER NOT NULL,            -- SP29: effective cutoff of this processing pass
        seq INTEGER NOT NULL,               -- last processed sequence at that cutoff
        PRIMARY KEY (library, device, cutoff) -- repeat calls remain no-ops after recovery
    );

INSERT INTO sync_late VALUES ('00112233445566778899aabbccddeeff', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 'place:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6', 3, 'aa', 3, 'removed', 6500, 'held', NULL);
INSERT INTO sync_late_over VALUES ('00112233445566778899aabbccddeeff', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 1);
INSERT INTO sync_late_seen VALUES ('00112233445566778899aabbccddeeff', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 3, 'place:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6', 1, 0);
INSERT INTO sync_late_passes VALUES ('00112233445566778899aabbccddeeff', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 0, 3);


    CREATE TABLE sync_change_lists (
        library TEXT NOT NULL,              -- SP1: library ID; this device's own chain
        seq INTEGER NOT NULL,               -- SP15: includes changes with no versions
        reserve TEXT NOT NULL,              -- SP33: canonical JSON array
        hold TEXT NOT NULL,                 -- SP34: canonical JSON array
        PRIMARY KEY (library, seq)
    );

INSERT INTO sync_change_lists VALUES ('00112233445566778899aabbccddeeff', 3, '[]', '[]');


    CREATE TABLE sync_spine_versions (
        library TEXT NOT NULL,
        device TEXT NOT NULL,
        seq INTEGER NOT NULL,
        key_hash BLOB NOT NULL CHECK(length(key_hash) = 16),
        hash TEXT NOT NULL,
        PRIMARY KEY (library, device, key_hash, seq)
    );

INSERT INTO sync_spine_versions VALUES ('00112233445566778899aabbccddeeff', 'a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0', 2, X'00112233445566778899aabbccddeeff', '1kGsMbM3D4yDrNxfnsLZZW8NHDYHSPZ4bnVixzmJ7pA');

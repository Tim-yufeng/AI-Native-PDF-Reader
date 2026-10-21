CREATE TABLE documents(id TEXT PRIMARY KEY, current_revision TEXT,
 FOREIGN KEY(current_revision,id) REFERENCES revisions(id,document_id) DEFERRABLE INITIALLY DEFERRED);
CREATE TABLE revisions(
 id TEXT PRIMARY KEY, document_id TEXT NOT NULL REFERENCES documents(id),
 hash TEXT NOT NULL CHECK(length(hash)=64), pages INTEGER NOT NULL CHECK(pages>0),
 parent_id TEXT, source TEXT NOT NULL, UNIQUE(id,document_id),
 FOREIGN KEY(parent_id,document_id) REFERENCES revisions(id,document_id));
CREATE TABLE locations(
 document_id TEXT NOT NULL REFERENCES documents(id), file_key TEXT NOT NULL,
 hash TEXT NOT NULL, name_hint TEXT NOT NULL, PRIMARY KEY(document_id,file_key));
CREATE TABLE conversations(
 id TEXT PRIMARY KEY, document_id TEXT NOT NULL REFERENCES documents(id), UNIQUE(id,document_id));
CREATE TABLE messages(
 id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, document_id TEXT NOT NULL,
 revision_id TEXT NOT NULL, parent_id TEXT, edited_from TEXT,
 role TEXT NOT NULL CHECK(role IN ('user','assistant')),
 status TEXT NOT NULL CHECK(status IN ('streaming','complete','cancelled','failed')),
 content TEXT NOT NULL, next_seq INTEGER NOT NULL DEFAULT 0,
 metadata TEXT NOT NULL, created_ms INTEGER NOT NULL,
 UNIQUE(id,conversation_id),
 FOREIGN KEY(conversation_id,document_id) REFERENCES conversations(id,document_id),
 FOREIGN KEY(revision_id,document_id) REFERENCES revisions(id,document_id),
 FOREIGN KEY(parent_id,conversation_id) REFERENCES messages(id,conversation_id),
 FOREIGN KEY(edited_from,conversation_id) REFERENCES messages(id,conversation_id));
CREATE TABLE message_chunks(
 message_id TEXT NOT NULL REFERENCES messages(id), sequence INTEGER NOT NULL,
 content TEXT NOT NULL, PRIMARY KEY(message_id,sequence));
CREATE TABLE anchors(
 id TEXT PRIMARY KEY, message_id TEXT NOT NULL REFERENCES messages(id),
 document_id TEXT NOT NULL, revision_id TEXT NOT NULL, page INTEGER NOT NULL,
 rect TEXT NOT NULL, FOREIGN KEY(revision_id,document_id) REFERENCES revisions(id,document_id));
CREATE TABLE positions(
 document_id TEXT PRIMARY KEY REFERENCES documents(id), revision_id TEXT NOT NULL,
 page INTEGER NOT NULL, zoom REAL NOT NULL, scroll REAL NOT NULL,
 FOREIGN KEY(revision_id,document_id) REFERENCES revisions(id,document_id));
CREATE TABLE settings(key TEXT PRIMARY KEY, schema_version INTEGER NOT NULL, value TEXT NOT NULL);
CREATE TABLE save_events(
 event_id TEXT PRIMARY KEY, document_id TEXT NOT NULL REFERENCES documents(id),
 old_revision TEXT NOT NULL, new_revision TEXT NOT NULL, new_hash TEXT NOT NULL, new_file_key TEXT NOT NULL,
 FOREIGN KEY(old_revision,document_id) REFERENCES revisions(id,document_id),
 FOREIGN KEY(new_revision,document_id) REFERENCES revisions(id,document_id));
CREATE INDEX messages_conversation ON messages(conversation_id,created_ms);
CREATE INDEX revisions_hash ON revisions(hash);
CREATE VIRTUAL TABLE message_search USING fts5(message_id UNINDEXED,content);
CREATE TRIGGER message_initial AFTER INSERT ON messages BEGIN
 INSERT INTO message_search(message_id,content) VALUES(new.id,new.content);
END;
CREATE TRIGGER message_append AFTER UPDATE OF content ON messages BEGIN
 DELETE FROM message_search WHERE message_id=old.id;
 INSERT INTO message_search(message_id,content) VALUES(new.id,new.content);
END;
PRAGMA application_id=1095659602;
PRAGMA user_version=1;

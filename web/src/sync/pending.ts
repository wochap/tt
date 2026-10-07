// Counts local changes the server has not acknowledged. Every sync message
// from the server carries its current heads for that document; changes we
// hold beyond those heads are pending. Server heads persist in IndexedDB so
// an offline start still shows the queued count. The count is recomputed on
// demand (cheap: only changes after the server's heads are visited); any repo
// message (from a tab or the server) asks for a fresh status.

import * as A from "@automerge/automerge/slim";
import type { DocumentId, Message, Repo } from "@automerge/automerge-repo/slim";

const STORE = "heads";

export class PendingTracker {
  readonly #repo: Repo;
  readonly #serverHeads = new Map<DocumentId, A.Heads>();
  readonly #db: Promise<IDBDatabase | undefined>;
  readonly #onChange: () => void;

  constructor(repo: Repo, dbName: string, onChange: () => void) {
    this.#repo = repo;
    this.#onChange = onChange;
    this.#db = openDb(dbName).catch(() => undefined);
    void this.#load();
    repo.networkSubsystem.on("message", () => onChange());
    repo.on("document", ({ handle }) => {
      handle.on("change", () => onChange());
      void handle.whenReady().then(onChange, () => undefined);
    });
  }

  async #load(): Promise<void> {
    const db = await this.#db;
    if (!db) return;
    const tx = db.transaction(STORE, "readonly");
    const request = tx.objectStore(STORE).openCursor();
    await new Promise<void>((resolve) => {
      request.onsuccess = () => {
        const cursor = request.result;
        if (!cursor) return resolve();
        if (!this.#serverHeads.has(cursor.key as DocumentId)) {
          this.#serverHeads.set(cursor.key as DocumentId, cursor.value as A.Heads);
        }
        cursor.continue();
      };
      request.onerror = () => resolve();
    });
    this.#onChange();
  }

  observeServer(message: Message): void {
    const id = message.documentId;
    if (!id) return;
    if ((message.type === "sync" || message.type === "request") && message.data) {
      try {
        const heads = A.decodeSyncMessage(message.data).heads;
        this.#serverHeads.set(id, heads);
        void this.#persist(id, heads);
      } catch {
        return;
      }
    } else if (message.type === "doc-unavailable") {
      this.#serverHeads.set(id, []);
    }
    this.#onChange();
  }

  async #persist(id: DocumentId, heads: A.Heads): Promise<void> {
    const db = await this.#db;
    db?.transaction(STORE, "readwrite").objectStore(STORE).put(heads, id);
  }

  count(): number {
    let total = 0;
    for (const [id, handle] of Object.entries(this.#repo.handles)) {
      if (!handle.isReady()) continue;
      const doc = handle.doc();
      const server = this.#serverHeads.get(id as DocumentId) ?? [];
      const missing = new Set(A.getMissingDeps(doc, server));
      const known = server.filter((hash) => !missing.has(hash));
      total += A.getChangesMetaSince(doc, known).length;
    }
    return total;
  }

  async close(): Promise<void> {
    (await this.#db)?.close();
  }
}

function openDb(name: string): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

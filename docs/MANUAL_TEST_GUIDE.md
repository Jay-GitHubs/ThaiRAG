# Manual Test Guide — Chat UI, Source Viewer & Factory Reset

Step-by-step manual checks for the first-party chat UI, the in-app source
viewer, and the factory reset. Each case lists the **action / prompt to type**
and the **expected result**.

## Setup

| Thing | Value |
|---|---|
| Chat UI | http://localhost:8082 |
| Admin UI | http://localhost:8081 |
| Super-admin login | `admin@thairag.local` / `admin123` |
| Workspace with a scanned doc | **KMs** (holds `scanned_gazette_2486.pdf`, 2 pages with page renders) |

Notes:
- Inline source images are **enabled** (Settings → Vector Database, or
  `chat_pipeline.inline_images_enabled`). They appear only for docs whose pages
  were rendered (scanned / vision-OCR'd) — use **KMs**.
- The first streamed answer after a cold start can take 30–60s; that's the model
  warming up, not a hang (you should see a progress indicator — see §1.4).

---

## 1. Chat basics

### 1.1 First message with no conversation selected
- **Action:** Log in. Without clicking "New chat", type `สวัสดี ทำอะไรได้บ้าง` and Send.
- **Expected:** A conversation is created automatically and the answer streams
  in. (It must NOT stay blank — regression guard for the lazy-create bug.)

### 1.2 New chat + send
- **Action:** Click **New chat** → type `What can you help me with?` → Send.
- **Expected:** Answer streams token-by-token; conversation appears in the sidebar, auto-titled from your message.

### 1.3 Durable history
- **Action:** Send a couple of messages, then **reload the page** (F5).
- **Expected:** The conversation and all messages are still there.

### 1.4 Progress feedback on a slow query
- **Action:** New chat → scope **KMs** → ask `สรุปเอกสารทั้งหมดในคลังให้หน่อย`.
- **Expected:** Before the answer text appears, a spinner with a stage label
  ("Searching your documents", "Reading the most relevant parts", …) shows —
  not a blank bubble.

### 1.5 Stop
- **Action:** Ask a long question (`อธิบายระบบนี้ทีละขั้นตอนโดยละเอียด`), then click **Stop** mid-stream.
- **Expected:** Streaming halts, the partial answer stays, the composer re-enables.

### 1.6 Regenerate
- **Action:** After an answer, click **Regenerate**.
- **Expected:** The previous answer is replaced by a fresh one — the turn is NOT duplicated (still one user + one assistant).

### 1.7 Rename a conversation
- **Action:** Hover a conversation in the sidebar → click the pencil → type a new title → Enter.
- **Expected:** The title updates immediately and persists after reload.

### 1.8 Delete the active conversation
- **Action:** With a conversation open, click its trash icon → confirm Delete.
- **Expected:** It disappears from the sidebar AND the message pane clears to the
  empty state (regression guard — the pane must not keep showing the old chat).

### 1.9 Feedback thumbs
- **Action:** Under an answer, click 👍 (then reload).
- **Expected:** The thumb shows as selected and stays selected after reload.
  Clicking it again clears it.

### 1.10 Edit & resend a question
- **Action:** Hover your most recent question (the last user bubble) → click the
  pencil → change the text → **Send** (or Enter; Esc cancels).
- **Expected:** The edited question replaces the original (no duplicate turn) and
  a fresh answer streams. After reload, only the edited question is there — the
  original is gone (the backend swapped the rows, it did not orphan them).

### 1.11 Find a conversation
- **Action:** Type in the sidebar **Search** box.
- **Expected:** The list filters to matching titles instantly; conversations are
  grouped by recency (Today / Yesterday / Previous 7 days / …). Clearing the box
  restores the full grouped list.

### 1.12 Attach by drag-drop or paste
- **Action:** Drag a file onto the composer (it highlights), or paste an image
  (⌘V) into the text box.
- **Expected:** The file is staged as a chip showing its name **and size**.
  Over 10 MB (or more than 5 files) is rejected with a friendly message.

---

## 2. Scope selector

### 2.1 Pin to a workspace
- **Action:** New chat → in **Search in** pick **KMs** → ask
  `เอกสารฉบับนี้เกี่ยวกับอะไร`.
- **Expected:** The answer is grounded only in KMs content; the Sources strip cites the gazette.

### 2.2 All workspaces
- **Action:** New chat → leave scope on **All my workspaces** → ask any question.
- **Expected:** An answer streams (it may be slower — it searches every
  workspace you can access). It must not hang blank.

---

## 3. Source viewer (Phase 1) — in-app, highlighted

### 3.1 Open a source in-app (no new tab)
- **Action:** Ask a KMs question that returns a **Sources** strip, then click a source chip.
- **Expected:** A slide-over **drawer** opens *inside* the app (no new browser
  tab) showing the document text.

### 3.2 Highlighted passage
- **Expected (in the drawer):** The cited passage is **highlighted** and scrolled
  into view, with a "Cited from …" banner. An "Open full document in a new tab"
  link is available as a fallback.

### 3.3 Close + reopen
- **Action:** Close the drawer, click a different source chip.
- **Expected:** The drawer reopens for the new source with its own highlight.

---

## 3A. Verify a claim fast via the highlight

The point of the highlight is to confirm an answer is correct in ~2 seconds
instead of reading the whole document. Two highlight surfaces:

| View | Highlight |
|---|---|
| **Text** (converted doc) | Renders the converted document as **rich markup** — tables (xlsx / docx / csv), headings and lists — and highlights the matching **block** (table row / paragraph / list item), scrolled to. Works for **every** doc type. |
| **Document** (original PDF) | Cited passage highlighted **on the PDF page** — but only for **born-digital** PDFs (those with a real text layer). Scanned PDFs have no text layer, so the PDF view only **jumps to the page**; use the Text tab for the highlight there. |

### 3A.0 Highlight in a table (docx / xlsx / csv)
- **Setup:** `complex_table.docx` (a Thai/English table) has been uploaded to **KMs**.
- **Action:** New chat → scope **KMs** → ask `กลุ่ม A มีรายการอะไรบ้างและมูลค่าเท่าไร`
  → click the source chip.
- **Expected:** The drawer shows the document as a **rendered table** (not raw
  text), with the cited **row highlighted**. The answer's values match it.

### 3A.1 Highlight on the original PDF (born-digital)
- **Setup:** A born-digital PDF must be in the workspace. `borderless_table.pdf`
  (a sales table) has been uploaded to **KMs** for this.
- **Action:** New chat → scope **KMs** → ask
  `What were the Q1 and Q2 sales for the North and South regions?` → click the
  first source chip.
- **Expected:** The drawer opens on **Document**, the PDF renders, and the cited
  table cells are **highlighted (yellow) on the PDF page**, scrolled into view.
  The answer's numbers match the highlighted cells → verified.

### 3A.2 Highlight on the Text view (any doc, incl. scanned)
- **Action:** Ask a factual question (e.g. on the KMs gazette:
  `เอกสารฉบับนี้ประกาศใช้เมื่อใด`) → click the source chip → toggle to **Text**.
- **Expected:** The supporting sentence is highlighted and scrolled to. Confirm
  it backs the answer.

### 3A.3 Honest limits to check
- Highlight is a **best-effort text match** on the cited snippet (no character
  offsets stored). Clean prose/tables match well; heavily OCR'd text may not.
- If the passage can't be located, the Text view shows _"Couldn't pinpoint the
  exact passage — showing the full document"_, and the PDF view shows no
  highlight (page jump only). That's expected, not a crash.

---

## 4. Inline source images (Phase 3) — scanned docs

### 4.1 Source page image appears
- **Action:** New chat → scope **KMs** → ask `สรุปสาระสำคัญของเอกสารนี้`.
- **Expected:** Under the answer, the **Sources** strip shows the **source page
  image** (the rendered gazette page), not just text chips.

### 4.2 Image preview
- **Action:** Click the source image.
- **Expected:** It opens in a full-size preview.

> If no image appears: confirm the workspace is **KMs** (a text-only workspace
> won't have page renders) and that inline images are enabled in Settings.

---

## 5. Login methods

### 5.1 Native login
- **Action:** Log out → log in with email + password.
- **Expected:** Lands on the chat page.

### 5.2 SSO (if Keycloak is up)
- **Action:** On the login page, click **Continue with …**.
- **Expected:** Redirects to the IdP, then back into the chat, signed in.

### 5.3 Mobile
- **Action:** Open the chat on a phone-sized viewport.
- **Expected:** Sidebar collapses to a hamburger/drawer; composer and bubbles fit; chat works.

---

## 6. Factory reset (admin UI) — ⚠️ destructive

> Back up first. A reset cannot be undone (re-ingestion needed to restore content).

### 6.1 Open the control
- **Action:** Admin UI → **Settings** → **Vector Database** tab → expand **Danger Zone**.
- **Expected:** A **Factory Reset** panel with a scope picker, a mode option (for global), and a "Type RESET to confirm" box.

### 6.2 Confirmation guard
- **Action:** Leave the confirm box empty (or wrong text).
- **Expected:** The **Factory Reset** button is disabled. It enables only when you type `RESET`.

### 6.3 Scoped reset (safe to try on a throwaway workspace)
- **Action:** Scope = **Workspace** → pick a test workspace → type `RESET` → Factory Reset → confirm.
- **Expected:** Success toast; only that workspace's documents/vectors are gone;
  other workspaces, users, and settings are untouched.

### 6.4 Global content reset
- **Action:** Scope = **Everything (global)**, mode = **Content only** → `RESET` → confirm.
- **Expected:** All documents/chunks/vectors/BM25/conversations are wiped, but
  users, orgs, workspaces and settings remain (you stay logged in).

### 6.5 Re-ingest after reset
- **Action:** Upload a document into a workspace and query it.
- **Expected:** Ingestion succeeds and the new document is searchable — proving the indexes were cleanly rebuilt.

---

## 7. Quick regression checklist

- [ ] First message (no conversation) streams an answer
- [ ] Progress indicator shows during retrieval
- [ ] Stop / Regenerate work
- [ ] Rename / Delete (active conversation clears the pane)
- [ ] Feedback thumbs persist across reload
- [ ] Source chip → in-app drawer with highlighted passage
- [ ] Scanned-doc answer shows an inline source image
- [ ] Factory reset requires typing RESET; content reset keeps users/structure
